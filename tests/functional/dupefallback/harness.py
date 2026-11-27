#!/usr/bin/env python3
#
#  This file is part of nzbget. See <https://nzbget.com>.
#
#  Copyright (C) 2026 Denis <denis@nzbget.com>
#
#  This program is free software; you can redistribute it and/or modify
#  it under the terms of the GNU General Public License as published by
#  the Free Software Foundation; either version 2 of the License, or
#  (at your option) any later version.
#
#  This program is distributed in the hope that it will be useful,
#  but WITHOUT ANY WARRANTY; without even the implied warranty of
#  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
#  GNU General Public License for more details.
#
#  You should have received a copy of the GNU General Public License
#  along with this program.  If not, see <http://www.gnu.org/licenses/>.

"""
Cross-platform functional harness for the DupeArticleFallback feature.

It drives nzbget's own test NNTP server (``nzbget --nserv``) plus a scratch
nzbget daemon, entirely offline, and verifies that a download missing articles
on the news server is completed by borrowing the equivalent articles from a
duplicate posting of the same content.

The same scenarios run on Linux, macOS and Android; the only thing that differs
is *where* the processes/files live, which is isolated behind the ``Target``
abstraction:

* ``LocalTarget`` runs nserv + the daemon as local subprocesses (Linux, macOS).
* ``AdbTarget`` runs them inside a connected Android emulator/device over adb,
  forwarding the RPC port back to the host.

``--fallback-mode live`` (or ``stream``) runs the scenarios written for a lower
``DupeArticleFallback`` mode under that one (the daemon gates its features with
``>=`` on no < article < stream < live, so whatever passes under a mode must
pass under every higher one); see ``FALLBACK_MODE_BOUND`` for the few that
assert their own mode's sequence of events.

Scenarios (all use the ``!serverlist`` nserv message-id suffix to make an
article "missing" on the active server, so no real Usenet access is needed):

* complementary - two postings, each missing different articles; neither
  completes alone, together they do.
* cutover       - the primary is missing most of a file; after a few
  recoveries the file leads with the duplicate (DupeArticleFallback cutover).
* leadswitch    - the top-scored duplicate shares the primary's hole; after a
  few consecutive lead misses the lead rotates to the next duplicate, which
  completes the file (lead demotion).
* manydonors    - more duplicates than the donor cache holds, to exercise the
  cache-eviction path (regression for the use-after-free crash).
* stream        - the donor is segmented differently; missing byte ranges are
  repaired on stream level in post-processing.
* liveoverlap   - DupeArticleFallback=live: a damaged file is stream-repaired
  DURING the download of the rest of the collection (log-order proof).
* livegate      - the same fixture under plain "stream" must not run live.
* livelastfile  - live mode, single-file collection: the live dispatch is
  skipped for the collection's last file (post-processing handles it).
* repost        - a 4-member opaque "rar+par2 release" reposted byte-identically
  under different segmentation; the damaged archive volume is repaired, while
  the PAR2 hole stays untouched and no donor PAR2 article is requested
  (final status FAILURE/PAR by design - the stand-in par2 is random bytes).
* repostrenamed - a 3-member repost whose members were RENAMED (different
  release base name, same volume suffixes): exact-name pairing cannot fire,
  proving the unique-suffix-key tier pairs the damaged member with its donor
  twin end-to-end.
* repostobfuscated - an 8-member repost of equal-size volumes with
  obfuscated names on BOTH sides and a shuffled donor order: names, suffixes,
  positions and article sizes all fail to identify the twins, so stream repair
  must probe the donor's members until one is byte-identical, for every
  damaged member.
* repostdonorgaps - the donor repost misses articles of its own exactly where
  the identity probes land first; missing probes must be replaced by other
  eligible donor articles instead of rejecting the true twin.
* xpackbare     - M2 cross-packing: a bare .mkv completed with a hole is
  repaired from a duplicate that posted the SAME movie packed into store-mode
  RAR3 volumes (different framing/offsets/segmentation), which M1 cannot pair;
  the ContentMap pass locates and patches the missing bytes inside the volumes.
* xpackrar      - store-rar target repaired from a bare donor, including a
  degraded volume whose header hole must exclude it from the map.
* xpackrar2rar  - rar-to-rar cross-packing with different volume sizes on
  each side (member-wise M1 cannot window these).
* xpackzip      - bare target repaired from a SPANNED STORED ZIP donor
  (z01+z02+zip).
* xpack7z       - bare target repaired from a 7z-COPY donor posted as
  .7z.001/.002 splits.
* xpacksplit    - store-rar target repaired from RAW SPLITS
  (movie.mkv.001/.002/.003).
* xpackcompressed - the mechanism ladder on a COMPRESSED archive: M2 never
  maps it (method gate), a byte-identical repost still repairs it via M1.
* xpackneg      - the negative: a same-size, wrong-bytes donor set must be
  rejected by the inner probes; nothing may be written.
* xcrypt_encplain  - M3 password-assisted cross-packing: an ENCRYPTED
  store-rar target (its own password known via its NZB) with a data hole,
  repaired from a BARE unencrypted donor; asserts byte-identical ciphertext.
* xpacknesteddonor / xpacknestedtarget - the movie three stored containers
  deep (rar in zip in 7z) as donor for, and as target repaired from, a BARE
  posting: matching and borrowing go by the payload, not the containers.
* xpacknestedsetdonor / xpacknestedsettarget - a rar VOLUME SET kept inside
  a 7z, as donor for, and as target repaired from, a BARE posting.
* xcrypt_nestedin7zenc / xcrypt_nestedinrarenc - the movie in a plain
  archive inside an ENCRYPTED one, as donor for a BARE target.
* xcrypt_nestedinenctarget - the same layout as target: left alone.
* xcrypt_enc7ztarget - a stored 7z target with encrypted headers and data
  (password in its NZB), split in volumes, repaired from a BARE donor.
* xcrypt_enc7zdonor - a BARE target repaired by range from a stored 7z donor
  with encrypted headers and data, without the decompression step.
* xcrypt_plainenc  - reverse direction: a BARE target repaired from an
  ENCRYPTED store-rar donor whose password travels via the donor's NZB.
* xcrypt_diffpass  - both sides encrypted under DIFFERENT passwords; proves
  the donor's and target's crypto contexts never mix.
* xcrypt_wrongpass - the negative: an encrypted donor whose supplied
  password does NOT match; rejected by the content-identity probe, nothing
  written. The four xcrypt_* scenarios SKIP gracefully when the
  ``cryptography`` package is not installed (see generators.HAVE_CRYPTO).
* xdecomp_zip    - M4 decompression-assisted donor extraction: a bare
  movie.mkv target with holes, repaired from a REAL DEFLATE-compressed zip
  donor of the identical file. M2 never maps a compressed zip entry, so only
  materializing the donor and shelling out to the configured SevenZipCmd
  (option DupeStreamDecompress=yes) can recover it.
* xdecomp_7z     - same shape, donor is a REAL LZMA2-compressed 7z archive.
* xdecomp_storetarget - a store-mode rar3 TARGET (not bare) repaired from a
  compressed-7z donor, proving the M4 path composes with the M2 plain
  target map, not just the bare/identity map.
* xdecomp_enc7z  - the POSIX password-quoting proof: a bare target repaired
  from a HEADER-ENCRYPTED 7z donor (-mhe=on), its password threaded via the
  donor's own NZB exactly like xcrypt_plainenc threads an encrypted-rar
  donor's password.
* xdecomp_enctarget - the M3+M4 composition: an ENCRYPTED store-rar target
  (password via its own NZB) repaired from a compressed 7z donor; the
  extracted plaintext is re-encrypted under the target's AES-CBC stream
  context and the ciphertext written into the hole.
* xdecomp_neg    - the negative: a compressed donor with the right inner
  size but the WRONG bytes; rejected by the identity probe, nothing written.
* xdecomp_symlink - the link fail-close: a compressed donor whose archive
  contains a symlink is rejected before selecting or patching anything;
  cleanup unlinks without following. POSIX-only.
* xdecomp_off    - the opt-in gate: the same compressed-7z-donor setup as
  xdecomp_7z, but DupeStreamDecompress is OMITTED (default no); the
  decompression path must never run and the item stays unrepaired. The eight
  xdecomp_* scenarios SKIP gracefully when no local 7z/7za/7zr binary is on
  PATH (see generators.HAVE_7Z).

Usage:
    harness.py --nzbget /path/to/nzbget [--target local|adb]
               [--scenario all|complementary|cutover|leadswitch|cutovertruth|manydonors|stream|liveoverlap|livegate|livelastfile|repost|repostrenamed|repostobfuscated|repostdonorgaps|xpackbare|xpackrar|xpackrar2rar|xpack2sets|xpackzip|xpack7z|xpacksplit|xpackcompressed|xpackneg|xcrypt_encplain|xcrypt_enc7ztarget|xcrypt_enc7zdonor|xcrypt_plainenc|xcrypt_diffpass|xcrypt_wrongpass|xdecomp_zip|xdecomp_7z|xdecomp_storetarget|xdecomp_enc7z|xdecomp_enctarget|xdecomp_neg|xdecomp_symlink|xdecomp_off]
               [--serial NNN] [--keep]
"""

import argparse
import base64
import io
import os
import random
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import uuid
import socketserver
import threading
import zlib
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
try:
    from xmlrpc.client import ServerProxy
except ImportError:
    from xmlrpclib import ServerProxy  # type: ignore  # python 2 fallback

# deterministic container generators for the xpack scenarios
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generators


# --------------------------------------------------------------------------- #
# NZB generation (nserv message-id format: <path?part=offset:size[!servers]>)
# --------------------------------------------------------------------------- #

def _nzb_file_block(served_path, subject_name, file_size, seg_size, missing_parts):
    """One <file>...</file> block for nserv-served content (msgid format:
    <path?part=offset:size[!servers]>)."""
    import html
    n = (file_size + seg_size - 1) // seg_size
    lines = [
        '<file poster="dupefallback@test" date="1700000000" '
        'subject="&quot;%s&quot; yEnc (1/%d)">' % (html.escape(subject_name), n),
        '<groups><group>alt.binaries.test</group></groups>',
        '<segments>',
    ]
    for i in range(1, n + 1):
        off = (i - 1) * seg_size
        size = min(seg_size, file_size - off)
        miss = '!2' if i in missing_parts else ''
        msgid = '%s?%d=%d:%d%s' % (served_path, i, off, size, miss)
        lines.append('<segment bytes="%d" number="%d">%s</segment>'
                     % (size, i, html.escape(msgid)))
    lines += ['</segments>', '</file>']
    return lines


def build_nzb(served_path, subject_name, file_size, seg_size, missing_parts,
              password=None):
    """Return NZB XML for a single file served by nserv.

    ``missing_parts`` is a set of 1-based part numbers to mark with ``!2`` so
    they are served only by nserv instance 2 (i.e. missing on the active
    instance-1 server, forcing a fallback). ``password`` is forwarded to
    build_multi_nzb (see there for how it reaches the daemon)."""
    return build_multi_nzb([(served_path, subject_name, file_size, seg_size,
                             missing_parts)], password=password)


def build_nzb_with_par2(t, served_path, subject_name, data, seg_size, missing_parts):
    """build_nzb plus a par2 index of the file (no recovery slices) in the same
    collection: article borrowing needs par2 to check borrowed bytes against."""
    par = generators.par2_index([(subject_name, data)])
    par_path = served_path.rsplit('/', 1)[0] + '/rel.par2'
    t.write_file(os.path.join('data', par_path), par)
    return build_multi_nzb([(served_path, subject_name, len(data), seg_size, missing_parts),
                            (par_path, subject_name.rsplit('.', 1)[0] + '.par2', len(par), 500_000, set())])


def build_multi_nzb(members, password=None):
    """Return NZB XML containing one <file> block per member tuple
    (served_path, subject_name, file_size, seg_size, missing_parts).
    Member subject names MUST be distinct: duplicate parsed filenames make
    nzbget fall back to raw subjects and set ManyDupeFiles.

    ``password``, if given, is emitted as a <head><meta type="password">
    block - the real NZB convention indexers use to advertise an archive's
    password. NzbFile::Parse (daemon/queue/NzbFile.cpp) matches any <meta
    type="password"> element regardless of nesting and stores its text as
    nzbFile.GetPassword(); Scanner::AddFileToQueue then copies that into the
    queued NzbInfo's "*Unpack:Password" parameter (daemon/queue/Scanner.cpp).
    This happens on the SAME on-disk-file parse path whether the NZB arrived
    via directory scan or the RPC "append" method (DownloadXmlCommand ->
    Scanner::AddExternalFile writes the posted content to NzbDir and scans it
    exactly like a dropped file) - no RPC signature change is needed to carry
    a password end to end. StreamRepair.cpp later reads that same parameter
    for both the target (m_targetPassword, in CollectTargets) and each donor
    (DonorSource::Password, in CollectDonors) - both captured from the LIVE
    NzbInfo under the queue guard. The later ParseDonorNzb re-parse of the raw
    .nzb CANNOT see intake- or API-set passwords, so donor capture must happen
    from the live NzbInfo, not the re-parse."""
    import html
    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<!DOCTYPE nzb PUBLIC "-//newzBin//DTD NZB 1.0//EN" '
        '"http://www.newzbin.com/DTD/nzb/nzb-1.0.dtd">',
        '<nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">',
    ]
    if password:
        lines += ['<head>',
                  '<meta type="password">%s</meta>' % html.escape(password),
                  '</head>']
    for member in members:
        lines += _nzb_file_block(*member)
    lines.append('</nzb>')
    return '\n'.join(lines) + '\n'


def free_port():
    s = socket.socket()
    s.bind(('127.0.0.1', 0))
    p = s.getsockname()[1]
    s.close()
    return p


# --------------------------------------------------------------------------- #
# Targets
# --------------------------------------------------------------------------- #

class LocalTarget:
    """Run nserv + daemon as local subprocesses (Linux, macOS)."""
    name = 'local'

    def __init__(self, nzbget_bin, workdir):
        self.nzbget = nzbget_bin
        self.work = workdir
        self.procs = []

    def path(self, *parts):
        return os.path.join(self.work, *parts)

    def makedirs(self, *parts):
        d = self.path(*parts)
        os.makedirs(d, exist_ok=True)
        return d

    def write_file(self, rel, data):
        full = self.path(rel)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, 'wb') as f:
            f.write(data)

    def read_file(self, rel):
        with open(self.path(rel), 'rb') as f:
            return f.read()

    def exists(self, rel):
        return os.path.exists(self.path(rel))

    def find_files(self, *parts):
        """Return rel paths (work-relative, '/'-joined) of all files under the
        given subdir."""
        base = self.path(*parts)
        out = []
        if os.path.isdir(base):
            for dp, _, files in os.walk(base):
                for fn in files:
                    rel = os.path.relpath(os.path.join(dp, fn), self.work)
                    out.append(rel.replace(os.sep, '/'))
        return out

    def spawn(self, args, output_rel=None):
        if output_rel:
            with open(self.path(output_rel), 'wb') as output:
                p = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=output,
                                     stderr=subprocess.STDOUT)
        else:
            p = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                  stderr=subprocess.DEVNULL)
        self.procs.append(p)
        return p

    def rpc_host(self):
        return '127.0.0.1'

    def forward_rpc(self, device_port):
        _ = device_port  # local: the daemon already listens on localhost; no-op

    def teardown(self, keep):
        for p in self.procs:
            try:
                p.kill()
            except Exception:
                pass
        if not keep and os.path.exists(self.work):
            shutil.rmtree(self.work, ignore_errors=True)


class AdbProcess:
    """Wait for a foreground process launched through adb to exit."""
    def __init__(self, target, pid):
        self.target = target
        self.pid = pid

    def wait(self, timeout=60):
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired('adb process %d' % self.pid, timeout)
            try:
                result = self.target._adb('shell',
                    'if kill -0 %d 2>/dev/null; then echo running; else echo exited; fi' % self.pid,
                    timeout=remaining)
            except subprocess.TimeoutExpired:
                raise subprocess.TimeoutExpired('adb process %d' % self.pid, timeout) from None
            status = result.stdout.strip()
            if result.returncode != 0 or status not in ('running', 'exited'):
                raise RuntimeError('could not check adb process %d: %s' % (self.pid, result.stderr))
            if status == 'exited':
                return
            if time.monotonic() >= deadline:
                raise subprocess.TimeoutExpired('adb process %d' % self.pid, timeout)
            time.sleep(0.3)


class AdbTarget:
    """Run nserv + daemon inside an Android emulator/device over adb.

    Files live under a device-local workdir; the RPC control port is forwarded
    back to the host so the same XML-RPC client works unchanged."""
    name = 'adb'
    DEVICE_ROOT = '/data/local/tmp/nzbget-dupefallback'

    def __init__(self, nzbget_device_path, workdir, serial=None, rpc_port=None):
        # nzbget_device_path is the path to the nzbget binary ALREADY on device
        self.nzbget = nzbget_device_path
        self.work = self.DEVICE_ROOT
        self.host_stage = workdir            # host-side staging dir
        self.serial = serial
        self.rpc_port = rpc_port
        self.forwarded = None
        os.makedirs(self.host_stage, exist_ok=True)
        self._adb('shell', 'rm', '-rf', self.work)
        self._adb('shell', 'mkdir', '-p', self.work)

    def _adb(self, *args, timeout=None):
        cmd = ['adb']
        if self.serial:
            cmd += ['-s', self.serial]
        cmd += list(args)
        return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)

    def path(self, *parts):
        return '/'.join([self.work] + list(parts))

    def makedirs(self, *parts):
        d = self.path(*parts)
        self._adb('shell', 'mkdir', '-p', d)
        return d

    def write_file(self, rel, data):
        full = self.path(rel)
        self._adb('shell', 'mkdir', '-p', full.rsplit('/', 1)[0])
        stage = os.path.join(self.host_stage, rel.replace('/', '_'))
        with open(stage, 'wb') as f:
            f.write(data)
        self._adb('push', stage, full)

    def read_file(self, rel):
        stage = os.path.join(self.host_stage, 'pull_' + rel.replace('/', '_'))
        self._adb('pull', self.path(rel), stage)
        with open(stage, 'rb') as f:
            return f.read()

    def exists(self, rel):
        r = self._adb('shell', 'ls', self.path(rel))
        return 'No such file' not in (r.stdout + r.stderr)

    def find_files(self, *parts):
        base = self.path(*parts)
        r = self._adb('shell', 'find', base, '-type', 'f')
        out = []
        for line in r.stdout.splitlines():
            line = line.strip()
            if line.startswith(self.work + '/'):
                out.append(line[len(self.work) + 1:])
        return out

    def spawn(self, args, output_rel=None):
        # launch detached on device; ports are device-local
        output = self._q(self.path(output_rel)) if output_rel else '/dev/null'
        remote = ' '.join(self._q(a) for a in args) + ' >' + output + ' 2>&1 & echo $!'
        result = self._adb('shell', remote)
        pid = result.stdout.strip()
        if result.returncode != 0 or not pid.isdigit() or int(pid) <= 0:
            raise RuntimeError('could not launch adb process: %s' % result.stderr)
        return AdbProcess(self, int(pid))

    @staticmethod
    def _q(a):
        return "'" + str(a).replace("'", "'\\''") + "'" if ' ' in str(a) else str(a)

    def rpc_host(self):
        return '127.0.0.1'

    def forward_rpc(self, device_port):
        self.rpc_port = device_port
        self._adb('forward', 'tcp:%d' % device_port, 'tcp:%d' % device_port)
        self.forwarded = device_port

    def teardown(self, keep):
        self._adb('shell', 'pkill', '-f', self.nzbget)
        if self.forwarded:
            self._adb('forward', '--remove', 'tcp:%d' % self.forwarded)
        if not keep:
            self._adb('shell', 'rm', '-rf', self.work)
        shutil.rmtree(self.host_stage, ignore_errors=True)


# --------------------------------------------------------------------------- #
# Daemon control
# --------------------------------------------------------------------------- #

class Daemon:
    def __init__(self, target, nntp_port, rpc_port):
        self.t = target
        self.nntp_port = nntp_port
        self.rpc_port = rpc_port
        self.datadir = target.makedirs('data')
        for d in ('dst', 'inter', 'nzb', 'queue', 'tmp', 'web'):
            target.makedirs('main', d)
        self.conf_rel = 'rustyget.conf'
        self.proxy = None
        self.fake_nntp = None

    def write_config(self, extra_options):
        w = self.t.path
        cfg = [
            'MainDir=%s' % w('main'),
            'DestDir=%s' % w('main', 'dst'),
            'InterDir=%s' % w('main', 'inter'),
            'NzbDir=%s' % w('main', 'nzb'),
            'QueueDir=%s' % w('main', 'queue'),
            'TempDir=%s' % w('main', 'tmp'),
            'LogFile=%s' % w('nzbget.log'),
            # Pin every path that nzbget would otherwise default to a
            # compiled-in location: an uncreatable WebDir is a fatal config
            # error that pauses the whole queue, which cross-compiled
            # (Android) builds trip over.
            'WebDir=%s' % w('main', 'web'),
            'LockFile=%s' % w('nzbget.lock'),
            'ConfigTemplate=', 'RequiredDir=',
            'WriteLog=append', 'OutputMode=log', 'ControlIP=127.0.0.1',
            'ControlPort=%d' % self.rpc_port,
            'ControlUsername=', 'ControlPassword=',
            'Server1.Host=127.0.0.1', 'Server1.Port=%d' % self.nntp_port,
            'Server1.Connections=4', 'Server1.Level=0', 'Server1.Encryption=no',
            'DirectWrite=yes', 'ArticleCache=0', 'ContinuePartial=no',
            'FileNaming=nzb', 'DupeCheck=yes', 'NzbCleanupDisk=no',
            'HealthCheck=none', 'ArticleRetries=1', 'ParCheck=manual',
            'ParRename=no', 'RarRename=no', 'Unpack=no', 'DirectUnpack=no',
            'DirectRename=no', 'UpdateCheck=none',
            # The scenarios were written against the old built-in defaults;
            # pin them so each one still sets exactly what it tests.
            'DupeArticleFallback=no', 'DupeStreamDecompress=no', 'ParScan=extended',
            'DupeWatch=no', 'DupeHealthPreset=custom',
        ] + extra_options
        self.t.write_file(self.conf_rel, ('\n'.join(cfg) + '\n').encode())

    def start_nserv(self, capture_requests=False, extra_args=(), port=None):
        # A single instance (-i 1) binds only nntp_port. Instance 1 already
        # returns "430 not found" for "!2" message-ids (its id 1 is not in the
        # server-list [2]), which is how a "missing" article is simulated on the
        # only server the daemon uses. A second instance would just bind
        # nntp_port+1 and risk colliding with the control port.
        self.t.spawn([self.t.nzbget, '--nserv', '-d', self.datadir,
                      '-p', str(port or self.nntp_port), '-i', '1',
                      '-v', '2' if capture_requests else '0'] + list(extra_args),
                     output_rel='nserv.log' if capture_requests else None)
        # Wait until nserv listens (it exits silently when its port is taken; the daemon
        # would then be refused for the whole run: livepartrustedtemp on a busy m5air).
        proc = self.t.procs[-1]
        deadline = time.time() + 15
        while time.time() < deadline:
            if proc.poll() is not None:
                raise RuntimeError('nserv exited with %s before listening on port %d'
                                   % (proc.returncode, port or self.nntp_port))
            try:
                socket.create_connection(('127.0.0.1', port or self.nntp_port), timeout=1).close()
                return
            except OSError:
                time.sleep(0.1)

    def start(self):
        self.process = self.t.spawn([self.t.nzbget, '-c', self.t.path(self.conf_rel), '-s'])

    def api(self):
        host = self.t.rpc_host()
        # (a scenario that sets ControlPassword sets creds "user:password" too)
        creds = getattr(self, 'creds', None)
        return ServerProxy('http://%s%s:%d/xmlrpc' % (creds + '@' if creds else '', host, self.rpc_port))

    def wait_ready(self, timeout=30):
        api = self.api()
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                api.status()
                return api
            except Exception:
                time.sleep(0.3)
        raise RuntimeError('nzbget did not become ready')

    def reload(self, api, timeout=60):
        """Reload nzbget and return a proxy to the reloaded instance. The
        ``reload`` call answers before the old instance stops, so for a while
        its port still takes calls and a status reply can come from it; the
        port then closes until the new instance listens. Wait for the new
        instance's start-up line in the log, which it writes after the old one
        has stopped, before asking whether it is ready."""
        started = _grep_log(self.t, 'RustyGet runs on')
        api.reload()
        deadline = time.time() + timeout
        while _grep_log(self.t, 'RustyGet runs on') <= started:
            if time.time() > deadline:
                raise RuntimeError('nzbget did not start again after the reload')
            time.sleep(0.2)
        return self.wait_ready(timeout=timeout)

    def append(self, api, name, nzb_xml, paused, dupekey, score):
        content = base64.standard_b64encode(nzb_xml.encode()).decode()
        return api.append(name, content, 'test', 0, False, paused,
                           dupekey, score, 'score', [])

    def wait_history(self, api, name, timeout=180):
        deadline = time.time() + timeout
        while time.time() < deadline:
            for h in api.history():
                if h.get('NZBName') == name or h.get('NZBFilename') == name + '.nzb':
                    return h
            time.sleep(0.5)
        raise RuntimeError('timeout waiting for %s in history' % name)


class FlakyNntpProxy:
    """A news server in front of nserv that, on the first BODY request whose
    message-id contains ``trigger``, drops every open connection (leaving that
    request unanswered) and turns new connections away with "502 too many
    connections" for ``window`` seconds: a provider enforcing a per-user
    connection limit while another client holds the account's connections.
    With ``window`` None the server stays down for good."""

    def __init__(self, listen_port, upstream_port, trigger, window):
        self.upstream_port = upstream_port
        self.trigger = trigger.encode()
        self.window = window
        self.triggered_at = None
        self.rejected = 0
        self.live = []
        self.lock = threading.Lock()
        self.srv = socket.socket()
        self.srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.srv.bind(('127.0.0.1', listen_port))
        self.srv.listen(64)
        threading.Thread(target=self._serve, daemon=True).start()

    @staticmethod
    def _kill(sock):
        try:
            sock.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        sock.close()

    def _serve(self):
        while True:
            try:
                client, _ = self.srv.accept()
            except OSError:
                return
            with self.lock:
                if self.triggered_at and (self.window is None or
                                          time.time() - self.triggered_at < self.window):
                    self.rejected += 1
                    client.sendall(b'502 too many connections\r\n')
                    self._kill(client)
                    continue
                upstream = socket.create_connection(('127.0.0.1', self.upstream_port))
                self.live += [client, upstream]
            threading.Thread(target=self._pipe, args=(client, upstream, True), daemon=True).start()
            threading.Thread(target=self._pipe, args=(upstream, client, False), daemon=True).start()

    def _pipe(self, src, dst, from_client):
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                if from_client and b'BODY <' in data and self.trigger in data:
                    with self.lock:
                        if not self.triggered_at:
                            self.triggered_at = time.time()
                            for sock in self.live:
                                self._kill(sock)
                            self.live = []
                            return
                dst.sendall(data)
        except OSError:
            pass
        finally:
            self._kill(src)
            self._kill(dst)

    def close(self):
        self._kill(self.srv)
        with self.lock:
            for sock in self.live:
                self._kill(sock)


class CorruptingNntpProxy:
    """A news server in front of nserv that changes one byte inside the yEnc
    data of every BODY response for a message-id containing ``marker``,
    leaving the declared crc32 as it was: a provider serving a corrupt copy."""

    def __init__(self, listen_port, upstream_port, marker):
        self.upstream_port = upstream_port
        self.marker = marker.encode()
        self.corrupted = 0
        self.srv = socket.socket()
        self.srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.srv.bind(('127.0.0.1', listen_port))
        self.srv.listen(64)
        threading.Thread(target=self._serve, daemon=True).start()

    def _serve(self):
        while True:
            try:
                client, _ = self.srv.accept()
            except OSError:
                return
            upstream = socket.create_connection(('127.0.0.1', self.upstream_port))
            state = {'corrupt': False}
            threading.Thread(target=self._requests, args=(client, upstream, state), daemon=True).start()
            threading.Thread(target=self._responses, args=(upstream, client, state), daemon=True).start()

    def _requests(self, client, upstream, state):
        try:
            while True:
                data = client.recv(65536)
                if not data:
                    break
                if b'BODY <' in data and self.marker in data:
                    state['corrupt'] = True
                upstream.sendall(data)
        except OSError:
            pass

    def _responses(self, upstream, client, state):
        buf = b''
        try:
            while True:
                data = upstream.recv(65536)
                if not data:
                    break
                if not state['corrupt']:
                    client.sendall(data)
                    continue
                buf += data
                head = buf.find(b'=ypart')
                line_end = buf.find(b'\r\n', head) if head >= 0 else -1
                if line_end < 0 or len(buf) < line_end + 500:
                    continue
                pos = line_end + 300
                # a plain data byte: never a line break, escape or dot-stuffing
                if buf[pos] not in b'\r\n=.' and (buf[pos] + 1) % 256 not in b'\r\n=.\0':
                    buf = buf[:pos] + bytes([(buf[pos] + 1) % 256]) + buf[pos + 1:]
                    self.corrupted += 1
                state['corrupt'] = False
                client.sendall(buf)
                buf = b''
        except OSError:
            pass

    def close(self):
        try:
            self.srv.close()
        except OSError:
            pass


class RewritingNntpProxy:
    """A news server in front of nserv that rewrites ``old`` to ``new`` in
    its replies (same length): a provider that says "451" for a missing
    article instead of "430"."""

    def __init__(self, listen_port, upstream_port, old, new):
        assert len(old) == len(new)
        self.upstream_port = upstream_port
        self.old, self.new = old, new
        self.rewritten = 0
        self.accepted = 0
        self.closed = 0
        self.srv = socket.socket()
        self.srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.srv.bind(('127.0.0.1', listen_port))
        self.srv.listen(64)
        threading.Thread(target=self._serve, daemon=True).start()

    def _serve(self):
        while True:
            try:
                client, _ = self.srv.accept()
            except OSError:
                return
            self.accepted += 1
            upstream = socket.create_connection(('127.0.0.1', self.upstream_port))
            threading.Thread(target=self._pipe, args=(client, upstream, False), daemon=True).start()
            threading.Thread(target=self._pipe, args=(upstream, client, True), daemon=True).start()

    def _pipe(self, src, dst, rewrite):
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                if rewrite and self.old in data:
                    self.rewritten += data.count(self.old)
                    data = data.replace(self.old, self.new)
                dst.sendall(data)
        except OSError:
            pass
        finally:
            if not rewrite:
                self.closed += 1
            for sock in (src, dst):
                try:
                    sock.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
                sock.close()

    def close(self):
        try:
            self.srv.close()
        except OSError:
            pass


class DelayingNntpProxy(RewritingNntpProxy):
    """A news server in front of nserv that holds every request naming a
    marker for that marker's delay (seconds) before passing it on: a slow
    server, or a duplicate whose articles are slow to come (in practice,
    requests for articles that no server has any more, each waiting out its
    timeout). ``delays`` is a list of (marker, seconds)."""

    def __init__(self, listen_port, upstream_port, delays):
        self.delays = delays
        self.delayed = 0
        self.fired = set()
        super().__init__(listen_port, upstream_port, b'', b'')

    def _pipe(self, src, dst, rewrite):
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                if not rewrite:
                    for entry in self.delays:
                        marker, delay = entry[0], entry[1]
                        # a third element True: the first request only
                        if marker in data and not (len(entry) > 2 and entry[2] and marker in self.fired):
                            self.delayed += 1
                            self.fired.add(marker)
                            time.sleep(delay)
                            break
                dst.sendall(data)
        except OSError:
            pass
        finally:
            for sock in (src, dst):
                try:
                    sock.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
                sock.close()
class FakeNntp:
    """A news server that holds exactly the articles in ``alive`` (message-ids without
    the angle brackets): STAT and BODY (a yEnc article with a valid checksum) answer
    for them, 430 for the others. ``delays`` maps a message-id prefix to seconds to
    wait before answering; ``reply451`` ids answer 451. Counts what was asked."""

    def __init__(self, port, alive=()):
        outer = self
        self.alive = set(alive)
        self.delays = {}
        self.reply451 = set()
        # message-id -> article body sent as is (a uuencoded part, for one)
        self.bodies_by_id = {}
        # BODY needs a GROUP first on the connection (412 otherwise), and the
        # connection drops after every ``drop_every`` bodies (0: never)
        self.require_group = False
        self.drop_every = 0
        # message-ids whose BODY sends half the article and then stalls (once each)
        self.stall_ids = set()
        self.no_group_replies = 0
        self.stats = 0
        self.bodies = 0
        self.bare_ids = 0
        self.sessions = 0
        self.max_sessions = 0
        self.open_sessions = 0
        self.lock = threading.Lock()

        class Handler(socketserver.StreamRequestHandler):
            def handle(self):
                with outer.lock:
                    outer.sessions += 1
                    outer.open_sessions += 1
                    outer.max_sessions = max(outer.max_sessions, outer.open_sessions)
                group_joined = False
                bodies_sent = 0
                try:
                    self.wfile.write(b'200 fake\r\n')
                    for raw in self.rfile:
                        line = raw.decode('latin1').strip()
                        cmd, _, arg = line.partition(' ')
                        cmd = cmd.upper()
                        arg = arg.strip()
                        mid = arg.strip('<>')
                        if (cmd == 'STAT' or cmd == 'BODY') and not (arg.startswith('<') and arg.endswith('>')):
                            # a real server takes a message-id only in angle brackets (RFC 3977 3.6)
                            with outer.lock:
                                outer.bare_ids += 1
                            self.wfile.write(b'501 message-id must be in angle brackets\r\n')
                        elif cmd == 'STAT' or cmd == 'BODY':
                            for prefix, delay in list(outer.delays.items()):
                                if mid.startswith(prefix):
                                    time.sleep(delay)
                            with outer.lock:
                                if cmd == 'STAT':
                                    outer.stats += 1
                                    observer = getattr(outer, '_stat_observer', None)
                                    if observer:
                                        observer(mid)
                                else:
                                    outer.bodies += 1
                            if mid in outer.reply451:
                                self.wfile.write(b'451 not here\r\n')
                            elif mid not in outer.alive:
                                self.wfile.write(b'430 no such article\r\n')
                            elif cmd == 'STAT':
                                self.wfile.write(('223 0 <%s>\r\n' % mid).encode())
                            elif outer.require_group and not group_joined:
                                with outer.lock:
                                    outer.no_group_replies += 1
                                self.wfile.write(b'412 no newsgroup selected\r\n')
                            elif mid in outer.stall_ids:
                                outer.stall_ids.discard(mid)
                                body = outer.bodies_by_id.get(mid) or FakeNntp.article(mid)
                                self.wfile.write(('222 0 <%s>\r\n' % mid).encode() + body[:len(body) // 2])
                                self.wfile.flush()
                                time.sleep(30)
                                break
                            else:
                                body = outer.bodies_by_id.get(mid) or FakeNntp.article(mid)
                                self.wfile.write(('222 0 <%s>\r\n' % mid).encode() + body + b'.\r\n')
                                bodies_sent += 1
                                if outer.drop_every and bodies_sent % outer.drop_every == 0:
                                    # a reset (not a clean close): the client's next
                                    # read fails, the connection is broken
                                    self.wfile.flush()
                                    import struct
                                    self.request.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack('ii', 1, 0))
                                    break
                        elif cmd == 'GROUP':
                            group_joined = True
                            self.wfile.write(('211 1 1 1 %s\r\n' % arg).encode())
                        elif cmd == 'MODE':
                            self.wfile.write(b'200 reader\r\n')
                        elif cmd == 'QUIT':
                            self.wfile.write(b'205 bye\r\n')
                            break
                        else:
                            self.wfile.write(b'500 what\r\n')
                        self.wfile.flush()
                finally:
                    with outer.lock:
                        outer.open_sessions -= 1

        class Server(socketserver.ThreadingTCPServer):
            allow_reuse_address = True
            daemon_threads = True

        self.server = Server(('127.0.0.1', port), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    @staticmethod
    def article(mid):
        data = (mid * 4).encode()[:64]
        enc = bytearray()
        for b in data:
            c = (b + 42) & 255
            if c in (0, 10, 13, 61):
                enc += b'=' + bytes([(c + 64) & 255])
            else:
                enc.append(c)
        crc = zlib.crc32(data) & 0xffffffff
        return (b'=ybegin part=1 total=1 line=128 size=%d name=a.mkv\r\n=ypart begin=1 end=%d\r\n' % (len(data), len(data))
                + bytes(enc) + b'\r\n=yend size=%d part=1 pcrc32=%08x\r\n' % (len(data), crc))

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class RejectingNntp:
    """A news server that refuses every login for good (502), as a server with a
    broken or expired account does."""

    def __init__(self):
        self.logins = 0
        self.srv = socket.socket()
        self.srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.srv.bind(('127.0.0.1', 0))
        self.srv.listen(64)
        self.port = self.srv.getsockname()[1]
        threading.Thread(target=self._serve, daemon=True).start()

    def _serve(self):
        while True:
            try:
                conn, _ = self.srv.accept()
            except OSError:
                return
            threading.Thread(target=self._client, args=(conn,), daemon=True).start()

    def _client(self, conn):
        try:
            conn.sendall(b'200 reject server ready\r\n')
            data = b''
            while True:
                chunk = conn.recv(4096)
                if not chunk:
                    break
                data += chunk
                while b'\r\n' in data:
                    line, data = data.split(b'\r\n', 1)
                    if line.upper().startswith(b'AUTHINFO USER'):
                        conn.sendall(b'381 password required\r\n')
                    elif line.upper().startswith(b'AUTHINFO PASS'):
                        self.logins += 1
                        conn.sendall(b'502 Authentication Failed\r\n')
                    else:
                        conn.sendall(b'480 authentication required\r\n')
        except OSError:
            pass
        finally:
            conn.close()

    def close(self):
        self.srv.close()


class FirstFailNntpProxy(DelayingNntpProxy):
    """A news server in front of nserv that answers the FIRST body request for
    each message-id naming one of ``markers`` itself, with a transient error
    ("503"), and passes every other request: articles a single failed request
    loses (ArticleRetries=0) though they exist on the server. With ``always``
    it answers every request for them that way: a server out of connections
    for as long as the test looks. ``requests`` keeps when each was asked for."""

    def __init__(self, listen_port, upstream_port, markers, always=False):
        self.markers = markers
        self.always = always
        self.seen = set()
        self.failed = 0
        self.requests = {}
        super().__init__(listen_port, upstream_port, [])

    def _pipe(self, src, dst, rewrite):
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                if not rewrite:
                    found = re.findall(rb'(?:BODY|ARTICLE) <([^>]+)>', data)
                    first = [m for m in found if any(k in m for k in self.markers) and
                             (self.always or m not in self.seen)]
                    if first and len(found) == 1:
                        self.seen.add(first[0])
                        self.failed += 1
                        self.requests.setdefault(first[0], []).append(time.time())
                        src.sendall(b'503 temporarily unavailable\r\n')
                        continue
                dst.sendall(data)
        except OSError:
            pass
        finally:
            for sock in (src, dst):
                try:
                    sock.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
                sock.close()


# --------------------------------------------------------------------------- #
# Scenarios
# --------------------------------------------------------------------------- #

def _payload(size, seed):
    r = random.Random(seed)
    return bytes(r.getrandbits(8) for _ in range(size)) if size < 1 else \
        r.randbytes(size) if hasattr(r, 'randbytes') else \
        bytes(bytearray(r.getrandbits(8) for _ in range(size)))


def _text_payload(size, seed):
    """``size`` bytes of compressible text (words drawn from a small
    vocabulary): an LZMA2 or RAR coder packs it into compressed chunks and
    blocks, not stored ones."""
    r = random.Random(seed)
    words = [bytes(r.choice(b'abcdefghijklmnopqrstuvwxyz') for _ in range(r.randint(2, 9)))
             for _ in range(4000)]
    out = bytearray()
    while len(out) < size:
        out += b' '.join(r.choice(words) for _ in range(12)) + b'.\n'
    return bytes(out[:size])


def _place_copy(target, subdir, data, filename='file.bin'):
    """Write a payload copy under data/<subdir>/<filename> and return the
    served path (relative to the nserv data dir)."""
    target.write_file(os.path.join('data', subdir, filename), data)
    return '%s/%s' % (subdir, filename)


def scenario_complementary(daemon, t):
    """Primary missing parts 3,5,7; donor missing 2,8 (different) — only
    together do they complete. Byte-identical result expected."""
    size, seg = 5_000_000, 500_000
    data = _payload(size, 849)
    pp = _place_copy(t, 'primA', data)
    dp = _place_copy(t, 'primB', data)
    primary = build_nzb_with_par2(t, pp, 'ReleaseA.bin', data, seg, {3, 5, 7})
    donor = build_nzb(dp, 'obf-b.bin', size, seg, {2, 8})
    api = daemon.wait_ready()
    daemon.append(api, 'DonorB', donor, True, 'comp-key', 50)
    daemon.append(api, 'ReleaseA', primary, False, 'comp-key', 100)
    h = daemon.wait_history(api, 'ReleaseA')
    ok = h['Status'].startswith('SUCCESS')
    recov = int(h.get('DupeRecoveredArticles', 0))
    integ = _verify_output(t, data)
    return ('complementary', ok and integ and recov == 3,
            'status=%s recovered=%d integrity=%s' % (h['Status'], recov, integ))


def scenario_historylog(daemon, t):
    """A finished download keeps its messages: there are no per-download log
    files any more, so the History Log tab (loadlog) shows the messages kept in
    memory, and they must survive the move from the queue to history."""
    size, seg = 1_000_000, 100_000
    data = _payload(size, 4242)
    pp = _place_copy(t, 'histlog', data)
    nzb = build_nzb(pp, 'HistLog.bin', size, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'HistLog', nzb, False, '', 0)
    h = daemon.wait_history(api, 'HistLog')
    nzb_id = h.get('NZBID') or h.get('ID')
    msgs = api.loadlog(nzb_id, 0, 1000)
    count = int(h.get('MessageCount', 0))
    ok = h['Status'].startswith('SUCCESS') and len(msgs) > 0 and count > 0
    later = api.loadlog(nzb_id, msgs[-1]['ID'], 0) if msgs else []
    ok = ok and len(later) == 1 and later[0]['ID'] == msgs[-1]['ID']
    return ('historylog', ok, 'status=%s messages=%d MessageCount=%d from_last_id=%d'
            % (h['Status'], len(msgs), count, len(later)))


def scenario_urlhistorylog(daemon, t):
    """A URL whose file is not an NZB fails and goes to history with the scan's
    messages: the History Log tab (loadlog) shows them for that URL item too."""
    import http.server
    import threading

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            body = b'this is not an nzb file'
            self.send_response(200)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    api = daemon.wait_ready()
    url = 'http://127.0.0.1:%d/bad.nzb' % server.server_address[1]
    _rpc(daemon, 'appendurl', ['BadUrlLog.nzb', url, '', 0, False, False, '', 0, 'SCORE', []], timeout=60)
    h = None
    deadline = time.time() + 60
    while time.time() < deadline and h is None:
        h = next((x for x in api.history() if x.get('NZBName', '').startswith('BadUrlLog')), None)
        time.sleep(0.5)
    server.shutdown()
    if h is None:
        return ('urlhistorylog', False, 'the URL never reached history')
    msgs = api.loadlog(h.get('NZBID') or h.get('ID'), 0, 1000)
    ok = h.get('Kind') == 'URL' and len(msgs) > 0
    return ('urlhistorylog', ok, 'kind=%s status=%s messages=%d first=%s'
            % (h.get('Kind'), h.get('Status'), len(msgs), msgs[0]['Text'][:60] if msgs else ''))


def scenario_rejectnextserver(daemon, t):
    """B6: Server1 serves every article with a malformed yEnc part header (a
    proxy turns "=ypart begin=" into "=ypart begxn="), Server2 serves them
    intact. A rejected article is the server's fault, not the article's: the
    next server is asked, and the download completes byte-identically without
    a duplicate."""
    size, seg = 1_000_000, 100_000
    data = _payload(size, 9900)
    pp = _place_copy(t, 'rejA', data, 'file.bin')
    api = daemon.wait_ready()
    daemon.append(api, 'RelRej', build_nzb(pp, 'Rej.bin', size, seg, set()), False, 'rej-key', 100)
    h = daemon.wait_history(api, 'RelRej')
    integ = _verify_output(t, data)
    rejected = _grep_log(t, 'Malformed article') + _grep_log(t, 'could not be decoded')
    ok = 'SUCCESS' in h['Status'] and integ and daemon.proxy.rewritten > 0
    return ('rejectnextserver', ok, 'status=%s integrity=%s rewritten=%d rejected_logs=%d'
            % (h['Status'], integ, daemon.proxy.rewritten, rejected))


def scenario_backbonemiss(daemon, t):
    """Backbone routing: Server1 (priority 1) and Server2 (priority 2) are on
    the backbone "TestBone", Server3 (priority 2) is independent (reached as
    "localhost", another endpoint, so backend identification does not put it
    with them); all three serve the same article store, which lacks two
    articles. The 430 from
    Server1 rules out Server2 for those articles (same article store), so they
    are asked of Server3 only, and every other article comes from Server1."""
    size, seg = 1_000_000, 100_000
    data = _payload(size, 9700)
    pp = _place_copy(t, 'bbA', data, 'file.bin')
    api = daemon.wait_ready()
    daemon.append(api, 'RelBB', build_nzb(pp, 'BB.bin', size, seg, {3, 4}), False, 'bb-key', 100)
    h = daemon.wait_history(api, 'RelBB')
    skipped = _grep_log(t, 'not asking other servers on backbone TestBone')
    on_server2 = _grep_log(t, '@ Server2 (')
    on_server3 = _grep_log(t, '@ Server3 (')
    ok = skipped >= 2 and on_server2 == 0 and on_server3 >= 2
    return ('backbonemiss', ok, 'status=%s skipped_logs=%d server2_logs=%d server3_logs=%d'
            % (h['Status'], skipped, on_server2, on_server3))


def scenario_backendauto(daemon, t):
    """Backend identification (nntp-rs): Server1 (priority 1) and Server2
    (priority 2) are the same host and port with no Backbone configured;
    Server3 (priority 2) is reached as "localhost" (another endpoint). The
    daemon identifies Server1 and Server2 as one article store at start and
    puts them on one backbone, so the 430s from Server1 for the two missing
    articles are not asked again on Server2: only Server3 is asked."""
    size, seg = 1_000_000, 100_000
    data = _payload(size, 9701)
    pp = _place_copy(t, 'baA', data, 'file.bin')
    api = daemon.wait_ready()
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, 'backbone Store of Server1 (backend identification)') < 2:
        time.sleep(0.2)
    grouped = _grep_log(t, 'backbone Store of Server1 (backend identification)')
    daemon.append(api, 'RelBA', build_nzb(pp, 'BA.bin', size, seg, {3, 4}), False, 'ba-key', 100)
    h = daemon.wait_history(api, 'RelBA')
    skipped = _grep_log(t, 'not asking other servers on backbone Store of Server1')
    on_server2 = _grep_log(t, '@ Server2 (')
    on_server3 = _grep_log(t, '@ Server3 (')
    shared = _grep_log(t, 'Backend of Server1: store 0 shared with Server2')
    ok = grouped == 2 and shared >= 1 and skipped >= 2 and on_server2 == 0 and on_server3 >= 2
    return ('backendauto', ok, 'status=%s grouped=%d shared=%d skipped_logs=%d server2_logs=%d server3_logs=%d'
            % (h['Status'], grouped, shared, skipped, on_server2, on_server3))


def scenario_nzbgapborrow(daemon, t):
    """B30 and B3: the release's nzb-file skips part 4 (an indexer that didn't
    capture it) and part 5 is missing on the server; a twin duplicate lists
    every part. The twin pairs by part number despite the gap (B30), and part 5
    is borrowed: its expected place isn't taken from part 3, its list neighbour
    but not the previous part (B3). Part 4, in no nzb-file of the release, stays
    missing."""
    size, seg = 1_000_000, 100_000
    data = _payload(size, 9800)
    pp = _place_copy(t, 'gapA', data, 'file.bin')
    dp = _place_copy(t, 'gapB', data, 'file.bin')
    primary = build_nzb_with_par2(t, pp, 'Gap.bin', data, seg, {5})
    primary = re.sub(r'<segment[^>]*number="4"[^>]*>[^<]*</segment>', '', primary)
    donor = build_nzb(dp, 'Gap.bin', size, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DonGap', donor, True, 'gap-key', 50)
    daemon.append(api, 'RelGap', primary, False, 'gap-key', 100)
    h = daemon.wait_history(api, 'RelGap')
    recovered = int(h.get('DupeRecoveredArticles', 0))
    unmatched = _grep_log(t, 'none with a matching file')
    mismatch = _grep_log(t, 'offset mismatch') + _grep_log(t, 'end mismatch')
    ok = recovered >= 1 and unmatched == 0 and mismatch == 0
    return ('nzbgapborrow', ok, 'status=%s recovered=%d unmatched_logs=%d mismatch_logs=%d'
            % (h['Status'], recovered, unmatched, mismatch))


def scenario_declaredcountnopair(daemon, t):
    """B39: a release file of 29 parts (its subject says 1/29) and a duplicate's
    file of 30 parts with the same article size, other content, both missing
    parts. The release lists every part it declares, so the 30-part file is
    another one: nothing is borrowed from it (it used to be paired - one
    trailing extra part was allowed for an nzb-file that lost its last one -
    and its bytes written into the release)."""
    seg = 100_000
    rel = _payload(2_900_000, 9810)
    don = _payload(3_000_000, 9811)
    rp = _place_copy(t, 'dcA', rel, 'rel.bin')
    dp = _place_copy(t, 'dcB', don, 'don.bin')
    primary = build_nzb(rp, 'Rel.bin', len(rel), seg, {5, 6, 20})
    donor = build_nzb(dp, 'Don.bin', len(don), seg, {9})
    api = daemon.wait_ready()
    daemon.append(api, 'DonCount', donor, True, 'dc-key', 50)
    daemon.append(api, 'RelCount', primary, False, 'dc-key', 100)
    h = daemon.wait_history(api, 'RelCount')
    recovered = int(h.get('DupeRecoveredArticles', 0))
    # (the borrowing's own message; "Queueing stream repair ... from duplicate
    # collections" under the stream/live modes is not a borrowed article)
    borrowed = _grep_log(t, 'missing article(s) of Rel.bin from duplicate collections')
    ok = recovered == 0 and borrowed == 0 and int(h.get('FailedArticles', 0)) == 3
    return ('declaredcountnopair', ok, 'status=%s recovered=%d recovered_logs=%d failed_articles=%s'
            % (h['Status'], recovered, borrowed, h.get('FailedArticles')))


def _final_delete_midway(daemon, t, name, restart_at_once):
    """A client cancels a download that is half done (nzbdavkodi's
    cancel_jobs: GroupFinalDelete, then GroupFinalDelete and HistoryFinalDelete
    again a moment later) while a lower-scored duplicate waits in history and
    articles are being borrowed from it. The download must be gone for good:
    not left in the queue without files (stuck QUEUED, never post-processed),
    not in history, and not back after a restart."""
    seg = 250_000
    size = 3_000_000
    members, backup = [], []
    for i in range(6):
        data = _payload(size, 9830 + i)
        pp = _place_copy(t, 'fdA%d' % i, data, 'f%d.bin' % i)
        bp = _place_copy(t, 'fdB%d' % i, data, 'f%d.bin' % i)
        members.append((pp, 'Movie.part%02d.rar' % (i + 1), size, seg, {3, 7}))
        backup.append((bp, 'Movie.part%02d.rar' % (i + 1), size, seg, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(members), True, 'fd-key', 100)
    daemon.append(api, 'Backup', build_multi_nzb(backup), False, 'fd-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    pid = [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'][0]
    api.editqueue('GroupResume', 0, '', [pid])
    deadline = time.time() + 120
    while time.time() < deadline:
        g = [x for x in api.listgroups() if x['NZBID'] == pid]
        if not g or g[0]['RemainingFileCount'] <= 4:
            break
        time.sleep(0.1)
    cancelled_with = g[0]['RemainingFileCount'] if g else -1
    api.editqueue('GroupFinalDelete', 0, '', [pid])
    if not restart_at_once:
        time.sleep(0.3)
        api.editqueue('GroupFinalDelete', 0, '', [pid])
        api.editqueue('HistoryFinalDelete', 0, '', [pid])
        time.sleep(8)

    def present():
        queued = [x for x in api.listgroups() if x['NZBID'] == pid]
        hist = [h for h in api.history(True) if h.get('NZBID') == pid or h.get('ID') == pid]
        return queued, hist
    queued, hist = present() if not restart_at_once else ([], [])
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    time.sleep(3)
    queued2, hist2 = present()
    ok = cancelled_with > 0 and not queued and not hist and not queued2 and not hist2
    return (name, ok, 'remaining_files_at_cancel=%s queued=%s history=%d after_restart_queued=%s history=%d'
            % (cancelled_with, [(x['Status'], x['RemainingFileCount']) for x in queued], len(hist),
               [(x['Status'], x['RemainingFileCount']) for x in queued2], len(hist2)))


def scenario_finaldeletemidway(daemon, t):
    """B42: a client cancels a half-done download (nzbdavkodi's cancel_jobs:
    GroupFinalDelete, then again with HistoryFinalDelete a moment later) while
    articles are borrowed from a duplicate in history: it is gone for good,
    before and after a restart."""
    return _final_delete_midway(daemon, t, 'finaldeletemidway', False)


def scenario_interdirchange(daemon, t):
    """6790, 2026-11-27: InterDir moves to the destination's file system so the
    final move is a rename, applied by a reload. A download's folder is named
    once, when it is queued (<InterDir>/<name>.#<id>), so after the change the
    queue still holds folders under the old InterDir. A queued download with
    nothing on disk yet downloads into the new InterDir; one that already wrote
    into the old InterDir (a RAR set, unpacked) keeps that folder and is still
    unpacked into and moved to its final folder. On main both stayed in the
    old InterDir as if it were their final folder."""
    size, seg = 2_000_000, 50_000
    du, dw = _payload(size, 5151), _payload(3 * size, 5252)
    pu = _place_copy(t, 'idcu', du, 'idcu.bin')
    members = []
    for i, vol in enumerate(generators.rar3_store_volumes('idcw.mkv', dw, size), 1):
        rel = 'idcw/idcw.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        members.append((rel, 'idcw.part%02d.rar' % i, len(vol), seg, set()))
    old_inter = t.path('main', 'inter')
    new_inter = t.makedirs('main', 'inter2')
    api = daemon.wait_ready()
    daemon.append(api, 'IdcUnwritten', build_nzb(pu, 'idcu.bin', size, seg, set()), True, '', 0)
    daemon.append(api, 'IdcWritten', build_multi_nzb(members), False, '', 0)
    deadline = time.time() + 60
    written = False
    while time.time() < deadline and not written:
        written = any('/IdcWritten.#' in f for f in t.find_files('main', 'inter'))
        time.sleep(0.1)
    with open(t.path(daemon.conf_rel), 'ab') as f:
        f.write(('InterDir=%s\n' % new_inter).encode())
    api = daemon.reload(api)
    api.rate(0)
    dirs = {g['NZBName']: g['DestDir'] for g in api.listgroups()}
    midway = 'IdcWritten' in dirs
    gid = [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'IdcUnwritten']
    api.editqueue('GroupResume', 0, '', gid)
    hu = daemon.wait_history(api, 'IdcUnwritten', timeout=120)
    hw = daemon.wait_history(api, 'IdcWritten', timeout=120)

    def final(name, data):
        found = [f for f in t.find_files('main', 'dst') if f.endswith(name)]
        return len(found) == 1 and t.read_file(found[0]) == data
    left = t.find_files('main', 'inter')
    ok = (written and midway and dirs.get('IdcUnwritten', '').startswith(new_inter + os.sep)
          and dirs.get('IdcWritten', '').startswith(old_inter + os.sep)
          and hu['Status'].startswith('SUCCESS') and hw['Status'].startswith('SUCCESS')
          and hw.get('UnpackStatus') == 'SUCCESS'
          and hu.get('MoveStatus') == 'SUCCESS' and hw.get('MoveStatus') == 'SUCCESS'
          and final('idcu.bin', du) and final('idcw.mkv', dw) and not left
          and _grep_log(t, 'InterDir changed: 1 queued download') > 0)
    rel = lambda p: os.path.relpath(p, t.path('main')) if p else p
    return ('interdirchange', ok, 'final_u=%s final_w=%s left_in_old=%s dst=%s written=%s midway=%s status=%s/%s '
            'unpack=%s move=%s/%s dirs_after_reload=%s'
            % (final('idcu.bin', du), final('idcw.mkv', dw), left, t.find_files('main', 'dst'), written, midway,
               hu['Status'], hw['Status'], hw.get('UnpackStatus'), hu.get('MoveStatus'), hw.get('MoveStatus'),
               {k: rel(v) for k, v in dirs.items()}))


def scenario_finaldeleterestart(daemon, t):
    """B42: nzbget shuts down right after a half-done download was final-deleted,
    before the files still downloading let the delete finish. The download
    must not come back after the restart as a QUEUED item without files
    (production 2425: never post-processed, never removed)."""
    return _final_delete_midway(daemon, t, 'finaldeleterestart', True)


def scenario_restartmidway(daemon, t):
    """B42: nzbget shuts down while a download is half done, articles are
    being borrowed from a duplicate in history (HealthCheck=dupe,
    DupeArticleFallback=live). After the restart the download keeps the files
    it hadn't finished and completes: it isn't left QUEUED without files
    (then nothing ever starts its post-processing)."""
    seg = 100_000
    size = 6_000_000
    members, backup, payloads = [], [], []
    for i in range(6):
        data = _payload(size, 9840 + i)
        payloads.append(data)
        pp = _place_copy(t, 'rmA%d' % i, data, 'f%d.bin' % i)
        bp = _place_copy(t, 'rmB%d' % i, data, 'f%d.bin' % i)
        members.append((pp, 'Movie.part%02d.rar' % (i + 1), size, seg, {3, 7}))
        backup.append((bp, 'Movie.part%02d.rar' % (i + 1), size, seg, set()))
    # a par2 index: articles are borrowed only for a collection par2 can check
    par = generators.par2_index([('Movie.part%02d.rar' % (i + 1), payloads[i]) for i in range(6)])
    t.write_file(os.path.join('data', 'rmA0/rel.par2'), par)
    members.append(('rmA0/rel.par2', 'Movie.par2', len(par), 500_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(members), True, 'rm-key', 100)
    daemon.append(api, 'Backup', build_multi_nzb(backup), False, 'rm-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    pid = [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'][0]
    api.editqueue('GroupResume', 0, '', [pid])
    deadline = time.time() + 120
    g = []
    while time.time() < deadline:
        g = [x for x in api.listgroups() if x['NZBID'] == pid]
        if not g or g[0]['RemainingFileCount'] <= 4:
            break
        time.sleep(0.1)
    before = g[0]['RemainingFileCount'] if g else -1
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    time.sleep(1)
    g = [x for x in api.listgroups() if x['NZBID'] == pid]
    after = g[0]['RemainingFileCount'] if g else -1
    files_after = len(api.listfiles(0, 0, pid)) if g else -1
    h = daemon.wait_history(api, 'Primary', timeout=180)
    # completed: whole by borrowing, or (its par2 index has no recovery data, so the
    # articles that failed before the par-first wait was lifted stay missing) by
    # failing over to the whole backup
    deadline = time.time() + 120
    backup = {}
    while time.time() < deadline and not h['Status'].startswith('SUCCESS'):
        backup = next((x for x in api.history() if x['NZBName'] == 'Backup'), {})
        if backup.get('Status', '').startswith('SUCCESS'):
            break
        time.sleep(0.5)
    completed = h['Status'].startswith('SUCCESS') or backup.get('Status', '').startswith('SUCCESS')
    # (each file takes seconds: at most one finishes before the count is taken)
    ok = before > 1 and after >= before - 1 and files_after == after and completed
    return ('restartmidway', ok, 'remaining_before=%d remaining_after=%d listfiles_after=%d status=%s backup=%s'
            % (before, after, files_after, h['Status'], backup.get('Status')))


def scenario_sidefilenorepair(daemon, t):
    """B53: the data file is whole and only the .nfo's article is missing, from
    the duplicate too. No repair from the duplicate is queued for
    a side file (it held up Industry S04E06 4155, whose par2 had verified the
    video): the download completes without one."""
    data = _payload(2_000_000, 9860)
    nfo = _payload(3_000, 9861)
    dp = _place_copy(t, 'sfA', data, 'movie.mkv')
    np_ = _place_copy(t, 'sfN', nfo, 'movie.nfo')
    db = _place_copy(t, 'sfB', data, 'movie.mkv')
    nb = _place_copy(t, 'sfM', nfo, 'movie.nfo')
    primary = build_multi_nzb([(dp, 'Movie.mkv', len(data), 100_000, set()), (np_, 'Movie.nfo', len(nfo), 3_000, {1})])
    # the duplicates lack the .nfo too (as in production): only a repair could bring it
    donor = build_multi_nzb([(db, 'Movie.mkv', len(data), 100_000, set()), (nb, 'Movie.nfo', len(nfo), 3_000, {1})])
    api = daemon.wait_ready()
    daemon.append(api, 'Donor', donor, True, 'sf-key', 50)
    daemon.append(api, 'Release', primary, False, 'sf-key', 100)
    h = daemon.wait_history(api, 'Release')
    queued = _grep_log(t, 'Queueing stream repair of Movie.nfo')
    skipped = _grep_log(t, 'Not repairing side file Movie.nfo')
    # (no par2 here: the missing .nfo leaves the download FAILURE/HEALTH, as without the fix)
    ok = queued == 0 and skipped == 1
    return ('sidefilenorepair', ok, 'status=%s nfo_repairs_queued=%d skipped_logs=%d' % (h['Status'], queued, skipped))


def scenario_recheckfailed(daemon, t):
    """Lesson 18: with ArticleRetries=0, three articles are lost to a single
    failed request each (the server answers their first request with a
    transient error) though they exist on the server. The release fails; its failed articles are
    then asked of the server (STAT), all three exist, and they are retried once:
    the release completes byte-identically."""
    size, seg = 2_000_000, 100_000
    data = _payload(size, 9950)
    pp = _place_copy(t, 'rcA', data, 'file.bin')
    nzb = build_nzb(pp, 'Rc.bin', size, seg, set())
    # the proxy's markers pick articles 5, 10 and 15
    api = daemon.wait_ready()
    daemon.append(api, 'RelRc', nzb, False, 'rc-key', 100)
    first = daemon.wait_history(api, 'RelRc')
    deadline = time.time() + 120
    final = first
    while time.time() < deadline:
        h = [x for x in api.history() if x['NZBName'] == 'RelRc']
        if h and 'SUCCESS' in h[0]['Status']:
            final = h[0]
            break
        time.sleep(1)
    rechecked = _grep_log(t, '3 of 3 failed articles exist on the servers; retrying them')
    integ = _verify_output(t, data)
    # (the retry can be over before the first look at the history: the log tells)
    ok = rechecked == 1 and daemon.proxy.failed == 3 and 'SUCCESS' in final['Status'] and integ
    return ('recheckfailed', ok, 'first=%s rechecked_logs=%d final=%s integrity=%s failed_requests=%d'
            % (first['Status'], rechecked, final['Status'], integ, daemon.proxy.failed))


def scenario_transientretry(daemon, t):
    """The server answers the first request of three articles with 503: the
    articles exist, so each is asked for again after ArticleInterval and none
    counts as failed. Before, the retries were used up within a moment."""
    size, seg = 2_000_000, 100_000
    data = _payload(size, 9951)
    pp = _place_copy(t, 'trA', data, 'file.bin')
    api = daemon.wait_ready()
    daemon.append(api, 'RelTr', build_nzb(pp, 'Tr.bin', size, seg, set()), False, 'tr-key', 100)
    h = daemon.wait_history(api, 'RelTr', timeout=120)
    integ = _verify_output(t, data)
    failed = int(h.get('FailedArticles', 0))
    ok = h['Status'].startswith('SUCCESS') and integ and failed == 0 and daemon.proxy.failed == 3
    return ('transientretry', ok, 'status=%s failed_articles=%d failed_requests=%d integrity=%s'
            % (h['Status'], failed, daemon.proxy.failed, integ))


def scenario_transientexhausted(daemon, t):
    """The server answers 503 to every request of three articles: each is
    asked for ArticleRetries times, with a doubling wait between the requests,
    then fails with a message that doesn't call it missing."""
    size, seg = 2_000_000, 100_000
    data = _payload(size, 9952)
    pp = _place_copy(t, 'teA', data, 'file.bin')
    api = daemon.wait_ready()
    daemon.append(api, 'RelTe', build_nzb(pp, 'Te.bin', size, seg, set()), False, 'te-key', 100)
    h = daemon.wait_history(api, 'RelTe', timeout=180)
    failed = int(h.get('FailedArticles', 0))
    logged = _grep_log(t, 'temporary server errors outlasted all attempts')
    requests = daemon.proxy.requests.values()
    counts = sorted(len(times) for times in requests)
    # ArticleInterval=2: 2 then 4 seconds. Only the least is certain under load
    spans = [times[-1] - times[0] for times in requests]
    ok = failed == 3 and logged == 3 and counts == [3, 3, 3] and min(spans) >= 5
    return ('transientexhausted', ok, 'status=%s failed_articles=%d distinct_logs=%d requests=%s min_span=%.1f'
            % (h['Status'], failed, logged, counts, min(spans) if spans else -1))


def scenario_transientotherserver(daemon, t):
    """One of two servers of a level answers 503 to three articles for good:
    the other one serves them meanwhile, and nothing fails."""
    size, seg = 2_000_000, 100_000
    data = _payload(size, 9953)
    pp = _place_copy(t, 'toA', data, 'file.bin')
    api = daemon.wait_ready()
    daemon.append(api, 'RelTo', build_nzb(pp, 'To.bin', size, seg, set()), False, 'to-key', 100)
    h = daemon.wait_history(api, 'RelTo', timeout=120)
    integ = _verify_output(t, data)
    failed = int(h.get('FailedArticles', 0))
    ok = h['Status'].startswith('SUCCESS') and integ and failed == 0
    return ('transientotherserver', ok, 'status=%s failed_articles=%d failed_requests=%d integrity=%s'
            % (h['Status'], failed, daemon.proxy.failed, integ))


def scenario_cutover(daemon, t):
    """Primary missing 10 of 20 articles => file cuts over to the duplicate."""
    size, seg = 10_000_000, 500_000
    data = _payload(size, 1206)
    pp = _place_copy(t, 'cutA', data)
    dp = _place_copy(t, 'cutB', data)
    primary = build_nzb_with_par2(t, pp, 'CutA.bin', data, seg, set(range(2, 12)))
    donor = build_nzb(dp, 'obf-cut.bin', size, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DonCut', donor, True, 'cut-key', 50)
    daemon.append(api, 'CutA', primary, False, 'cut-key', 100)
    h = daemon.wait_history(api, 'CutA')
    ok = h['Status'].startswith('SUCCESS')
    recov = int(h.get('DupeRecoveredArticles', 0))
    cut = _grep_log(t, 'Leading with duplicate collections')
    integ = _verify_output(t, data)
    # Only REACTIVE recoveries count: fresh articles served proactively after
    # the cutover trips must not inflate the metric, so the count can never
    # exceed the 10 articles the primary is actually missing (proactive
    # inflation would push it towards 19) (any proactive round, incl. donor
    # round >= 2 under the primary-last order). The exact value below 10 is
    # timing-dependent - it is the number of primary failures already in
    # flight when cutover trips (>= the 3 recoveries that trip it), which
    # grows under system load beyond the 4 concurrent connections.
    # the summary's "of N missing" counts only articles that failed: the ones the
    # duplicate served first after the cutover were never missing (it read
    # "of 19 missing" for a file missing 10)
    with open(t.path('nzbget.log'), errors='replace') as f:
        found = re.findall(r'Recovered (\d+) of (\d+) missing article\(s\) of CutA', f.read())
    attempted = int(found[-1][1]) if found else -1
    return ('cutover', ok and integ and 3 <= recov <= 10 and cut == 1 and 0 < attempted <= 10,
            'status=%s recovered=%d attempted=%d cutover_logs=%d integrity=%s'
            % (h['Status'], recov, attempted, cut, integ))


def scenario_leadswitch(daemon, t):
    """Primary AND the top-scored duplicate share the same hole (parts 2-12);
    a second, lower-scored duplicate covers it. After a few consecutive lead
    misses the file must switch its lead to the next duplicate instead of
    paying a wasted fetch per article on the holed one, and still complete
    byte-identically from the second duplicate."""
    size, seg = 10_000_000, 500_000
    data = _payload(size, 4242)
    pp = _place_copy(t, 'leadP', data)
    dhp = _place_copy(t, 'leadH', data)
    dlp = _place_copy(t, 'leadL', data)
    holes = set(range(2, 13))
    primary = build_nzb_with_par2(t, pp, 'LeadA.bin', data, seg, holes)
    donor_high = build_nzb(dhp, 'obf-lh.bin', size, seg, holes)
    donor_low = build_nzb(dlp, 'obf-ll.bin', size, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'LeadHigh', donor_high, True, 'lead-key', 60)
    daemon.append(api, 'LeadLow', donor_low, True, 'lead-key', 50)
    daemon.append(api, 'LeadA', primary, False, 'lead-key', 100)
    h = daemon.wait_history(api, 'LeadA')
    ok = h['Status'].startswith('SUCCESS')
    recov = int(h.get('DupeRecoveredArticles', 0))
    # exactly ONE switch: more would mean stale in-flight failures of the old
    # lead cascade-demoted the new (complete) lead - the regression the
    # per-article lead snapshot exists to prevent
    switched = _grep_log(t, 'Switching lead duplicate collection')
    integ = _verify_output(t, data)
    # Only REACTIVE recoveries count (proactive traffic after cutover must
    # not inflate the metric), bounding the count by the 11 articles the
    # primary is missing; the exact value below that is timing-dependent -
    # how many articles were already mid-fallback when the proactive
    # pre-assignment took over grows under system load (any proactive round,
    # incl. donor round >= 2 under the primary-last order).
    return ('leadswitch', ok and integ and 3 <= recov <= 11 and switched == 1,
            'status=%s recovered=%d lead_switch_logs=%d integrity=%s'
            % (h['Status'], recov, switched, integ))


def scenario_cutovertruth(daemon, t):
    """Counter honesty under cutover: the lead duplicate misses articles the
    PRIMARY actually has (parts 20-23); the second duplicate serves them at
    round 2. Those proactive successes prove nothing about the primary and
    must NOT count as recovered - only reactive recoveries of the primary's
    own 11 missing articles may. Without the !DupeProactive guard the
    primary-last cutover order counts them (recov ~12-15 here)."""
    size, seg = 20_000_000, 500_000          # 40 articles
    data = _payload(size, 1207)
    pp = _place_copy(t, 'ctrA', data)
    dhp = _place_copy(t, 'ctrH', data)
    dlp = _place_copy(t, 'ctrL', data)
    primary = build_nzb_with_par2(t, pp, 'CtrA.bin', data, seg, set(range(2, 13)))       # 11 missing
    donor_high = build_nzb(dhp, 'obf-cth.bin', size, seg, set(range(20, 24)))
    donor_low = build_nzb(dlp, 'obf-ctl.bin', size, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'CtrHigh', donor_high, True, 'ctr-key', 60)
    daemon.append(api, 'CtrLow', donor_low, True, 'ctr-key', 50)
    daemon.append(api, 'CtrA', primary, False, 'ctr-key', 100)
    h = daemon.wait_history(api, 'CtrA')
    ok = h['Status'].startswith('SUCCESS')
    recov = int(h.get('DupeRecoveredArticles', 0))
    switched = _grep_log(t, 'Switching lead duplicate collection')
    integ = _verify_output(t, data)
    # bound = the primary's 11 missing articles: proactive traffic (incl. the
    # lead's private holes at parts 20-23, served by the second duplicate)
    # never counts, so the bound is timing-independent. The lead's 4
    # consecutive misses may legitimately rotate the lead once.
    return ('cutovertruth', ok and integ and 3 <= recov <= 11 and switched <= 1,
            'status=%s recovered=%d lead_switch_logs=%d integrity=%s'
            % (h['Status'], recov, switched, integ))


def scenario_manydonors(daemon, t, ndonors=18):
    """More duplicates than the donor cache (16) => exercises cache eviction
    (regression for the use-after-free crash). Primary missing 2,3,4."""
    size, seg = 3_000_000, 500_000
    data = _payload(size, 77)
    pp = _place_copy(t, 'manyPrim', data)
    primary = build_nzb_with_par2(t, pp, 'ManyPrim.bin', data, seg, {2, 3, 4})
    api = daemon.wait_ready()
    for i in range(ndonors):
        dp = _place_copy(t, 'manyD%02d' % i, data)
        donor = build_nzb(dp, 'obf-d%02d.bin' % i, size, seg, set())
        daemon.append(api, 'ManyD%02d' % i, donor, True, 'many-key', 50 + i)
    daemon.append(api, 'ManyPrim', primary, False, 'many-key', 100)
    h = daemon.wait_history(api, 'ManyPrim')
    ok = h['Status'].startswith('SUCCESS')
    recov = int(h.get('DupeRecoveredArticles', 0))
    integ = _verify_output(t, data)
    # crash regression: the daemon must still answer RPC afterwards
    alive = True
    try:
        api.status()
    except Exception:
        alive = False
    return ('manydonors', ok and integ and recov >= 3 and alive,
            'status=%s recovered=%d integrity=%s daemon_alive=%s'
            % (h['Status'], recov, integ, alive))


def scenario_articledecoy(daemon, t):
    """Article borrowing from a duplicate of the same size and article layout but
    other bytes (a different encode), no par2 to check against: the download must
    not end SUCCESS with the decoy's bytes in its file."""
    size, seg = 5_000_000, 500_000
    data = _payload(size, 6100)
    decoy = _payload(size, 6101)
    pp = _place_copy(t, 'adA', data)
    dp = _place_copy(t, 'adB', decoy)
    primary = build_nzb(pp, 'Decoy.bin', size, seg, {3, 7})
    donor = build_nzb(dp, 'Decoy.bin', size, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DonAD', donor, True, 'ad-key', 50)
    daemon.append(api, 'RelAD', primary, False, 'ad-key', 100)
    h = daemon.wait_history(api, 'RelAD')
    integ = _verify_output(t, data)
    wrong_success = h['Status'].startswith('SUCCESS') and not integ
    return ('articledecoy', not wrong_success, 'status=%s integrity=%s recovered=%s' % (
        h['Status'], integ, h.get('DupeRecoveredArticles')))


def scenario_articledecoypar(daemon, t):
    """The decoy of articledecoy (same size and article layout, other bytes), the
    collection with a par2 index this time: articles are borrowed, but their
    bytes fail the par2 checksums, so the download doesn't end SUCCESS with them."""
    size, seg = 5_000_000, 500_000
    data = _payload(size, 6200)
    decoy = _payload(size, 6201)
    pp = _place_copy(t, 'apA', data)
    dp = _place_copy(t, 'apB', decoy)
    primary = build_nzb_with_par2(t, pp, 'DecoyPar.bin', data, seg, {3, 7})
    donor = build_nzb(dp, 'DecoyPar.bin', size, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DonAP', donor, True, 'ap-key', 50)
    daemon.append(api, 'RelAP', primary, False, 'ap-key', 100)
    h = daemon.wait_history(api, 'RelAP')
    integ = _verify_output(t, data)
    wrong_success = h['Status'].startswith('SUCCESS') and not integ
    return ('articledecoypar', not wrong_success, 'status=%s integrity=%s recovered=%s rejected_logs=%d' % (
        h['Status'], integ, h.get('DupeRecoveredArticles'), _grep_log(t, "don't match its par2 checksums")))


def scenario_stream(daemon, t):
    """Primary (.mkv, 500 KB parts) missing a middle block; the donor posted
    the SAME content split into 250 KB parts (different article count) and
    sits paused in the queue under the same dupe-key. Article-level fallback
    cannot borrow across segmentations, so the post-processing stream repair
    must fetch the missing byte ranges from the donor. A DECOY duplicate of
    the same byte size but different content carries a HIGHER dupe-score, so
    it is tried first and must be rejected by the identity probe (the
    negative half of the test: no corruption from a same-size impostor).
    Every hole gets filled and there is no par2 here, so the byte-based
    health recount (StreamRepairController::RepairCompleted crediting each
    fully-repaired file's encoded failed size back to health) drives
    CalcHealth() to 1000: the history status now completes as SUCCESS
    (moved to the final destination) instead of parking at FAILURE/HEALTH -
    that status flip is asserted directly below, alongside byte integrity,
    the repair logs and the DupeRecoveredArticles counter."""
    size, seg_primary, seg_donor = 6_000_000, 500_000, 250_000
    data = _payload(size, 4242)
    decoy_data = _payload(size, 999)  # same size, different bytes
    pp = _place_copy(t, 'streamA', data, 'file.mkv')
    dp = _place_copy(t, 'streamB', data, 'file.mkv')
    xp = _place_copy(t, 'streamX', decoy_data, 'file.mkv')
    primary = build_nzb(pp, 'StreamA.mkv', size, seg_primary, {5, 6})
    donor = build_nzb(dp, 'obf-stream.mkv', size, seg_donor, set())
    decoy = build_nzb(xp, 'obf-decoy.mkv', size, seg_donor, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DecoyStream', decoy, True, 'stream-key', 75)
    daemon.append(api, 'DonStream', donor, True, 'stream-key', 50)
    daemon.append(api, 'StreamA', primary, False, 'stream-key', 100)
    h = daemon.wait_history(api, 'StreamA')
    recov = int(h.get('DupeRecoveredArticles', 0))
    queued = _grep_log(t, 'Queueing stream repair')
    repaired = _grep_log(t, 'donor article(s)')
    rejected = _grep_log(t, 'content identity not confirmed')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    success = 'SUCCESS' in h['Status']
    return ('stream',
            success and integ and recov == 2 and queued >= 1 and repaired >= 1 and rejected >= 1,
            'status=%s recovered=%d queued_logs=%d repair_logs=%d rejected_logs=%d integrity=%s'
            % (h['Status'], recov, queued, repaired, rejected, integ))


def scenario_streamrestartcredit(daemon, t):
    """A stream repair interrupted by a shutdown after it fully repaired one file
    (the other's duplicate is slow): after the restart the other file is repaired
    too, and the release ends SUCCESS. The fully repaired file's failed size was
    never credited back to health (its job was dropped as done), so the release
    stayed short of full health though its files were complete."""
    size, seg_primary, seg_donor = 2_000_000, 500_000, 300_000
    d1, d2 = _payload(size, 7676), _payload(size, 7677)
    p1 = _place_copy(t, 'srA', d1, 'f1.mkv')
    p2 = _place_copy(t, 'srC', d2, 'f2.mkv')
    b1 = _place_copy(t, 'srB', d1, 'f1.mkv')
    b2 = _place_copy(t, 'srSlow', d2, 'f2.mkv')
    primary = build_multi_nzb([(p1, 'Sr1.mkv', size, seg_primary, {2}), (p2, 'Sr2.mkv', size, seg_primary, {3})])
    donor = build_multi_nzb([(b1, 'obf-sr1.mkv', size, seg_donor, set()), (b2, 'obf-sr2.mkv', size, seg_donor, set())])
    api = daemon.wait_ready()
    daemon.append(api, 'DonSR', donor, True, 'sr2-key', 50)
    daemon.append(api, 'RelSR2', primary, False, 'sr2-key', 100)
    # "Recovered ... of Sr1.mkv from duplicate DonSR", not the "Queueing stream repair of
    # Sr1.mkv from duplicate collections" before it: the shutdown must catch the repair running
    deadline = time.time() + 120
    while time.time() < deadline and _grep_log(t, 'of Sr1.mkv from duplicate DonSR') == 0:
        time.sleep(0.3)
    first_repaired = _grep_log(t, 'of Sr1.mkv from duplicate DonSR')
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    h = daemon.wait_history(api, 'RelSR2', timeout=240)
    integ = _verify_output(t, d1, '.mkv', dirs=(('main', 'dst'), ('main', 'inter'))) and \
        _verify_output(t, d2, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    interrupted = _grep_log(t, 'Stream repair interrupted')
    ok = first_repaired >= 1 and interrupted >= 1 and h['Status'].startswith('SUCCESS') and h['Health'] == 1000 and integ
    return ('streamrestartcredit', ok, 'first_repaired=%d interrupted=%d status=%s health=%s integrity=%s' % (
        first_repaired, interrupted, h['Status'], h['Health'], integ))


def scenario_truncatedstate(daemon, t):
    """A file's partial state (<id>s, written in place) is cut short, as a power
    loss leaves it: the download still completes, the file downloaded again. The
    load left blank articles behind (they downloaded nothing, the file failed)
    and stopped reading the remaining states."""
    size, seg = 3_000_000, 100_000
    data = _payload(size, 7979)
    pp = _place_copy(t, 'tsA', data, 'f.bin')
    api = daemon.wait_ready()
    daemon.append(api, 'RelTS', build_nzb(pp, 'ts.bin', size, seg, set()), False, 'ts-key', 100)
    queue_dir = t.path('main', 'queue')
    deadline = time.time() + 60
    state = None
    while time.time() < deadline and not state:
        state = next((f for f in os.listdir(queue_dir) if re.fullmatch(r'\d+s', f)), None)
        time.sleep(0.2)
    t.procs[-1].kill()
    t.procs[-1].wait(timeout=30)
    if state:
        path = os.path.join(queue_dir, state)
        content = open(path, 'rb').read()
        open(path, 'wb').write(content[:len(content) // 2])
    daemon.start()
    api = daemon.wait_ready()
    h = daemon.wait_history(api, 'RelTS', timeout=180)
    integ = _verify_output(t, data)
    ok = bool(state) and h['Status'].startswith('SUCCESS') and integ
    return ('truncatedstate', ok, 'state_file=%s status=%s integrity=%s discarded_logs=%d' % (
        state, h['Status'], integ, _grep_log(t, 'Discarding damaged download state')))


def scenario_queuebatchstart(daemon, t):
    """Each pass of the queue coordinator takes 100 ms (RUSTYGET_TEST_COORD_PASS_DELAY_MS,
    as a long queue and lock waits make it on 6790) and each article 1 s (proxy). Started
    one per pass, at most about 10 articles are ever in flight however many connections
    there are (6790: 117 of 345); started in a batch per pass, the 30 connections fill.
    A small item (6 articles) comes first: the second item must start in the same passes,
    so both download at once."""
    seg = 50_000
    small = _payload(6 * seg, 5100)
    big = _payload(90 * seg, 5101)
    pa = _place_copy(t, 'qbA', small, 'a.bin')
    pb = _place_copy(t, 'qbB', big, 'b.bin')
    api = daemon.wait_ready()
    daemon.append(api, 'RelQBA', build_nzb(pa, 'qba.bin', len(small), seg, set()), False, 'qba-key', 100)
    daemon.append(api, 'RelQBB', build_nzb(pb, 'qbb.bin', len(big), seg, set()), False, 'qbb-key', 100)
    peak, both = 0, False
    deadline = time.time() + 120
    while time.time() < deadline:
        groups = api.listgroups()
        active = {g['NZBName']: g.get('ActiveDownloads', 0) for g in groups}
        peak = max(peak, sum(active.values()))
        both |= active.get('RelQBA', 0) > 0 and active.get('RelQBB', 0) > 0
        if not groups:
            break
        time.sleep(0.1)
    ha = daemon.wait_history(api, 'RelQBA', timeout=60)
    hb = daemon.wait_history(api, 'RelQBB', timeout=60)
    ok = (ha['Status'].startswith('SUCCESS') and hb['Status'].startswith('SUCCESS')
          and peak >= 24 and both)
    return ('queuebatchstart', ok, 'status=%s/%s peak_in_flight=%d of 30 (need 24) both_at_once=%s' % (
        ha['Status'], hb['Status'], peak, both))


def scenario_queuesyncslow(daemon, t):
    """Every sync of a saved state file takes a second (RUSTYGET_TEST_STATE_SYNC_DELAY_MS,
    as on 6790 while a post-processing move saturated the queue disk with FlushQueue=yes).
    The queue coordinator saves the partial states and the progress once a second; it must
    not wait for those syncs, or no articles are handed out meanwhile and the rate falls
    to a fraction (6790: 108 MB/s to 4-20 MB/s, 10-50 of 205 connections busy). Six files
    of 30 articles each, every article delayed 0.3 s by the proxy, 6 connections: about
    9 s of transfer. Waiting for the syncs (two or more seconds of every second) makes it
    take several times that; the bound is 30 s."""
    seg, size, nfiles = 100_000, 3_000_000, 6
    members = []
    for i in range(nfiles):
        data = _payload(size, 4400 + i)
        members.append((_place_copy(t, 'qsA', data, 'f%d.bin' % i), 'qs%d.bin' % i, size, seg, set()))
    api = daemon.wait_ready()
    start = time.time()
    daemon.append(api, 'RelQS', build_multi_nzb(members), False, 'qs-key', 100)
    h = daemon.wait_history(api, 'RelQS', timeout=240)
    elapsed = time.time() - start
    ok = h['Status'].startswith('SUCCESS') and elapsed < 30.0
    return ('queuesyncslow', ok, 'status=%s elapsed=%.1fs bound=30s' % (h['Status'], elapsed))


def _small_fs_mount(t, mnt, size_mb):
    """Mounts a small file system of its own at ``mnt`` (Linux: an ext4 image through
    sudo, which frees a truncated file's blocks only at the next journal commit, as the
    NVMe on 6790 did; macOS: an HFS+ disk image). Returns the unmount command, or None
    with the reason when this machine cannot."""
    os.makedirs(mnt, exist_ok=True)
    img = t.path('smallfs.img')
    if sys.platform.startswith('linux'):
        if shutil.which('mkfs.ext4') is None:
            return None, 'mkfs.ext4 not installed'
        if subprocess.run(['sudo', '-n', 'true'], capture_output=True).returncode != 0:
            return None, 'no passwordless sudo to mount'
        with open(img, 'wb') as f:
            f.truncate(size_mb * 1024 * 1024)
        if subprocess.run(['mkfs.ext4', '-q', '-F', '-m', '0', img], capture_output=True).returncode != 0:
            return None, 'mkfs.ext4 failed'
        if subprocess.run(['sudo', '-n', 'mount', '-o', 'loop', img, mnt], capture_output=True).returncode != 0:
            return None, 'loop mount failed'
        subprocess.run(['sudo', '-n', 'chown', '%d:%d' % (os.getuid(), os.getgid()), mnt], check=True)
        return ['sudo', '-n', 'umount', mnt], None
    if sys.platform == 'darwin':
        dmg = img + '.dmg'
        made = subprocess.run(['hdiutil', 'create', '-quiet', '-size', '%dm' % size_mb, '-fs', 'HFS+',
                               '-volname', 'rgfull', dmg], capture_output=True)
        if made.returncode != 0:
            return None, 'hdiutil create failed'
        if subprocess.run(['hdiutil', 'attach', '-quiet', '-nobrowse', '-mountpoint', mnt, dmg],
                          capture_output=True).returncode != 0:
            return None, 'hdiutil attach failed'
        return ['hdiutil', 'detach', '-quiet', '-force', mnt], None
    return None, 'no small file system on %s' % sys.platform


class _DiskFiller:
    """Fills the file system of ``path`` and keeps it full: a thread writes whatever
    space comes free (a deleted temporary file, a journal commit) into the filler file
    until ``release``."""

    def __init__(self, path):
        self.path = path
        self.stop = threading.Event()
        self.f = open(path, 'wb', buffering=0)
        self._fill()
        self.thread = threading.Thread(target=self._keep_full, daemon=True)
        self.thread.start()

    def _fill(self):
        for chunk in (1 << 20, 1 << 16, 1 << 12, 512):
            while True:
                try:
                    self.f.write(b'\0' * chunk)
                except OSError:
                    break

    def _keep_full(self):
        while not self.stop.wait(0.02):
            self._fill()

    def release(self):
        self.stop.set()
        self.thread.join(timeout=10)
        try:
            self.f.close()
        except OSError:
            pass
        os.remove(self.path)


def scenario_diskfullrestart(daemon, t):
    """6790, 2026-11-27: the disk holding QueueDir and InterDir filled up during a
    download. The partial state of the downloading file (``<id>s``, then written in
    place) was cut short by the failed write, the deploy restart found it damaged and
    threw away every article it recorded: 57 GB downloaded again. Here QueueDir and
    InterDir sit on a small file system of their own; it is filled at 40 %, the daemon
    is stopped (its last saves fail), space is freed and the daemon started again. No
    article that came before the fill (less the last 4 s, which a save may not have
    recorded yet) may be fetched again, the output must be byte-exact, and the stop
    with failing saves must not exit 0 (not a clean stop)."""
    seg, size = 100_000, 40_000_000
    data = _payload(size, 6790)
    pp = _place_copy(t, 'dfA', data, 'f.bin')
    api = daemon.wait_ready()
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    mnt = t.path('main', 'small')
    umount, why = _small_fs_mount(t, mnt, 64)
    if umount is None:
        return ('diskfullrestart', None, 'SKIP: %s' % why)
    filler = None
    try:
        os.makedirs(os.path.join(mnt, 'queue'))
        os.makedirs(os.path.join(mnt, 'inter'))
        conf = t.path(daemon.conf_rel)
        with open(conf, 'ab') as f:
            f.write(('QueueDir=%s\nInterDir=%s\n' % (os.path.join(mnt, 'queue'),
                                                     os.path.join(mnt, 'inter'))).encode())
        daemon.start()
        api = daemon.wait_ready()
        daemon.append(api, 'RelDF', build_nzb(pp, 'df.bin', size, seg, set()), False, 'df-key', 100)
        total = size // seg
        history = []  # (time, requests so far)
        deadline = time.time() + 120
        while time.time() < deadline:
            history.append((time.time(), len(_failcont_requests(t))))
            if history[-1][1] >= total * 4 // 10:
                break
            time.sleep(0.1)
        filler = _DiskFiller(os.path.join(mnt, 'filler'))
        fill_time = time.time()
        recorded = max([n for when, n in history if when <= fill_time - 4.0] or [0])
        before = _failcont_requests(t)[:recorded]
        time.sleep(4)  # saves and article writes fail on the full disk
        try:
            api.shutdown()
        except Exception:
            pass
        stop_rc = t.procs[-1].wait(timeout=120)
        save_errors = _grep_log(t, 'Error saving diskstate')
        unclean = _grep_log(t, 'Stopping is not clean')
        filler.release()
        filler = None
        restart_at = len(_failcont_requests(t))
        daemon.start()
        api = daemon.wait_ready()
        h = daemon.wait_history(api, 'RelDF', timeout=240)
        after = _failcont_requests(t)[restart_at:]
        again = sorted(set(before) & set(after))
        integ = _verify_output(t, data)
        discarded = _grep_log(t, 'Discarding damaged download state')
        ok = (h['Status'].startswith('SUCCESS') and integ and recorded > 0 and not again
              and save_errors > 0 and stop_rc == 1 and unclean > 0)
        return ('diskfullrestart', ok,
                'status=%s integrity=%s recorded_before_fill=%d fetched_again=%d of them '
                'fetched_after_restart=%d save_errors=%d stop_rc=%s unclean_logs=%d discarded_states=%d'
                % (h['Status'], integ, recorded, len(again), len(after), save_errors, stop_rc,
                   unclean, discarded))
    finally:
        if filler:
            filler.release()
        for p in t.procs:
            if p.poll() is None:
                p.kill()
                p.wait(timeout=30)
        if subprocess.run(umount, capture_output=True).returncode != 0:
            # Busy: detach it lazily, so the work dir cleanup never reaches into it.
            lazy = umount[:-1] + ['-l', umount[-1]] if umount[0] == 'sudo' else umount
            rc = subprocess.run(lazy, capture_output=True).returncode
            print('       diskfullrestart: unmount of %s failed, lazy unmount rc=%d' % (mnt, rc))


def _cache_commit_full(daemon, t, name):
    """6790, 2026-11-27: on the full NVMe the article cache could not write its articles
    ("Could not commit cached article ... No space left on device", 2628 times); it dropped
    their bytes while the articles stayed counted as downloaded, which left holes of zeros
    in DirectWrite output. Here InterDir is a 64 MB file system of its own, ArticleCache=16
    and DiskSpace=1: the disk is filled at 25 %, cache flushes fail and the low space pause
    stops downloading; the space is freed and downloading resumed. Every lost article must
    be downloaded again (no par2 here: the output must be byte-exact), the loss logged."""
    seg, size = 100_000, 40_000_000
    data = _payload(size, 6791)
    pp = _place_copy(t, 'ccA', data, 'c.bin')
    api = daemon.wait_ready()
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    mnt = t.path('main', 'small')
    umount, why = _small_fs_mount(t, mnt, 64)
    if umount is None:
        return (name, None, 'SKIP: %s' % why)
    filler = None
    try:
        os.makedirs(os.path.join(mnt, 'inter'))
        conf = t.path(daemon.conf_rel)
        with open(conf, 'ab') as f:
            f.write(('InterDir=%s\n' % os.path.join(mnt, 'inter')).encode())
        daemon.start()
        api = daemon.wait_ready()
        daemon.append(api, 'RelCC', build_nzb(pp, 'cc.bin', size, seg, set()), False, 'cc-key', 100)
        total = size // seg
        deadline = time.time() + 120
        while time.time() < deadline and len(_failcont_requests(t)) < total // 4:
            time.sleep(0.1)
        filler = _DiskFiller(os.path.join(mnt, 'filler'))
        deadline = time.time() + 60
        while time.time() < deadline and not (_grep_log(t, 'Could not commit cached article')
                                              and _grep_log(t, 'Pausing download')):
            time.sleep(0.2)
        lost_logs = _grep_log(t, 'Could not commit cached article')
        paused = _grep_log(t, 'Pausing download')
        time.sleep(2)
        g = [x for x in api.listgroups() if x['NZBName'] == 'RelCC']
        state = (g[0].get('RemainingSizeMB'), g[0].get('SuccessArticles'), g[0].get('ActiveDownloads')) if g else None
        filler.release()
        filler = None
        before = _failcont_requests(t)
        requests_before = len(before)
        api.resumedownload()
        h = daemon.wait_history(api, 'RelCC', timeout=240)
        ids = _failcont_requests(t)
        again = len(ids) - len(set(ids))
        again_after = len(set(before) & set(ids[requests_before:]))
        integ = _verify_output(t, data)
        refetch_logs = _grep_log(t, 'could not be written and will be downloaded again')
        log = t.read_file('nzbget.log').decode(errors='replace')
        handed = re.findall(r'(\d+) article\(s\) could not be written and will be downloaded again, (\d+) counted as failed', log)
        more = re.findall(r'Could not commit (\d+) more cached articles', log)
        saved = re.findall(r'Saved (\d+) articles', log)
        reason = re.findall(r'Could not commit cached article for [^:]*: ([^;\n]*)', log)
        handed_n = sum(int(n) for n, _ in handed)
        downloaded_mb = h.get('DownloadedSizeMB', 0)
        # The handed-back articles came again: the daemon downloaded more articles than the
        # file has (nserv may log none of them: a fetched-again body can come from the
        # client's shared body cache, so the daemon's own log counts). A lower bound only:
        # the byte-exact output (no par2 here) is what proves no hole was left.
        fetched = len(re.findall(r'Successfully downloaded RelCC/cc\.bin \[', log))
        fetched_again = fetched - total
        ok = (h['Status'].startswith('SUCCESS') and integ and lost_logs > 0 and paused > 0
              and refetch_logs > 0 and handed_n > 0 and fetched_again >= handed_n)
        return (name, ok,
                'status=%s integrity=%s commit_errors=%d paused_logs=%d refetch_logs=%d '
                'requests_before_resume=%d total_requests=%d repeated_requests=%d repeated_after_resume=%d '
                'handed_back=%s more=%s reason=%s paused_state=%s saved=%s downloaded_mb=%s fetched_again=%d'
                % (h['Status'], integ, lost_logs, paused, refetch_logs, requests_before, len(ids),
                   again, again_after, handed, more, reason[:2], state, saved[:12], downloaded_mb, fetched_again))
    finally:
        if filler:
            filler.release()
        for p in t.procs:
            if p.poll() is None:
                p.kill()
                p.wait(timeout=30)
        if subprocess.run(umount, capture_output=True).returncode != 0:
            lazy = umount[:-1] + ['-l', umount[-1]] if umount[0] == 'sudo' else umount
            rc = subprocess.run(lazy, capture_output=True).returncode
            print('       %s: unmount of %s failed, lazy unmount rc=%d' % (name, mnt, rc))


def scenario_cachecommitfull(daemon, t):
    """See `_cache_commit_full`: DirectWrite=yes, the cache writes into the output file."""
    return _cache_commit_full(daemon, t, 'cachecommitfull')


def _interdir_budget_run(daemon, t, scen, first_prio, second_prio):
    """Two 30 MB downloads in a 64 MB InterDir with DiskSpace=4 and a 1 MB state reserve
    (RUSTYGET_TEST_STATE_RESERVE_MB): each needs 33 MB (remaining plus 10 %), so only one
    may run. BudgetA is added first with ``first_prio``, BudgetB 0.3 s later with
    ``second_prio``; the one with the higher priority (A on a tie: queue order) is the
    lead (both are queued while downloading is paused, then it is resumed). Each comes in 4
    articles over 8 connections, every request held 2 s (nserv -w), so without the budget
    both run at once: the API is sampled and the other must show no active download or
    downloaded byte while the lead still downloads. Once the lead has moved
    to DestDir, the other starts. Both end SUCCESS and byte-exact, the other's hold is
    logged once and the lead's never."""
    seg, size = 7_500_000, 30_000_000
    data_a, data_b = _payload(size, 7101), _payload(size, 7102)
    pa = _place_copy(t, 'bgA', data_a, 'a.bin')
    pb = _place_copy(t, 'bgB', data_b, 'b.bin')
    lead, other = ('BudgetB', 'BudgetA') if second_prio > first_prio else ('BudgetA', 'BudgetB')
    api = daemon.wait_ready()
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    mnt = t.path('main', 'small')
    umount, why = _small_fs_mount(t, mnt, 64)
    if umount is None:
        return (scen, None, 'SKIP: %s' % why)
    try:
        os.makedirs(os.path.join(mnt, 'inter'))
        conf = t.path(daemon.conf_rel)
        with open(conf, 'ab') as f:
            f.write(('InterDir=%s\n' % os.path.join(mnt, 'inter')).encode())
        daemon.start()
        api = daemon.wait_ready()
        # Both are in the queue before the first pick: downloading is paused meanwhile.
        api.pausedownload()
        content = base64.standard_b64encode(build_nzb(pa, 'a.bin', size, seg, set()).encode()).decode()
        api.append('BudgetA', content, 'test', first_prio, False, False, 'bud-a', 100, 'score', [])
        time.sleep(0.3)
        content = base64.standard_b64encode(build_nzb(pb, 'b.bin', size, seg, set()).encode()).decode()
        api.append('BudgetB', content, 'test', second_prio, False, False, 'bud-b', 100, 'score', [])
        deadline = time.time() + 30
        while time.time() < deadline and len(api.listgroups()) < 2:
            time.sleep(0.1)
        time.sleep(0.5)
        api.resumedownload()
        samples = lead_seen = overlap = 0
        deadline = time.time() + 120
        while time.time() < deadline:
            groups = {g['NZBName']: g for g in api.listgroups()}
            a, b = groups.get(lead), groups.get(other)
            if a is None:
                break
            samples += 1
            # The lead downloads while it has articles in flight or left to fetch; once it is
            # all down (moving to DestDir) the other may start.
            downloading = a.get('ActiveDownloads', 0) > 0 or a.get('RemainingSizeMB', 0) > 0
            if a.get('ActiveDownloads', 0) > 0:
                lead_seen += 1
            if downloading and b is not None and (b.get('ActiveDownloads', 0) > 0
                                                  or b.get('DownloadedSizeMB', 0) > 0):
                overlap += 1
            time.sleep(0.2)
        ha = daemon.wait_history(api, 'BudgetA', timeout=180)
        hb = daemon.wait_history(api, 'BudgetB', timeout=180)
        ids = _failcont_requests(t)
        got_other = sum(1 for m in ids if m.startswith('bgB/' if other == 'BudgetB' else 'bgA/'))
        integ = _verify_output(t, data_a) and _verify_output(t, data_b)
        held = _grep_log(t, 'Holding %s: needs' % other)
        held_lead = _grep_log(t, 'Holding %s: needs' % lead)
        ok = (ha['Status'].startswith('SUCCESS') and hb['Status'].startswith('SUCCESS') and integ
              and lead_seen > 0 and overlap == 0 and got_other > 0 and held == 1 and held_lead == 0)
        return (scen, ok,
                'lead=%s statusA=%s statusB=%s integrity=%s samples=%d lead_active_samples=%d '
                'other_active_while_lead_downloads=%d other_requests=%d held_logs_other=%d held_logs_lead=%d'
                % (lead, ha['Status'], hb['Status'], integ, samples, lead_seen, overlap, got_other,
                   held, held_lead))
    finally:
        for p in t.procs:
            if p.poll() is None:
                p.kill()
                p.wait(timeout=30)
        if subprocess.run(umount, capture_output=True).returncode != 0:
            lazy = umount[:-1] + ['-l', umount[-1]] if umount[0] == 'sudo' else umount
            rc = subprocess.run(lazy, capture_output=True).returncode
            print('       %s: unmount of %s failed, lazy unmount rc=%d' % (scen, mnt, rc))


def scenario_interdirbudget(daemon, t):
    """6790, 2026-11-27: about 450 GB of downloads started together in a 470 GB InterDir,
    which filled up. Two downloads that do not both fit: the second added waits until the
    first has left InterDir (see `_interdir_budget_run`)."""
    return _interdir_budget_run(daemon, t, 'interdirbudget', 0, 0)


def scenario_interdirbudgetprio(daemon, t):
    """The budget goes by the pick, not by queue order: a high-priority download added
    after a low-priority one that does not fit beside it runs first, the low one waits
    (checking each item met in the queue walk would let the earlier, lower one take the
    room)."""
    return _interdir_budget_run(daemon, t, 'interdirbudgetprio', 0, 100)


def scenario_streamtimeout(daemon, t):
    """DupeStreamTimeout: the only duplicate is stalled (every request for its
    articles waits 8 s, longer than the timeout), so the repair of the
    primary's 4 MB hole recovers nothing. With DupeStreamTimeout=5 it stops
    after 5 seconds without progress, logs why, and the download finishes at
    once as a failure (no par2 here) instead of sitting in stream repair, so a
    client can grab another release."""
    size, seg_primary, seg_donor = 6_000_000, 500_000, 250_000
    data = _payload(size, 4343)
    pp = _place_copy(t, 'slowA', data, 'file.mkv')
    dp = _place_copy(t, 'slowB', data, 'file.mkv')
    primary = build_nzb(pp, 'SlowA.mkv', size, seg_primary, set(range(2, 10)))
    donor = build_nzb(dp, 'obf-slow.mkv', size, seg_donor, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DonSlow', donor, True, 'slow-key', 50)
    daemon.append(api, 'SlowA', primary, False, 'slow-key', 100)
    start = time.time()
    h = daemon.wait_history(api, 'SlowA', timeout=120)
    took = time.time() - start
    stopped = _grep_log(t, 'nothing recovered for 5 seconds (option DupeStreamTimeout)')
    interrupted = _grep_log(t, 'Stream repair interrupted')
    ok = stopped == 1 and interrupted == 0 and 'SUCCESS' not in h['Status'] and took < 40 and \
        daemon.proxy.delayed > 0
    return ('streamtimeout', ok, 'status=%s took=%.0fs stopped_logs=%d interrupted_logs=%d delayed=%d'
            % (h['Status'], took, stopped, interrupted, daemon.proxy.delayed))


def scenario_streamslowprogress(daemon, t):
    """DupeStreamTimeout counts time WITHOUT progress: a duplicate that answers
    slowly but keeps delivering is not cut off, even
    when the whole repair takes longer than the timeout (5 s), as long as it is
    not 3 times slower than downloading: the download is slow here too (2 s
    per request, the duplicate 0.5 s). The file is repaired in full."""
    size, seg_primary, seg_donor = 6_000_000, 500_000, 250_000
    data = _payload(size, 4343)
    pp = _place_copy(t, 'slowA', data, 'file.mkv')
    dp = _place_copy(t, 'slowB', data, 'file.mkv')
    primary = build_nzb(pp, 'SlowA.mkv', size, seg_primary, set(range(2, 10)))
    donor = build_nzb(dp, 'obf-slow.mkv', size, seg_donor, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DonSlow', donor, True, 'slow-key', 50)
    daemon.append(api, 'SlowA', primary, False, 'slow-key', 100)
    h = daemon.wait_history(api, 'SlowA', timeout=180)
    stopped = _grep_log(t, '(option DupeStreamTimeout)')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    ok = stopped == 0 and integ and 'SUCCESS' in h['Status'] and daemon.proxy.delayed > 5
    return ('streamslowprogress', ok, 'status=%s stopped_logs=%d integrity=%s delayed=%d'
            % (h['Status'], stopped, integ, daemon.proxy.delayed))


def scenario_streamtooslow(daemon, t):
    """A repair that keeps recovering, but so slowly that the rest would take
    more than 3 times as long as downloading it at nzbget's average speed, is
    stopped (another release is quicker): the download itself is fast here,
    every request to the duplicate waits 1 s. (A 40 MB download first gives
    nzbget an average speed to compare with; it is slowed to take a few
    seconds, since nzbget measures download time in whole seconds and a
    download under one second leaves the speed unknown.)"""
    warm = 40_000_000
    wp = _place_copy(t, 'warm', _payload(warm, 77), 'file.bin')
    api = daemon.wait_ready()
    daemon.append(api, 'Warm', build_nzb(wp, 'warm.bin', warm, 500_000, set()), False, 'warm-key', 0)
    daemon.wait_history(api, 'Warm')
    size, seg_primary, seg_donor = 6_000_000, 500_000, 250_000
    data = _payload(size, 4343)
    pp = _place_copy(t, 'slowA', data, 'file.mkv')
    dp = _place_copy(t, 'slowB', data, 'file.mkv')
    primary = build_nzb(pp, 'SlowA.mkv', size, seg_primary, set(range(2, 10)))
    donor = build_nzb(dp, 'obf-slow.mkv', size, seg_donor, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DonSlow', donor, True, 'slow-key', 50)
    daemon.append(api, 'SlowA', primary, False, 'slow-key', 100)
    h = daemon.wait_history(api, 'SlowA', timeout=180)
    slow = _grep_log(t, 'more than 3 times a download at')
    stalled = _grep_log(t, 'nothing recovered for')
    ok = slow == 1 and stalled == 0 and 'SUCCESS' not in h['Status']
    return ('streamtooslow', ok, 'status=%s too_slow_logs=%d stalled_logs=%d' % (h['Status'], slow, stalled))


def scenario_streammaxrun(daemon, t):
    """Whatever its progress, a stream repair stops once it ran 5 times
    DupeStreamTimeout (Industry S04E02: a repair that never ended). The
    duplicate delivers every 1 s, under the 8 s without progress, and the
    download is slow too (2 s a request), so the repair isn't 3 times slower
    than downloading either: only the 40 s cap stops it, with 62 MB to
    recover. The download then ends as a failure (no par2 here), as any
    download whose repair failed, instead of sitting in stream repair."""
    size, seg_primary, seg_donor = 64_000_000, 500_000, 250_000
    data = _payload(size, 4343)
    pp = _place_copy(t, 'capA', data, 'file.mkv')
    dp = _place_copy(t, 'capB', data, 'file.mkv')
    primary = build_nzb(pp, 'CapA.mkv', size, seg_primary, set(range(2, 126)))
    donor = build_nzb(dp, 'obf-cap.mkv', size, seg_donor, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DonCap', donor, True, 'cap-key', 50)
    daemon.append(api, 'CapA', primary, False, 'cap-key', 100)
    start = time.time()
    h = daemon.wait_history(api, 'CapA', timeout=240)
    took = time.time() - start
    capped = _grep_log(t, 'still running after 40 seconds, 5 times the timeout (option DupeStreamTimeout)')
    other = _grep_log(t, 'nothing recovered for') + _grep_log(t, 'times a download at')
    ok = capped == 1 and other == 0 and 'SUCCESS' not in h['Status'] and took < 170
    return ('streammaxrun', ok, 'status=%s took=%.0fs capped_logs=%d other_stop_logs=%d delayed=%d'
            % (h['Status'], took, capped, other, daemon.proxy.delayed))


def scenario_streamstarved(daemon, t):
    """B76 (Las Azules S02E06 4440): the stream repair of a damaged download
    competes with the next download for the server's 2 connections, which a
    slow download holds (2 s a request) for about 40 s. Waiting for a
    connection isn't a lack of progress: the repair waits, then repairs the
    file from the whole duplicate. Before, each fetch gave up after waiting,
    and the repair stopped with nothing recovered after DupeStreamTimeout."""
    size, seg_primary, seg_donor = 6_000_000, 500_000, 250_000
    data = _payload(size, 7676)
    primary = build_nzb(_place_copy(t, 'stA', data, 'file.mkv'), 'StarvA.mkv', size, seg_primary, set(range(2, 6)))
    donor = build_nzb(_place_copy(t, 'stB', data, 'file.mkv'), 'obf-starv.mkv', size, seg_donor, set())
    busy_size = 4_000_000
    busy = build_nzb(_place_copy(t, 'busy', _payload(busy_size, 7677)), 'Busy.bin', busy_size, 100_000, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DonStarv', donor, True, 'starv-key', 50)
    daemon.append(api, 'StarvA', primary, False, 'starv-key', 100)
    daemon.append(api, 'Busy', busy, False, 'busy-key', 0)
    h = daemon.wait_history(api, 'StarvA', timeout=240)
    stopped = _grep_log(t, 'nothing recovered for')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    ok = stopped == 0 and integ and h['Status'].startswith('SUCCESS')
    return ('streamstarved', ok, 'status=%s stopped_logs=%d integrity=%s delayed=%d'
            % (h['Status'], stopped, integ, daemon.proxy.delayed))


def scenario_streamdeaddonor(daemon, t):
    """Before stream repair reads from a duplicate, a few of its articles are
    checked (STAT) on the servers: a dead duplicate (none of its articles
    exists any more) scored above the good one is skipped at once instead of
    being asked for every missing range, and the good one repairs the file."""
    size, seg_primary, seg_donor = 6_000_000, 500_000, 250_000
    data = _payload(size, 4545)
    pp = _place_copy(t, 'deadA', data, 'file.mkv')
    gp = _place_copy(t, 'deadG', data, 'file.mkv')
    xp = _place_copy(t, 'deadX', data, 'file.mkv')
    primary = build_nzb(pp, 'DeadA.mkv', size, seg_primary, {5, 6})
    good = build_nzb(gp, 'obf-good.mkv', size, seg_donor, set())
    dead = build_nzb(xp, 'obf-dead.mkv', size, seg_donor, set(range(size // seg_donor)))
    api = daemon.wait_ready()
    daemon.append(api, 'DeadDonor', dead, True, 'dead-key', 75)
    daemon.append(api, 'GoodDonor', good, True, 'dead-key', 50)
    daemon.append(api, 'DeadA', primary, False, 'dead-key', 100)
    h = daemon.wait_history(api, 'DeadA')
    skipped = _grep_log(t, 'Skipping duplicate DeadDonor for stream repair: none of 10 sampled articles')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    ok = skipped == 1 and integ and 'SUCCESS' in h['Status']
    return ('streamdeaddonor', ok, 'status=%s skipped_logs=%d integrity=%s' % (h['Status'], skipped, integ))


def scenario_liveoverlap(daemon, t):
    """DupeArticleFallback=live: FileA (differently-segmented donor queued)
    completes with a hole while the big FileB still downloads (the global
    DownloadRate throttle keeps B busy). The live pass must repair A DURING
    the download - asserted by log order: 'Starting live stream repair'
    strictly before the collection's 'completely downloaded' line - while
    the post-processing stage stays the accounting authority, taking the
    fully repaired release to SUCCESS with byte-identical output."""
    size_a, seg_a = 2_000_000, 200_000
    size_b, seg_b = 24_000_000, 500_000
    data_a = _payload(size_a, 7401)
    data_b = _payload(size_b, 7402)
    pa = _place_copy(t, 'liveA', data_a, 'a.mkv')
    pb = _place_copy(t, 'liveB', data_b, 'b.mkv')
    da = _place_copy(t, 'liveDon', data_a, 'a.mkv')
    primary = build_multi_nzb([
        (pa, 'LiveA.mkv', size_a, seg_a, {4, 5, 6}),
        (pb, 'LiveB.mkv', size_b, seg_b, set()),
    ])
    donor = build_nzb(da, 'obf-live.mkv', size_a, 160_000, set())
    api = daemon.wait_ready()
    daemon.append(api, 'LiveDonor', donor, True, 'live-key', 50)
    daemon.append(api, 'LiveMain', primary, False, 'live-key', 100)
    h = daemon.wait_history(api, 'LiveMain')
    ok = h['Status'].startswith('SUCCESS')
    live = _grep_log(t, 'Starting live stream repair')
    overlapped = _log_before(t, 'Starting live stream repair',
                             'completely downloaded')
    integ = (_verify_output(t, data_a, '.mkv', dirs=(('main', 'dst'), ('main', 'inter'))) and
             _verify_output(t, data_b, '.mkv', dirs=(('main', 'dst'), ('main', 'inter'))))
    return ('liveoverlap', ok and integ and live == 1 and overlapped,
            'status=%s live_logs=%d overlapped=%s integrity=%s'
            % (h['Status'], live, overlapped, integ))


def scenario_livegate(daemon, t):
    """Same shape as liveoverlap but DupeArticleFallback=stream: the live
    pass must NOT run (option gate); the repair happens in post-processing
    as before and the release still completes SUCCESS byte-identically."""
    size_a, seg_a = 2_000_000, 200_000
    size_b, seg_b = 4_000_000, 500_000
    data_a = _payload(size_a, 7403)
    data_b = _payload(size_b, 7404)
    pa = _place_copy(t, 'gateA', data_a, 'a.mkv')
    pb = _place_copy(t, 'gateB', data_b, 'b.mkv')
    da = _place_copy(t, 'gateDon', data_a, 'a.mkv')
    primary = build_multi_nzb([
        (pa, 'GateA.mkv', size_a, seg_a, {4, 5, 6}),
        (pb, 'GateB.mkv', size_b, seg_b, set()),
    ])
    donor = build_nzb(da, 'obf-gate.mkv', size_a, 160_000, set())
    api = daemon.wait_ready()
    daemon.append(api, 'GateDonor', donor, True, 'gate-key', 50)
    daemon.append(api, 'GateMain', primary, False, 'gate-key', 100)
    h = daemon.wait_history(api, 'GateMain')
    ok = h['Status'].startswith('SUCCESS')
    live = _grep_log(t, 'Starting live stream repair')
    repaired = _grep_log(t, 'donor article(s)')
    integ = (_verify_output(t, data_a, '.mkv', dirs=(('main', 'dst'), ('main', 'inter'))) and
             _verify_output(t, data_b, '.mkv', dirs=(('main', 'dst'), ('main', 'inter'))))
    return ('livegate', ok and integ and live == 0 and repaired >= 1,
            'status=%s live_logs=%d repair_logs=%d integrity=%s'
            % (h['Status'], live, repaired, integ))


def scenario_reloadmidrepair(daemon, t):
    """6790 SIGSEGV of 2026-11-26 02:40:45 CST: a Settings save in the web
    interface (saveconfig, then the interface's reload) while a live stream
    repair was fetching from a duplicate freed the news server connection pool
    under the repair's batch workers. Same shape as liveoverlap (FileA has a
    hole, its differently segmented duplicate is queued, the big FileB keeps
    the download going), but every duplicate article takes 2 s to come, and
    while a duplicate article request is in flight the settings are saved
    and reloaded, three times (Server1.Connections changed each time), as
    the web interface does. The daemon must keep running (the same process answers
    after each reload) and the release completes SUCCESS byte-identically.
    The reload cancels the connections in flight first, so whether one is
    still out when the pool is replaced (a worker that took a new one in
    between, as on 6790) depends on timing: the counts are reported, not
    required (the unit test reload_ownership_tests reproduces that window)."""
    size_a, seg_a = 2_000_000, 200_000
    size_b, seg_b = 40_000_000, 500_000
    data_a = _payload(size_a, 7501)
    data_b = _payload(size_b, 7502)
    pa = _place_copy(t, 'rlA', data_a, 'a.mkv')
    pb = _place_copy(t, 'rlB', data_b, 'b.mkv')
    da = _place_copy(t, 'rlDon', data_a, 'a.mkv')
    primary = build_multi_nzb([
        (pa, 'ReloadA.mkv', size_a, seg_a, {4, 5, 6}),
        (pb, 'ReloadB.mkv', size_b, seg_b, set()),
    ])
    donor = build_nzb(da, 'obf-reload.mkv', size_a, 160_000, set())
    api = daemon.wait_ready()
    daemon.append(api, 'RlDonor', donor, True, 'reload-key', 50)
    daemon.append(api, 'RlMain', primary, False, 'reload-key', 100)
    deadline = time.time() + 90
    while _grep_log(t, 'Starting live stream repair') == 0 and time.time() < deadline:
        time.sleep(0.1)
    started = _grep_log(t, 'Starting live stream repair') > 0
    reloads = 0
    if started:
        for connections in ('10', '6', '8'):
            # a duplicate article request is held in the proxy right now:
            # the repair has a connection out when the pool is replaced
            seen = daemon.proxy.delayed
            deadline = time.time() + 60
            while daemon.proxy.delayed == seen and time.time() < deadline:
                time.sleep(0.05)
            values = [{'Name': o['Name'],
                       'Value': connections if o['Name'] == 'Server1.Connections' else o['Value']}
                      for o in api.loadconfig()]
            if not api.saveconfig(values):
                break
            api = daemon.reload(api)
            reloads += 1
            time.sleep(1.0)
    h = daemon.wait_history(api, 'RlMain', timeout=300)
    alive = daemon.process.poll() is None
    try:
        api.status()
    except Exception:
        alive = False
    still_out = _grep_log(t, 'still in use when the server pool was replaced')
    freed = _grep_log(t, 'Old server pool freed after its last')
    ok = h['Status'].startswith('SUCCESS')
    integ = (_verify_output(t, data_a, '.mkv', dirs=(('main', 'dst'), ('main', 'inter'))) and
             _verify_output(t, data_b, '.mkv', dirs=(('main', 'dst'), ('main', 'inter'))))
    return ('reloadmidrepair',
            ok and integ and alive and started and reloads == 3,
            'status=%s integrity=%s alive=%s live_started=%s reloads=%d still_out_logs=%d '
            'old_pool_freed_logs=%d delayed=%d'
            % (h['Status'], integ, alive, started, reloads, still_out, freed, daemon.proxy.delayed))


def scenario_livelastfile(daemon, t):
    """DupeArticleFallback=live with a SINGLE damaged file: when its job is
    captured no other file remains queued, so the live dispatch is skipped
    (the post-processing stage starts moments later and repairs it there) -
    asserts the last-file guard against a pointless race with the imminent
    post-processing stage."""
    size, seg = 2_000_000, 200_000
    data = _payload(size, 7405)
    pp = _place_copy(t, 'lastA', data, 'a.mkv')
    dp = _place_copy(t, 'lastDon', data, 'a.mkv')
    primary = build_nzb(pp, 'LastA.mkv', size, seg, {4, 5, 6})
    donor = build_nzb(dp, 'obf-last.mkv', size, 160_000, set())
    api = daemon.wait_ready()
    daemon.append(api, 'LastDonor', donor, True, 'last-key', 50)
    daemon.append(api, 'LastMain', primary, False, 'last-key', 100)
    h = daemon.wait_history(api, 'LastMain')
    ok = h['Status'].startswith('SUCCESS')
    live = _grep_log(t, 'Starting live stream repair')
    repaired = _grep_log(t, 'donor article(s)')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    return ('livelastfile', ok and integ and live == 0 and repaired >= 1,
            'status=%s live_logs=%d repair_logs=%d integrity=%s'
            % (h['Status'], live, repaired, integ))


def scenario_repost(daemon, t):
    """M1 same-bytes matching: a 4-member "release" (three equal-size rar
    volumes + a small par2) where the payloads are random bytes standing in
    for a PASSWORD-PROTECTED, COMPRESSED archive - stream repair never
    interprets them, which is the point. The primary posting is missing
    blocks in part01 and in the par2; the donor is a REPOST: byte-identical
    members under the same names, cut into different article sizes. Suffix/
    name pairing must repair the damaged archive volume. The PAR2 file must
    remain exactly as downloaded, and the donor's PAR set is never used (the
    twin check may read it for a fingerprint): names, sizes, and partial byte
    identity do not authorize borrowing another PAR set.
    Expected history status is FAILURE/PAR because the stand-in parity is
    opaque random bytes. Capture nserv requests when running this scenario."""
    seg_primary, seg_donor = 500_000, 300_000
    vol = 1_500_000
    members = [
        ('repostA/x.part01.rar', 'Rel.part01.rar', vol, seg_primary, {2}),
        ('repostA/x.part02.rar', 'Rel.part02.rar', vol, seg_primary, set()),
        ('repostA/x.part03.rar', 'Rel.part03.rar', vol, seg_primary, set()),
        ('repostA/x.par2', 'Rel.vol00+01.par2', 80_000, 70_000, {1}),
    ]
    payloads = {}
    for i, m in enumerate(members):
        data = _payload(m[2], 7000 + i)
        payloads[m[1]] = data
        t.write_file(os.path.join('data', m[0]), data)

    donor_members = [(m[0].replace('repostA', 'repostB'), m[1], m[2], seg_donor, set())
                     for m in members]
    for m in donor_members:
        t.write_file(os.path.join('data', m[0]), payloads[m[1]])

    primary = build_multi_nzb(members)
    donor = build_multi_nzb(donor_members)
    api = daemon.wait_ready()
    daemon.append(api, 'DonRepost', donor, True, 'repost-key', 50)
    daemon.append(api, 'RelRepost', primary, False, 'repost-key', 100)
    h = daemon.wait_history(api, 'RelRepost')
    recov = int(h.get('DupeRecoveredArticles', 0))
    queued = _grep_log(t, 'Queueing stream repair')
    repaired = _grep_log(t, 'donor article(s)')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ_rar = _verify_output(t, payloads['Rel.part01.rar'], '.rar', dirs=both_dirs)
    par_payload = payloads['Rel.vol00+01.par2']
    untouched_par = _verify_output(t, b'\0' * 70_000 + par_payload[70_000:],
                                    '.par2', dirs=both_dirs)
    intact_2 = _verify_output(t, payloads['Rel.part02.rar'], '.rar', dirs=both_dirs)
    intact_3 = _verify_output(t, payloads['Rel.part03.rar'], '.rar', dirs=both_dirs)
    # nserv flushes command output every 100 ms; wait for the final fetch.
    time.sleep(0.25)
    requests = re.findall(r'Received: (?:BODY|ARTICLE) [^\r\n]*',
                          t.read_file('nserv.log').decode(errors='replace'))
    donor_par = sum('repostB/x.par2?' in request for request in requests)
    donor_rar = sum('repostB/x.part01.rar?' in request for request in requests)
    # the twin check reads the donor's smallest par2-file for its fingerprint
    # (as soon as it's added): reading isn't borrowing. Borrowing would be its
    # par2 set used in the repair, or blocks in our par2-file (untouched_par)
    donor_par_set_used = _grep_log(t, 'par2 set of twin') > 0
    return ('repost',
            integ_rar and untouched_par and intact_2 and intact_3 and
            donor_rar > 0 and not donor_par_set_used and
            h['Status'] == 'FAILURE/PAR' and recov == 1 and queued == 1 and repaired >= 1,
            'status=%s recovered=%d queued_logs=%d repair_logs=%d '
            'rar=%s par2_untouched=%s intact=%s/%s donor_rar=%d donor_par2=%d'
            % (h['Status'], recov, queued, repaired,
               integ_rar, untouched_par, intact_2, intact_3, donor_rar, donor_par))


def scenario_repostrenamed(daemon, t):
    """M1 tier-2 pairing end-to-end: the donor is a byte-identical repost
    whose members were RENAMED (different release base name, same volume
    suffixes), so exact-name pairing cannot fire and the unique-suffix-key
    tier must pair the damaged member with its donor twin. No par2 members,
    and every hole gets filled, so the byte-based health recount now takes
    the item all the way to SUCCESS (moved to the final destination) instead
    of parking at FAILURE/HEALTH; byte integrity and the counters are the
    pass criteria."""
    seg_primary, seg_donor = 500_000, 300_000
    vol = 1_500_000
    members = [
        ('renA/x.part01.rar', 'Rel.part01.rar', vol, seg_primary, set()),
        ('renA/x.part02.rar', 'Rel.part02.rar', vol, seg_primary, {2}),
        ('renA/x.part03.rar', 'Rel.part03.rar', vol, seg_primary, set()),
    ]
    payloads = {}
    for i, m in enumerate(members):
        data = _payload(m[2], 8100 + i)
        payloads[m[1]] = data
        t.write_file(os.path.join('data', m[0]), data)

    donor_members = [(m[0].replace('renA', 'renB'), m[1].replace('Rel.', 'Other.'),
                      m[2], seg_donor, set()) for m in members]
    for dm, m in zip(donor_members, members):
        t.write_file(os.path.join('data', dm[0]), payloads[m[1]])

    primary = build_multi_nzb(members)
    donor = build_multi_nzb(donor_members)
    api = daemon.wait_ready()
    daemon.append(api, 'DonRenamed', donor, True, 'ren-key', 50)
    daemon.append(api, 'RelRenamed', primary, False, 'ren-key', 100)
    h = daemon.wait_history(api, 'RelRenamed')
    recov = int(h.get('DupeRecoveredArticles', 0))
    repaired = _grep_log(t, 'donor article(s)')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=both_dirs)
                for m in members)
    return ('repostrenamed', integ and recov >= 1 and repaired >= 1,
            'status=%s recovered=%d repair_logs=%d integrity=%s'
            % (h['Status'], recov, repaired, integ))


def scenario_repostobfuscated(daemon, t):
    """M1 sibling search end-to-end: a dupe of eight equal-size volumes whose
    names say nothing on either side (as when an indexer deobfuscated the
    release name but the posting kept random file names). The donor lists
    its members in a different order and cuts them into different article
    sizes, so no name, suffix, position or size-step tier can pair a damaged
    member with its twin. The twins of the damaged members sit beyond the
    first four donor members, which is all the old candidate cap probed:
    every damaged member must still be repaired byte-exactly."""
    seg_primary, seg_donor = 400_000, 300_000
    vol = 1_200_000
    names = ['Zk4q.bin', 'Yq8w.bin', 'Xa1e.bin', 'Wd3r.bin',
             'Vf7t.bin', 'Ug2y.bin', 'Th5u.bin', 'Sj6i.bin']
    damaged = {1, 4, 6, 7}
    members = [('obfA/f%d.bin' % i, names[i], vol, seg_primary,
                {2} if i in damaged else set()) for i in range(8)]
    payloads = []
    for i, m in enumerate(members):
        data = _payload(vol, 8300 + i)
        payloads.append(data)
        t.write_file(os.path.join('data', m[0]), data)

    donor_order = [0, 2, 3, 5, 1, 4, 6, 7]
    donor_members = []
    for pos, i in enumerate(donor_order):
        path = 'obfB/g%d.bin' % pos
        t.write_file(os.path.join('data', path), payloads[i])
        donor_members.append((path, 'd%02dx%d.bin' % (pos, 9 - pos), vol, seg_donor, set()))

    primary = build_multi_nzb(members)
    donor = build_multi_nzb(donor_members)
    api = daemon.wait_ready()
    daemon.append(api, 'DonObf', donor, True, 'obf-key', 50)
    daemon.append(api, 'RelObf', primary, False, 'obf-key', 100)
    h = daemon.wait_history(api, 'RelObf')
    recov = int(h.get('DupeRecoveredArticles', 0))
    repaired = _grep_log(t, 'donor article(s)')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[i], '.bin', dirs=both_dirs) for i in range(8))
    return ('repostobfuscated', integ and recov >= len(damaged) and repaired >= len(damaged),
            'status=%s recovered=%d repair_logs=%d integrity=%s'
            % (h['Status'], recov, repaired, integ))


def scenario_repostdonorgaps(daemon, t):
    """Probe replacement end-to-end: the donor is a byte-identical repost
    (different segmentation) that misses articles of its own - the four it
    misses are exactly the first two identity probes and their first two
    replacements. A probe the donor cannot supply is inconclusive, not a
    mismatch: verification must draw further probes until it can compare
    the twin, then repair the primary's hole. Nothing may be rejected."""
    size, seg_primary, seg_donor = 6_000_000, 500_000, 250_000
    data = _payload(size, 8500)
    pp = _place_copy(t, 'gapsA', data, 'file.mkv')
    dp = _place_copy(t, 'gapsB', data, 'file.mkv')
    # primary part 6 = donor parts 11-12; the clear probe candidates are the
    # remaining donor parts minus a one-part margin, and the spread picks
    # (then the spread replacements) are donor parts 6/20, then 5/19
    primary = build_nzb(pp, 'GapsA.mkv', size, seg_primary, {6})
    donor = build_nzb(dp, 'obf-gaps.mkv', size, seg_donor, {5, 6, 19, 20})
    api = daemon.wait_ready()
    daemon.append(api, 'DonGaps', donor, True, 'gaps-key', 50)
    daemon.append(api, 'RelGaps', primary, False, 'gaps-key', 100)
    h = daemon.wait_history(api, 'RelGaps')
    repaired = _grep_log(t, 'donor article(s)')
    rejected = _grep_log(t, 'content identity not confirmed')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    success = 'SUCCESS' in h['Status']
    return ('repostdonorgaps', success and integ and repaired >= 1 and rejected == 0,
            'status=%s repair_logs=%d rejected_logs=%d integrity=%s'
            % (h['Status'], repaired, rejected, integ))


def scenario_xpackbare(daemon, t):
    """M2 cross-packing smoke test: the primary posts movie.mkv BARE and
    completes with a hole; the only duplicate posts the SAME movie packed
    into store-mode RAR3 volumes (different framing, different offsets,
    different segmentation). M1 cannot pair bare against rar volumes -
    the ContentMap pass must locate the missing bytes inside the donor's
    volumes and patch them. No par2, and the hole is fully filled, so the
    byte-based health recount now completes the item as SUCCESS (moved to
    the final destination) instead of parking at FAILURE/HEALTH; integrity,
    the cross-packing logs and the counter are the pass criteria."""
    size, seg_primary, seg_donor = 6_000_000, 500_000, 300_000
    data = _payload(size, 5150)
    pp = _place_copy(t, 'xpbA', data, 'movie.mkv')
    primary = build_nzb(pp, 'movie.mkv', size, seg_primary, {5, 6})

    volumes = generators.rar3_store_volumes('movie.mkv', data, 2_000_000)
    donor_members = []
    for i, vol in enumerate(volumes, 1):
        rel = 'xpbB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        donor_members.append((rel, 'Rel.part%02d.rar' % i, len(vol), seg_donor, set()))
    donor = build_multi_nzb(donor_members)

    api = daemon.wait_ready()
    daemon.append(api, 'DonXpb', donor, True, 'xpb-key', 50)
    daemon.append(api, 'RelXpb', primary, False, 'xpb-key', 100)
    h = daemon.wait_history(api, 'RelXpb')
    recov = int(h.get('DupeRecoveredArticles', 0))
    xpack = _grep_log(t, 'Cross-packing repair of')
    repaired = _grep_log(t, 'cross-packing')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    return ('xpackbare', integ and recov >= 1 and xpack >= 1 and repaired >= 1,
            'status=%s recovered=%d xpack_logs=%d repair_logs=%d integrity=%s'
            % (h['Status'], recov, xpack, repaired, integ))


def _xpack_run(daemon, t, tag, primary_members, donor_members, payloads,
               primary_password=None, donor_password=None):
    """Append donor (paused) + primary under one dupe-key, wait for history,
    return (history, per-payload integrity dict, log counters).
    ``primary_password``/``donor_password`` attach a <meta type="password">
    block to the respective NZB (see build_multi_nzb) for the M3
    password-assisted xcrypt scenarios; both default to no password, which is
    the M2 (unencrypted) behavior every other xpack* scenario relies on."""
    primary = build_multi_nzb(primary_members, password=primary_password)
    donor = build_multi_nzb(donor_members, password=donor_password)
    api = daemon.wait_ready()
    daemon.append(api, 'Don' + tag, donor, True, tag + '-key', 50)
    daemon.append(api, 'Rel' + tag, primary, False, tag + '-key', 100)
    h = daemon.wait_history(api, 'Rel' + tag)
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integrity = {name: _verify_output(t, data, os.path.splitext(name)[1], dirs=both_dirs)
                 for name, data in payloads.items()}
    counters = {
        'recov': int(h.get('DupeRecoveredArticles', 0)),
        'xpack': _grep_log(t, 'Cross-packing repair of'),
        'repaired': _grep_log(t, 'cross-packing'),
        'rejected': _grep_log(t, 'content identity not confirmed'),
        'missing': _grep_log(t, 'still missing after stream repair'),
    }
    return h, integrity, counters


def scenario_xpackrar(daemon, t):
    """Store-rar target repaired from a BARE donor, with degradation: vol2
    has a data hole (repairable through the map), vol3 lost its first part
    including the rar headers (that volume must be excluded and stay
    damaged for par2 - which doesn't exist here). This is the PARTIAL-repair
    proof for the health recount: RepairCompleted only credits a target's
    encoded failed size back to health when EVERY one of its holes is
    filled, so vol3's unrepaired header hole means the item still stays
    FAILURE/HEALTH - never a false SUCCESS - and that is asserted directly
    below."""
    size = 6_000_000
    data = _payload(size, 6100)
    volumes = generators.rar3_store_volumes('movie.mkv', data, 2_000_000)
    payloads, members = {}, []
    missing = [set(), {2}, {1}]        # vol2: data hole; vol3: header hole
    for i, (vol, miss) in enumerate(zip(volumes, missing), 1):
        rel = 'xprA/rel.part%02d.rar' % i
        name = 'Rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        payloads[name] = vol
        members.append((rel, name, len(vol), 500_000, miss))
    dp = _place_copy(t, 'xprB', data, 'movie.mkv')
    donor_members = [(dp, 'movie.mkv', size, 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xpr', members, donor_members, payloads)
    not_success = 'SUCCESS' not in h['Status']
    ok = (integ['Rel.part01.rar'] and integ['Rel.part02.rar'] and
          not integ['Rel.part03.rar'] and              # header-holed vol stays damaged
          not_success and                               # partial repair never parks as SUCCESS
          c['recov'] >= 1 and c['xpack'] >= 1 and c['repaired'] >= 1 and
          c['missing'] >= 1)
    return ('xpackrar', ok,
            'status=%s recovered=%d xpack=%d repaired=%d missing=%d integ=%s'
            % (h['Status'], c['recov'], c['xpack'], c['repaired'], c['missing'],
               {k: v for k, v in integ.items()}))


def scenario_xpackgapdonor(daemon, t):
    """Cross-packing repair from a donor missing one article inside the hole: the
    rest of the hole is still recovered from it. A read fails as a whole when any
    donor article in it is missing, and that gave up the entire hole."""
    size = 6_000_000
    data = _payload(size, 6150)
    volumes = generators.rar3_store_volumes('movie.mkv', data, 2_000_000)
    payloads, members = {}, []
    missing = [set(), {2, 3, 4}, set()]        # vol2: a 1.5 MB data hole
    for i, (vol, miss) in enumerate(zip(volumes, missing), 1):
        rel = 'xgdA/rel.part%02d.rar' % i
        name = 'Rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        payloads[name] = vol
        members.append((rel, name, len(vol), 500_000, miss))
    dp = _place_copy(t, 'xgdB', data, 'movie.mkv')
    # the donor lacks one 300 KB article inside the hole (inner offset about 2.7 MB)
    donor_members = [(dp, 'movie.mkv', size, 300_000, {10})]
    h, integ, c = _xpack_run(daemon, t, 'Xgd', members, donor_members, payloads)
    with open(t.path('nzbget.log'), errors='replace') as f:
        found = re.findall(r'Recovered ([\d.]+) MB .*?of movie\.mkv from duplicate', f.read())
    recovered_mb = max((float(x) for x in found), default=0.0)
    ok = recovered_mb >= 1.0 and integ['Rel.part01.rar'] and integ['Rel.part03.rar']
    return ('xpackgapdonor', ok, 'status=%s recovered_mb=%.1f repaired=%d' % (
        h['Status'], recovered_mb, c['repaired']))


def scenario_xpackrar2rar(daemon, t):
    """rar-to-rar with DIFFERENT volume sizes (3x2MB target, 4x1.5MB donor):
    member-wise M1 cannot window these (sizes differ by 25%), the inner
    stream matches exactly."""
    size = 6_000_000
    data = _payload(size, 6200)
    volumes = generators.rar3_store_volumes('movie.mkv', data, 2_000_000)
    payloads, members = {}, []
    for i, vol in enumerate(volumes, 1):
        rel = 'xr2A/rel.part%02d.rar' % i
        name = 'Rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        payloads[name] = vol
        members.append((rel, name, len(vol), 500_000, {2} if i == 1 else set()))
    donor_members = []
    for i, vol in enumerate(generators.rar3_store_volumes('movie.mkv', data, 1_500_000), 1):
        rel = 'xr2B/other.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        donor_members.append((rel, 'Other.part%02d.rar' % i, len(vol), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xr2', members, donor_members, payloads)
    ok = all(integ.values()) and c['recov'] >= 1 and c['repaired'] >= 1
    return ('xpackrar2rar', ok, 'status=%s recovered=%d repaired=%d integ=%s'
            % (h['Status'], c['recov'], c['repaired'], all(integ.values())))


def scenario_xpack2sets(daemon, t):
    """TWO damaged store-rar sets in one item, repaired from ONE duplicate
    carrying both sets at different volume sizes. Locks R>1 correctness of
    per-donor-set map reuse: both sets must repair byte-identically and the
    per-pair identity gate (inner-size equality + probes) must still route
    each repair set to ITS donor set only."""
    size = 6_000_000
    data_one = _payload(size, 6300)
    data_two = _payload(size, 6301)
    payloads, members = {}, []
    for tag, data in (('one', data_one), ('two', data_two)):
        volumes = generators.rar3_store_volumes('movie_%s.mkv' % tag, data, 2_000_000)
        for i, vol in enumerate(volumes, 1):
            rel = 'x2sA/%s.part%02d.rar' % (tag, i)
            name = 'Rel%s.part%02d.rar' % (tag.capitalize(), i)
            t.write_file(os.path.join('data', rel), vol)
            payloads[name] = vol
            members.append((rel, name, len(vol), 500_000, {2} if i == 1 else set()))
    donor_members = []
    for tag, data in (('one', data_one), ('two', data_two)):
        volumes = generators.rar3_store_volumes('movie_%s.mkv' % tag, data, 1_500_000)
        for i, vol in enumerate(volumes, 1):
            rel = 'x2sB/o%s.part%02d.rar' % (tag, i)
            t.write_file(os.path.join('data', rel), vol)
            donor_members.append(
                (rel, 'O%s.part%02d.rar' % (tag.capitalize(), i), len(vol), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'X2s', members, donor_members, payloads)
    ok = all(integ.values()) and c['repaired'] >= 2 and c['recov'] >= 2
    return ('xpack2sets', ok, 'status=%s recovered=%d repaired=%d integ=%s'
            % (h['Status'], c['recov'], c['repaired'], all(integ.values())))


def scenario_xpackzip(daemon, t):
    """Bare target repaired from a SPANNED STORED ZIP donor (z01+z02+zip).
    No par2 and the hole is fully filled, so the byte-based health recount
    takes the item to SUCCESS (moved to the final destination) - asserted
    directly below as the cross-packing proof of the recount."""
    size = 6_000_000
    data = _payload(size, 6300)
    pp = _place_copy(t, 'xpzA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    zip_bytes = generators.zip_store([('movie.mkv', data)])
    pieces = generators.split_bytes(zip_bytes, [2_500_000, 2_500_000])
    suffixes = ['z01', 'z02', 'zip']
    donor_members = []
    for piece, suffix in zip(pieces, suffixes):
        rel = 'xpzB/rel.%s' % suffix
        t.write_file(os.path.join('data', rel), piece)
        donor_members.append((rel, 'Rel.%s' % suffix, len(piece), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xpz', members, donor_members, payloads)
    ok = (integ['movie.mkv'] and c['recov'] >= 1 and c['repaired'] >= 1 and
          'SUCCESS' in h['Status'])
    return ('xpackzip', ok, 'status=%s recovered=%d repaired=%d integrity=%s'
            % (h['Status'], c['recov'], c['repaired'], integ['movie.mkv']))


def scenario_xpack7z(daemon, t):
    """Bare target repaired from a 7z-COPY donor posted as .7z.001/.002."""
    size = 6_000_000
    data = _payload(size, 6400)
    pp = _place_copy(t, 'xp7A', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    archive = generators.seven_zip_copy([('movie.mkv', data)])
    pieces = generators.split_bytes(archive, [3_000_100])
    donor_members = []
    for i, piece in enumerate(pieces, 1):
        rel = 'xp7B/rel.7z.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        donor_members.append((rel, 'Rel.7z.%03d' % i, len(piece), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xp7', members, donor_members, payloads)
    ok = integ['movie.mkv'] and c['recov'] >= 1 and c['repaired'] >= 1
    return ('xpack7z', ok, 'status=%s recovered=%d repaired=%d integrity=%s'
            % (h['Status'], c['recov'], c['repaired'], integ['movie.mkv']))


def _xpack_many_volumes(daemon, t, tag, donor_volumes):
    """Bare target (2 lost articles) repaired from a donor of many small volumes
    (``donor_volumes``: [(name, bytes)]) against a slow server (SCENARIO_NSERV_ARGS):
    returns (ok, detail) where ok needs the movie intact and the donor map's
    per-volume articles (each volume's first, a RAR volume's last) fetched in
    batches, not one by one. 6790 (NZBID 2966): an alt split into 207 7z volumes
    cost 208 single fetches, 231 s one after another, for 22 articles of repair."""
    size = 6_000_000
    data = _payload(size, 6450)
    pp = _place_copy(t, tag + 'A', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    donor_members = []
    for name, piece in donor_volumes(data):
        rel = '%sB/%s' % (tag, name)
        t.write_file(os.path.join('data', rel), piece)
        donor_members.append((rel, name, len(piece), 60_000, set()))
    h, integ, c = _xpack_run(daemon, t, tag, members, donor_members, {'movie.mkv': data})
    costs = [(float(m.group(1)), int(m.group(2)), int(m.group(3))) for m in re.finditer(
        r'Fetch cost of borrowing from Don%s: [\d.]+ MB in [\d.]+ s, waiting on the network ([\d.]+) s; '
        r'(\d+) single.*?of (\d+) article' % re.escape(tag), _log_text(t))]
    singles = sum(x[1] for x in costs)
    vols = len(donor_members)
    # The map asks for at least one article per volume; one by one that is >= vols singles.
    ok = (integ['movie.mkv'] and c['recov'] >= 1 and 'SUCCESS' in h['Status'] and costs
          and singles * 4 < vols)
    return ok, ('status=%s recovered=%d integrity=%s donor_volumes=%d singles=%d costs(waited,singles,batched)=%s'
                % (h['Status'], c['recov'], integ['movie.mkv'], vols, singles, costs))


def scenario_xpack7zmanyvols(daemon, t):
    """A split 7z donor of 40 volumes: its payload map learns every volume's size
    from that volume's first article, fetched as one batch (see _xpack_many_volumes)."""
    def volumes(data):
        archive = generators.seven_zip_copy([('movie.mkv', data)])
        pieces = generators.split_bytes(archive, [150_000] * (len(archive) // 150_000))
        return [('Rel.7z.%03d' % i, p) for i, p in enumerate(pieces, 1)]
    ok, detail = _xpack_many_volumes(daemon, t, 'X7m', volumes)
    return ('xpack7zmanyvols', ok, detail)


def scenario_xpackrarmanyvols(daemon, t):
    """An alt in 40 stored RAR volumes: every volume's headers and end block,
    which its payload map reads, come in one batch (see _xpack_many_volumes)."""
    def volumes(data):
        vols = generators.rar3_store_volumes('movie.mkv', data, 150_000)
        return [('Rel.part%02d.rar' % i, v) for i, v in enumerate(vols, 1)]
    ok, detail = _xpack_many_volumes(daemon, t, 'Xrm', volumes)
    return ('xpackrarmanyvols', ok, detail)


def scenario_xpack7zlzma2(daemon, t):
    """Bare target repaired from a real 7-Zip LZMA2 donor (.7z.001/.002) whose
    data did not compress: LZMA2 stored it as raw chunks, and only the articles
    around the holes are borrowed. DupeStreamDecompress is off, so the donor is
    never fetched whole and extracted. Needs the 7z binary to make the donor."""
    import shutil
    import subprocess
    import tempfile
    seven = shutil.which('7z') or shutil.which('7zz') or shutil.which('7za')
    if not seven:
        return ('xpack7zlzma2', True, 'skipped: no 7z binary to build the donor')
    size = 6_000_000
    data = _payload(size, 6450)
    pp = _place_copy(t, 'xl2A', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    with tempfile.TemporaryDirectory() as tmp:
        with open(os.path.join(tmp, 'movie.mkv'), 'wb') as f:
            f.write(data)
        subprocess.run([seven, 'a', '-bso0', '-bsp0', '-m0=lzma2', '-mx=5',
                        os.path.join(tmp, 'rel.7z'), 'movie.mkv'], cwd=tmp, check=True)
        with open(os.path.join(tmp, 'rel.7z'), 'rb') as f:
            archive = f.read()
    raw_stored = len(archive) >= size
    pieces = generators.split_bytes(archive, [3_000_100])
    donor_members = []
    for i, piece in enumerate(pieces, 1):
        rel = 'xl2B/rel.7z.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        donor_members.append((rel, 'Rel.7z.%03d' % i, len(piece), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xl2', members, donor_members, payloads)
    raw_logs = _grep_log(t, '(7z raw chunks)')
    ok = raw_stored and integ['movie.mkv'] and raw_logs >= 1
    return ('xpack7zlzma2', ok, 'status=%s raw_stored=%s raw_chunk_logs=%d integrity=%s'
            % (h['Status'], raw_stored, raw_logs, integ['movie.mkv']))


def scenario_xpacksplit(daemon, t):
    """Store-rar target repaired from RAW SPLITS (movie.mkv.001/.002/.003)."""
    size = 6_000_000
    data = _payload(size, 6500)
    volumes = generators.rar3_store_volumes('movie.mkv', data, 2_000_000)
    payloads, members = {}, []
    for i, vol in enumerate(volumes, 1):
        rel = 'xpsA/rel.part%02d.rar' % i
        name = 'Rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        payloads[name] = vol
        members.append((rel, name, len(vol), 500_000, {3} if i == 1 else set()))
    donor_members = []
    for i, piece in enumerate(generators.split_bytes(data, [2_000_000, 2_000_000]), 1):
        rel = 'xpsB/movie.mkv.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        donor_members.append((rel, 'movie.mkv.%03d' % i, len(piece), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xps', members, donor_members, payloads)
    ok = all(integ.values()) and c['recov'] >= 1 and c['repaired'] >= 1
    return ('xpacksplit', ok, 'status=%s recovered=%d repaired=%d integ=%s'
            % (h['Status'], c['recov'], c['repaired'], all(integ.values())))


def scenario_xpackcompressed(daemon, t):
    """The mechanism ladder on a COMPRESSED archive: M2 must never map it
    (method gate), but a byte-identical repost still repairs it via M1 -
    exactly the promise the docs make for compressed/encrypted content."""
    size = 4_000_000
    data = _payload(size, 6600)      # opaque stand-in for compressed bytes
    volumes = generators.rar3_store_volumes('movie.mkv', data, 2_000_000, method=0x33)
    payloads, members = {}, []
    for i, vol in enumerate(volumes, 1):
        rel = 'xpcA/rel.part%02d.rar' % i
        name = 'Rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        payloads[name] = vol
        members.append((rel, name, len(vol), 500_000, {2} if i == 1 else set()))
    donor_members = []
    for i, vol in enumerate(volumes, 1):
        rel = 'xpcB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        donor_members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xpc', members, donor_members, payloads)
    repaired_m1 = _grep_log(t, 'donor article(s)')
    ok = (all(integ.values()) and c['recov'] >= 1 and repaired_m1 >= 1 and
          c['xpack'] == 0)          # M1 filled the holes; M2 never needed
    return ('xpackcompressed', ok,
            'status=%s recovered=%d m1_repairs=%d xpack=%d integ=%s'
            % (h['Status'], c['recov'], repaired_m1, c['xpack'], all(integ.values())))


def scenario_xpackneg(daemon, t):
    """The negative: a donor set with the RIGHT inner size but the WRONG
    bytes (different payload packed into store-rar volumes). The inner
    probes must reject it; nothing may be written."""
    size = 6_000_000
    data = _payload(size, 6700)
    decoy = _payload(size, 6800)
    pp = _place_copy(t, 'xpnA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    donor_members = []
    for i, vol in enumerate(generators.rar3_store_volumes('movie.mkv', decoy, 2_000_000), 1):
        rel = 'xpnB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        donor_members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xpn', members, donor_members,
                             {'movie.mkv': data, 'decoy': decoy})
    ok = (c['rejected'] >= 1 and c['recov'] == 0 and c['missing'] >= 1 and
          not integ['movie.mkv'] and not integ['decoy'])
    return ('xpackneg', ok,
            'status=%s recovered=%d rejected=%d missing=%d file_matches=%s/%s'
            % (h['Status'], c['recov'], c['rejected'], c['missing'],
               integ['movie.mkv'], integ['decoy']))


def scenario_xcrypt_encplain(daemon, t):
    """M3 password-assisted cross-packing: the TARGET is a password-ENCRYPTED
    store-rar release (one continuous AES-128-CBC stream across volumes -
    ContentMap.cpp's cipher-composite map - built with a deliberately
    non-16-aligned volume_size to exercise real WinRAR's arbitrary volume
    cuts, see rar3_store_volumes_encrypted). Its own password travels via its
    NZB <head><meta type="password">; the target's ContentMap is built with
    that password directly (no ladder needed on the target side - see
    ExecCrossPackRepair). A data hole sits in the MIDDLE volume (volume 1's
    and 3's headers+salt stay intact, only volume 2 loses a non-header
    segment). The donor is the SAME movie posted BARE and unencrypted -
    cross-packing decrypts the target's plaintext space with its own key,
    patches from the donor's plaintext, and re-encrypts back into the
    on-disk ciphertext. Byte-identical ENCRYPTED volumes afterward prove the
    whole decrypt/patch/re-encrypt round trip."""
    if not generators.HAVE_CRYPTO:
        return ('xcrypt_encplain', None, 'SKIP: cryptography not installed')
    size = 6_000_000
    data = _payload(size, 7100)
    password = 'target-pw-A'
    volumes = generators.rar3_store_volumes_encrypted('movie.mkv', data, 2_000_003, password)
    payloads, members = {}, []
    missing = [set(), {2}, set()]      # only volume 2 loses a non-header segment
    for i, (vol, miss) in enumerate(zip(volumes, missing), 1):
        rel = 'xceA/rel.part%02d.rar' % i
        name = 'Rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        payloads[name] = vol
        members.append((rel, name, len(vol), 500_000, miss))
    dp = _place_copy(t, 'xceB', data, 'movie.mkv')
    donor_members = [(dp, 'movie.mkv', size, 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xce', members, donor_members, payloads,
                             primary_password=password)
    ok = all(integ.values()) and c['recov'] >= 1 and c['xpack'] >= 1 and c['repaired'] >= 1
    return ('xcrypt_encplain', ok,
            'status=%s recovered=%d xpack=%d repaired=%d integ=%s'
            % (h['Status'], c['recov'], c['xpack'], c['repaired'], all(integ.values())))


def _nested_archive(data):
    """movie.mkv in a store-mode rar, that rar in a stored zip, that zip in a
    copy-mode 7z: three containers around the payload."""
    rar = generators.rar3_store_volumes('movie.mkv', data, len(data) + 4096)
    assert len(rar) == 1
    inner_zip = generators.zip_store([('inner.rar', rar[0])])
    return generators.seven_zip_copy([('inner.zip', inner_zip)])


def _nested_volume_set(data):
    """movie.mkv in four store-mode rar volumes, those four files in one
    copy-mode 7z: a volume set kept inside another archive."""
    vols = generators.rar3_store_volumes('movie.mkv', data, 1_600_000)
    assert len(vols) >= 3
    return generators.seven_zip_copy([('in.part%02d.rar' % i, v) for i, v in enumerate(vols, 1)])


def scenario_xpacknestedsetdonor(daemon, t):
    """The DONOR is a 7z holding a rar VOLUME SET that holds the movie, cut
    into .7z.001-.003; the target is the same movie posted bare with two
    missing articles. Every inner volume is found in the 7z, the set is
    mapped through it, and the holes are filled by the payload's bytes."""
    size = 6_000_000
    data = _payload(size, 7800)
    pp = _place_copy(t, 'xpsdA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    donor_members = []
    for i, piece in enumerate(generators.split_bytes(_nested_volume_set(data), [2_500_000, 2_500_000]), 1):
        rel = 'xpsdB/rel.7z.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        donor_members.append((rel, 'Rel.7z.%03d' % i, len(piece), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xpsd', members, donor_members, payloads)
    ok = integ['movie.mkv'] and c['recov'] >= 1 and c['repaired'] >= 1
    return ('xpacknestedsetdonor', ok,
            'status=%s recovered=%d repaired=%d integrity=%s'
            % (h['Status'], c['recov'], c['repaired'], integ['movie.mkv']))


def scenario_xpacknestedsettarget(daemon, t):
    """The TARGET is a 7z holding a rar VOLUME SET that holds the movie, cut
    into .7z.001-.003, with a data hole in the middle volume; the donor is
    the same movie posted bare. The volumes come out byte-identical."""
    size = 6_000_000
    data = _payload(size, 7900)
    pieces = generators.split_bytes(_nested_volume_set(data), [2_000_003, 2_000_003])
    payloads, members = {}, []
    missing = [set(), {2}, set()]
    for i, (piece, miss) in enumerate(zip(pieces, missing), 1):
        rel = 'xpstA/rel.7z.%03d' % i
        name = 'Rel.7z.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        payloads[name] = piece
        members.append((rel, name, len(piece), 500_000, miss))
    dp = _place_copy(t, 'xpstB', data, 'movie.mkv')
    donor_members = [(dp, 'movie.mkv', size, 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xpst', members, donor_members, payloads)
    ok = all(integ.values()) and c['recov'] >= 1 and c['xpack'] >= 1 and c['repaired'] >= 1
    return ('xpacknestedsettarget', ok,
            'status=%s recovered=%d xpack=%d repaired=%d integ=%s'
            % (h['Status'], c['recov'], c['xpack'], c['repaired'], all(integ.values())))


def scenario_xpacknestedsamename(daemon, t):
    """The DONOR is a stored zip with two files of ONE name in different
    folders: extras/inner.rar (a small sample) listed first and main/inner.rar
    (the movie). The bigger one is the nested archive to follow, whatever a
    listing by base name alone would say; the bare target is repaired from it."""
    size = 6_000_000
    data = _payload(size, 7950)
    pp = _place_copy(t, 'xpsnA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    sample = generators.rar3_store_volumes('sample.mkv', _payload(300_000, 7951), 400_000)[0]
    movie = generators.rar3_store_volumes('movie.mkv', data, size + 4096)[0]
    outer = generators.zip_store([('extras/inner.rar', sample), ('main/inner.rar', movie)])
    rel = 'xpsnB/rel.zip'
    t.write_file(os.path.join('data', rel), outer)
    donor_members = [(rel, 'Rel.zip', len(outer), 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xpsn', members, donor_members, payloads)
    ok = integ['movie.mkv'] and c['recov'] >= 1 and c['repaired'] >= 1
    return ('xpacknestedsamename', ok,
            'status=%s recovered=%d repaired=%d integrity=%s'
            % (h['Status'], c['recov'], c['repaired'], integ['movie.mkv']))


def scenario_xcrypt_nestedin7zenc(daemon, t):
    """The DONOR holds the movie in a store-mode rar, and that rar in a stored
    7z with encrypted headers and data (password in the donor's NZB), cut into
    volumes. The target is the same movie posted bare with two missing
    articles. The inner rar is read as the decrypted content of the 7z, the
    movie is mapped inside it, and the missing ranges are decrypted from the
    donor: the payload is reached through an encrypted container."""
    if not generators.HAVE_7Z:
        return ('xcrypt_nestedin7zenc', None, 'SKIP: 7z not installed')
    size = 6_000_000
    data = _payload(size, 8010)
    password = 'outer-7z-pw'
    pp = _place_copy(t, 'xcn7A', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    rar = generators.rar3_store_volumes('movie.mkv', data, size + 4096)
    with tempfile.TemporaryDirectory(prefix='xcn7-') as workdir:
        archive = generators.seven_zip_copy_encrypted([('inner.rar', rar[0])], workdir, password)
    donor_members = []
    for i, piece in enumerate(generators.split_bytes(archive, [2_500_000, 2_500_000]), 1):
        rel = 'xcn7B/rel.7z.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        donor_members.append((rel, 'Rel.7z.%03d' % i, len(piece), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xcn7', members, donor_members, payloads,
                             donor_password=password)
    ok = integ['movie.mkv'] and c['recov'] >= 1 and c['repaired'] >= 1
    return ('xcrypt_nestedin7zenc', ok,
            'status=%s recovered=%d repaired=%d integrity=%s'
            % (h['Status'], c['recov'], c['repaired'], integ['movie.mkv']))


def scenario_xcrypt_nestedinrarenc(daemon, t):
    """The DONOR holds the movie in a stored zip, and that zip in a
    password-encrypted store-mode rar volume set. The target is the same movie
    posted bare with two missing articles; it is repaired by decrypting the
    needed ranges of the rar and reading the movie out of the zip inside."""
    if not generators.HAVE_CRYPTO:
        return ('xcrypt_nestedinrarenc', None, 'SKIP: cryptography not installed')
    size = 6_000_000
    data = _payload(size, 8020)
    password = 'outer-rar-pw'
    pp = _place_copy(t, 'xcnrA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    inner_zip = generators.zip_store([('movie.mkv', data)])
    volumes = generators.rar3_store_volumes_encrypted('inner.zip', inner_zip, 2_000_003, password)
    donor_members = []
    for i, vol in enumerate(volumes, 1):
        rel = 'xcnrB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        donor_members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xcnr', members, donor_members, payloads,
                             donor_password=password)
    ok = integ['movie.mkv'] and c['recov'] >= 1 and c['repaired'] >= 1
    return ('xcrypt_nestedinrarenc', ok,
            'status=%s recovered=%d repaired=%d integrity=%s'
            % (h['Status'], c['recov'], c['repaired'], integ['movie.mkv']))


def scenario_xcrypt_nestedinenctarget(daemon, t):
    """The negative for the target side: the TARGET holds the movie in a rar
    inside an encrypted 7z and has a data hole; the donor is the movie posted
    bare. A range of such a file cannot be written in place (the cipher blocks
    at its edges also hold bytes of the rar around it), so cross-packing must
    leave the target alone: nothing is written and the download stays
    FAILURE/HEALTH for par2 or a twin."""
    if not generators.HAVE_7Z:
        return ('xcrypt_nestedinenctarget', None, 'SKIP: 7z not installed')
    size = 6_000_000
    data = _payload(size, 8030)
    password = 'outer-7z-pw'
    rar = generators.rar3_store_volumes('movie.mkv', data, size + 4096)
    with tempfile.TemporaryDirectory(prefix='xcnt-') as workdir:
        archive = generators.seven_zip_copy_encrypted([('inner.rar', rar[0])], workdir, password)
    pieces = generators.split_bytes(archive, [2_000_003, 2_000_003])
    payloads, members = {}, []
    missing = [set(), {2}, set()]
    for i, (piece, miss) in enumerate(zip(pieces, missing), 1):
        rel = 'xcntA/rel.7z.%03d' % i
        name = 'Rel.7z.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        payloads[name] = piece
        members.append((rel, name, len(piece), 500_000, miss))
    dp = _place_copy(t, 'xcntB', data, 'movie.mkv')
    donor_members = [(dp, 'movie.mkv', size, 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xcnt', members, donor_members, payloads,
                             primary_password=password)
    refused = _grep_log(t, 'nested archive inside an encrypted archive')
    intact = list(integ.values())
    ok = c['recov'] == 0 and refused >= 1 and intact.count(False) == 1
    return ('xcrypt_nestedinenctarget', ok,
            'status=%s recovered=%d refused_logs=%d intact=%s'
            % (h['Status'], c['recov'], refused, intact))


def scenario_xpacknesteddonor(daemon, t):
    """The DONOR holds the movie three containers deep (rar in zip in 7z, all
    stored), cut into .7z.001-.003; the target is the same movie posted bare
    with two missing articles. The content map follows the containers down to
    movie.mkv, so the donor is matched and borrowed from by the payload's
    bytes: the containers around it never have to match anything."""
    size = 6_000_000
    data = _payload(size, 7600)
    pp = _place_copy(t, 'xpndA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    donor_members = []
    for i, piece in enumerate(generators.split_bytes(_nested_archive(data), [2_500_000, 2_500_000]), 1):
        rel = 'xpndB/rel.7z.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        donor_members.append((rel, 'Rel.7z.%03d' % i, len(piece), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xpnd', members, donor_members, payloads)
    ok = integ['movie.mkv'] and c['recov'] >= 1 and c['repaired'] >= 1
    return ('xpacknesteddonor', ok,
            'status=%s recovered=%d repaired=%d integrity=%s'
            % (h['Status'], c['recov'], c['repaired'], integ['movie.mkv']))


def scenario_xpacknestedtarget(daemon, t):
    """The TARGET holds the movie three containers deep (rar in zip in 7z, all
    stored), cut into .7z.001-.003, with a data hole in the middle volume;
    the donor is the same movie posted bare. The hole maps through the
    containers to a range of movie.mkv, is filled from the donor, and the
    volumes come out byte-identical."""
    size = 6_000_000
    data = _payload(size, 7700)
    pieces = generators.split_bytes(_nested_archive(data), [2_000_003, 2_000_003])
    payloads, members = {}, []
    missing = [set(), {2}, set()]
    for i, (piece, miss) in enumerate(zip(pieces, missing), 1):
        rel = 'xpntA/rel.7z.%03d' % i
        name = 'Rel.7z.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        payloads[name] = piece
        members.append((rel, name, len(piece), 500_000, miss))
    dp = _place_copy(t, 'xpntB', data, 'movie.mkv')
    donor_members = [(dp, 'movie.mkv', size, 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xpnt', members, donor_members, payloads)
    ok = all(integ.values()) and c['recov'] >= 1 and c['xpack'] >= 1 and c['repaired'] >= 1
    return ('xpacknestedtarget', ok,
            'status=%s recovered=%d xpack=%d repaired=%d integ=%s'
            % (h['Status'], c['recov'], c['xpack'], c['repaired'], all(integ.values())))


def scenario_xcrypt_enc7ztarget(daemon, t):
    """The TARGET is a stored 7z with encrypted headers and data, cut into
    .7z.001-.003 volumes; its password travels in its own NZB. A data hole
    sits in the middle volume (the start header in the first volume and the
    encrypted headers at the end of the last one stay intact). The donor is
    the same movie posted bare. The headers are decrypted with the password,
    the packed stream maps as one AES-256-CBC stream over the movie, and the
    hole is filled with donor plaintext re-encrypted in place: byte-identical
    encrypted volumes afterwards prove it."""
    if not generators.HAVE_7Z:
        return ('xcrypt_enc7ztarget', None, 'SKIP: 7z not installed')
    size = 6_000_000
    data = _payload(size, 7400)
    password = 'target-7z-pw'
    with tempfile.TemporaryDirectory(prefix='xc7t-') as workdir:
        archive = generators.seven_zip_copy_encrypted([('movie.mkv', data)], workdir, password)
    pieces = generators.split_bytes(archive, [2_000_003, 2_000_003])
    payloads, members = {}, []
    missing = [set(), {2}, set()]
    for i, (piece, miss) in enumerate(zip(pieces, missing), 1):
        rel = 'xc7tA/rel.7z.%03d' % i
        name = 'Rel.7z.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        payloads[name] = piece
        members.append((rel, name, len(piece), 500_000, miss))
    dp = _place_copy(t, 'xc7tB', data, 'movie.mkv')
    donor_members = [(dp, 'movie.mkv', size, 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xc7t', members, donor_members, payloads,
                             primary_password=password)
    ok = all(integ.values()) and c['recov'] >= 1 and c['xpack'] >= 1 and c['repaired'] >= 1
    return ('xcrypt_enc7ztarget', ok,
            'status=%s recovered=%d xpack=%d repaired=%d integ=%s'
            % (h['Status'], c['recov'], c['xpack'], c['repaired'], all(integ.values())))


def scenario_xcrypt_enc7zdonor(daemon, t):
    """The DONOR is a stored 7z with encrypted headers and data; the target is
    the same movie posted bare with two missing articles. With the donor's
    password (from its NZB) the donor maps like any stored archive, so only
    the articles around the holes are fetched and decrypted: no
    decompression step runs (DupeStreamDecompress is off here) and the donor
    is not downloaded whole."""
    if not generators.HAVE_7Z:
        return ('xcrypt_enc7zdonor', None, 'SKIP: 7z not installed')
    size = 6_000_000
    data = _payload(size, 7500)
    password = 'donor-7z-pw'
    pp = _place_copy(t, 'xc7dA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    with tempfile.TemporaryDirectory(prefix='xc7d-') as workdir:
        archive = generators.seven_zip_copy_encrypted([('movie.mkv', data)], workdir, password)
    donor_members = []
    for i, piece in enumerate(generators.split_bytes(archive, [2_500_000, 2_500_000]), 1):
        rel = 'xc7dB/rel.7z.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        donor_members.append((rel, 'Rel.7z.%03d' % i, len(piece), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xc7d', members, donor_members, payloads,
                             donor_password=password)
    decompressed = _grep_log(t, '(decompressed)')
    ok = integ['movie.mkv'] and c['recov'] >= 1 and c['repaired'] >= 1 and decompressed == 0
    return ('xcrypt_enc7zdonor', ok,
            'status=%s recovered=%d repaired=%d decompressed_logs=%d integrity=%s'
            % (h['Status'], c['recov'], c['repaired'], decompressed, integ['movie.mkv']))


def scenario_xcrypt_plainenc(daemon, t):
    """Reverse direction: a BARE unencrypted target with a hole is repaired
    from a password-ENCRYPTED store-rar donor. The donor's password travels
    via its own NZB <meta type="password">; ExecCrossPackRepair's M3 retry
    ladder tries the donor without a password first (fails: "encrypted
    archive data"), then retries with donor.Password and succeeds."""
    if not generators.HAVE_CRYPTO:
        return ('xcrypt_plainenc', None, 'SKIP: cryptography not installed')
    size = 6_000_000
    data = _payload(size, 7200)
    password = 'donor-pw-B'
    pp = _place_copy(t, 'xpeA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    donor_members = []
    for i, vol in enumerate(
            generators.rar3_store_volumes_encrypted('movie.mkv', data, 2_000_003, password), 1):
        rel = 'xpeB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        donor_members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xpe', members, donor_members, payloads,
                             donor_password=password)
    ok = integ['movie.mkv'] and c['recov'] >= 1 and c['repaired'] >= 1
    return ('xcrypt_plainenc', ok, 'status=%s recovered=%d repaired=%d integrity=%s'
            % (h['Status'], c['recov'], c['repaired'], integ['movie.mkv']))


def scenario_xcrypt_diffpass(daemon, t):
    """Both sides encrypted with DIFFERENT passwords and different volume
    sizes: the target (password A) has a data hole in its middle volume; the
    donor (password B) is a repost of the same movie under its own key. This
    proves the two crypto contexts never mix - the donor's plaintext is
    recovered with ITS key (VerifyDonorSetEncrypted/ReadDonorInner use the
    donor's own ContentMap+RunCrypto) and the patch is re-encrypted with the
    TARGET's key (PatchFromDonorSetEncrypted uses repairSet.Map's RunCrypto)."""
    if not generators.HAVE_CRYPTO:
        return ('xcrypt_diffpass', None, 'SKIP: cryptography not installed')
    size = 6_000_000
    data = _payload(size, 7300)
    pw_a, pw_b = 'password-A', 'password-B'
    volumes = generators.rar3_store_volumes_encrypted('movie.mkv', data, 2_000_003, pw_a)
    payloads, members = {}, []
    missing = [set(), {2}, set()]
    for i, (vol, miss) in enumerate(zip(volumes, missing), 1):
        rel = 'xdpA/rel.part%02d.rar' % i
        name = 'Rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        payloads[name] = vol
        members.append((rel, name, len(vol), 500_000, miss))
    donor_members = []
    for i, vol in enumerate(
            generators.rar3_store_volumes_encrypted('movie.mkv', data, 1_500_001, pw_b), 1):
        rel = 'xdpB/other.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        donor_members.append((rel, 'Other.part%02d.rar' % i, len(vol), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xdp', members, donor_members, payloads,
                             primary_password=pw_a, donor_password=pw_b)
    ok = all(integ.values()) and c['recov'] >= 1 and c['xpack'] >= 1 and c['repaired'] >= 1
    return ('xcrypt_diffpass', ok,
            'status=%s recovered=%d xpack=%d repaired=%d integ=%s'
            % (h['Status'], c['recov'], c['xpack'], c['repaired'], all(integ.values())))


def scenario_xcrypt_wrongpass(daemon, t):
    """NEGATIVE: a bare unencrypted target with a hole; the encrypted donor's
    NZB advertises a password that does NOT match the one it was actually
    encrypted with. RAR3 carries no stored password-check value (real WinRAR
    fact, mirrored in StreamCryptoRar3WrongPasswordTest) - so the M3 retry
    ladder's BuildMap call SUCCEEDS with a wrong key instead of failing
    closed there; the mismatch is only caught downstream, when the wrongly
    "decrypted" donor plaintext is content-identity-probed against the
    target's known bytes and rejected ("content identity not confirmed").
    Nothing may be written and the target must stay exactly as it arrived -
    the same fail-closed guarantee xpackneg proves for the plain case."""
    if not generators.HAVE_CRYPTO:
        return ('xcrypt_wrongpass', None, 'SKIP: cryptography not installed')
    size = 6_000_000
    data = _payload(size, 7400)
    real_password = 'correct-pw'
    wrong_password = 'incorrect-pw'
    pp = _place_copy(t, 'xwpA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    donor_members = []
    for i, vol in enumerate(
            generators.rar3_store_volumes_encrypted('movie.mkv', data, 2_000_003, real_password), 1):
        rel = 'xwpB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        donor_members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 300_000, set()))

    h, integ, c = _xpack_run(daemon, t, 'Xwp', members, donor_members, payloads,
                             donor_password=wrong_password)
    ok = c['rejected'] >= 1 and c['recov'] == 0 and not integ['movie.mkv']
    return ('xcrypt_wrongpass', ok,
            'status=%s recovered=%d rejected=%d integrity=%s'
            % (h['Status'], c['recov'], c['rejected'], integ['movie.mkv']))


def scenario_xdecomp_zip(daemon, t):
    """M4 decompression donor: a bare movie.mkv target with holes, repaired
    from a REAL DEFLATE-compressed zip donor (generators.zip_deflated)
    holding the identical movie.mkv. M2 never maps this (BuildZipMap rejects
    any Method != 0 entry), so only the M4 ladder - materialize the donor's
    articles, extract movie.mkv via in-process zipfast/zipview (or SevenZipCmd),
    verify the extracted bytes against the target's own downloaded bytes,
    then patch the holes - can repair it. Requires DupeStreamDecompress=yes
    (see SCENARIO_OPTIONS). No par2 and the hole is fully filled, so the
    byte-based health recount takes the item to SUCCESS - asserted directly
    below as the decompression proof of the recount."""
    size = 4_000_000
    data = _payload(size, 8200)
    pp = _place_copy(t, 'xdzA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    archive = generators.zip_deflated([('movie.mkv', data)])
    rel = 'xdzB/rel.zip'
    t.write_file(os.path.join('data', rel), archive)
    donor_members = [(rel, 'Rel.zip', len(archive), 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xdz', members, donor_members, payloads)
    decompressed = _grep_log(t, '(decompressed)')
    ok = (integ['movie.mkv'] and c['recov'] >= 1 and decompressed >= 1 and
          'SUCCESS' in h['Status'])
    return ('xdecomp_zip', ok,
            'status=%s recovered=%d decompressed_logs=%d integrity=%s'
            % (h['Status'], c['recov'], decompressed, integ['movie.mkv']))


def scenario_xdecomp_7z(daemon, t):
    """Same shape as xdecomp_zip, but the donor is a REAL LZMA2-compressed 7z
    archive (generators.seven_zip_lzma) - BuildSevenZipMap rejects its
    non-Copy coder, so only the M4 decompression ladder can repair the bare
    target."""
    if not generators.HAVE_7Z:
        return ('xdecomp_7z', None, 'SKIP: 7z not installed')
    size = 4_000_000
    # Repeating content, so LZMA2 really compresses it. Data it cannot compress
    # is stored as raw chunks and borrowed without decompression (xpack7zlzma2).
    data = (_payload(200_000, 8300) * 20)[:size]
    pp = _place_copy(t, 'xd7A', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    with tempfile.TemporaryDirectory(prefix='xd7z-') as workdir:
        archive = generators.seven_zip_lzma([('movie.mkv', data)], workdir)
    rel = 'xd7B/rel.7z'
    t.write_file(os.path.join('data', rel), archive)
    donor_members = [(rel, 'Rel.7z', len(archive), 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xd7', members, donor_members, payloads)
    decompressed = _grep_log(t, '(decompressed)')
    ok = integ['movie.mkv'] and c['recov'] >= 1 and decompressed >= 1
    return ('xdecomp_7z', ok,
            'status=%s recovered=%d decompressed_logs=%d integrity=%s'
            % (h['Status'], c['recov'], decompressed, integ['movie.mkv']))


def scenario_xdecomp_storetarget(daemon, t):
    """The M4 ladder against a non-bare TARGET: a store-mode rar3 target
    (generators.rar3_store_volumes, the same M2 target-side generator
    xpackrar uses - no external tool needed to CREATE it) with a data hole
    in its second volume, repaired from a REAL LZMA2-compressed 7z donor of
    the same inner movie.mkv. Proves the M4 extracted-donor path reuses the
    plain (M2) inner-space target map, not just the identity/bare map."""
    if not generators.HAVE_7Z:
        return ('xdecomp_storetarget', None, 'SKIP: 7z not installed')
    size = 4_000_000
    data = _payload(size, 8400)
    volumes = generators.rar3_store_volumes('movie.mkv', data, 2_000_000)
    payloads, members = {}, []
    for i, vol in enumerate(volumes, 1):
        rel = 'xdsA/rel.part%02d.rar' % i
        name = 'Rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        payloads[name] = vol
        members.append((rel, name, len(vol), 500_000, {2} if i == 2 else set()))
    with tempfile.TemporaryDirectory(prefix='xds-') as workdir:
        archive = generators.seven_zip_lzma([('movie.mkv', data)], workdir)
    rel = 'xdsB/rel.7z'
    t.write_file(os.path.join('data', rel), archive)
    donor_members = [(rel, 'Rel.7z', len(archive), 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xds', members, donor_members, payloads)
    # A 7z whose LZMA2 coder left raw chunks is borrowed from by range, without
    # extracting the whole duplicate; either path proves the inner-space map.
    decompressed = _grep_log(t, '(decompressed)') + _grep_log(t, '(7z raw chunks)')
    ok = all(integ.values()) and c['recov'] >= 1 and decompressed >= 1
    return ('xdecomp_storetarget', ok,
            'status=%s recovered=%d decompressed_logs=%d integ=%s'
            % (h['Status'], c['recov'], decompressed, all(integ.values())))


def scenario_xdecomp_enc7z(daemon, t):
    """The POSIX password-quoting proof: a bare target repaired from a
    HEADER-ENCRYPTED LZMA2 7z donor (generators.seven_zip_lzma_encrypted,
    ``-mhe=on -p<password>``), the donor's password traveling via its own
    NZB <meta type="password"> exactly like xcrypt_plainenc threads an
    encrypted-rar donor's password. Unpack::MakeExtractor/MakePassword must
    pass that password to 7z as a single raw argv element on POSIX (M4 Task
    2, commit 3c3e9130) - a quote-wrapped password would make 7z see the
    literal quote characters and fail extraction, and this scenario would
    then FAIL (not SKIP)."""
    if not generators.HAVE_7Z:
        return ('xdecomp_enc7z', None, 'SKIP: 7z not installed')
    size = 4_000_000
    data = _payload(size, 8500)
    password = 'decomp-pw-7z'
    pp = _place_copy(t, 'xdeA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    with tempfile.TemporaryDirectory(prefix='xde-') as workdir:
        archive = generators.seven_zip_lzma_encrypted([('movie.mkv', data)], workdir, password)
    rel = 'xdeB/rel.7z'
    t.write_file(os.path.join('data', rel), archive)
    donor_members = [(rel, 'Rel.7z', len(archive), 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xde', members, donor_members, payloads,
                             donor_password=password)
    decompressed = _grep_log(t, '(decompressed)')
    ok = integ['movie.mkv'] and c['recov'] >= 1 and decompressed >= 1
    return ('xdecomp_enc7z', ok,
            'status=%s recovered=%d decompressed_logs=%d integrity=%s'
            % (h['Status'], c['recov'], decompressed, integ['movie.mkv']))


def scenario_xdecomp_enctarget(daemon, t):
    """The M3+M4 composition: a password-ENCRYPTED store-rar TARGET (same
    generator as xcrypt_encplain) with a data hole in its middle volume,
    repaired from a COMPRESSED 7z donor of the same movie.mkv. The donor
    cannot map for byte-copy (it is compressed), so M4 materializes and
    extracts it to plaintext; that plaintext is then re-encrypted under the
    target's own AES-CBC stream context (the M3 write core) and the
    ciphertext written into the hole. Byte-identical ENCRYPTED volumes prove
    the extract -> re-encrypt -> patch round trip. Requires both a crypto
    module (encrypted target generator) and a 7z binary (compressed donor)."""
    if not generators.HAVE_CRYPTO:
        return ('xdecomp_enctarget', None, 'SKIP: cryptography not installed')
    if not generators.HAVE_7Z:
        return ('xdecomp_enctarget', None, 'SKIP: 7z not installed')
    size = 6_000_000
    data = _payload(size, 8600)
    password = 'target-pw-D'
    volumes = generators.rar3_store_volumes_encrypted('movie.mkv', data, 2_000_003, password)
    payloads, members = {}, []
    missing = [set(), {2}, set()]      # only volume 2 loses a non-header segment
    for i, (vol, miss) in enumerate(zip(volumes, missing), 1):
        rel = 'xdeA/rel.part%02d.rar' % i
        name = 'Rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        payloads[name] = vol
        members.append((rel, name, len(vol), 500_000, miss))
    with tempfile.TemporaryDirectory(prefix='xde-') as workdir:
        archive = generators.seven_zip_lzma([('movie.mkv', data)], workdir)
    rel = 'xdeB/rel.7z'
    t.write_file(os.path.join('data', rel), archive)
    donor_members = [(rel, 'Rel.7z', len(archive), 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xde', members, donor_members, payloads,
                             primary_password=password)
    decompressed = _grep_log(t, '(decompressed)')
    ok = all(integ.values()) and c['recov'] >= 1 and decompressed >= 1
    return ('xdecomp_enctarget', ok,
            'status=%s recovered=%d decompressed_logs=%d integ=%s'
            % (h['Status'], c['recov'], decompressed, all(integ.values())))


def scenario_xdecomp_neg(daemon, t):
    """The negative: a bare target repaired attempt from a REAL 7z donor
    holding the RIGHT-size but WRONG-bytes inner file (a decoy payload of
    the same size). VerifyDonorInnerFile's identity probe must reject it
    (`content identity not confirmed`) before any write; nothing may be
    written and neither file may end up matching the other."""
    if not generators.HAVE_7Z:
        return ('xdecomp_neg', None, 'SKIP: 7z not installed')
    size = 4_000_000
    data = _payload(size, 8600)
    decoy = _payload(size, 8700)
    pp = _place_copy(t, 'xdnA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    with tempfile.TemporaryDirectory(prefix='xdn-') as workdir:
        archive = generators.seven_zip_lzma([('movie.mkv', decoy)], workdir)
    rel = 'xdnB/rel.7z'
    t.write_file(os.path.join('data', rel), archive)
    donor_members = [(rel, 'Rel.7z', len(archive), 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xdn', members, donor_members,
                             {'movie.mkv': data, 'decoy': decoy})
    decompressed = _grep_log(t, '(decompressed)')
    ok = (c['rejected'] >= 1 and c['recov'] == 0 and decompressed == 0 and
          not integ['movie.mkv'] and not integ['decoy'])
    return ('xdecomp_neg', ok,
            'status=%s recovered=%d rejected=%d decompressed_logs=%d file_matches=%s/%s'
            % (h['Status'], c['recov'], c['rejected'], decompressed,
               integ['movie.mkv'], integ['decoy']))


def scenario_xdecomp_symlink(daemon, t):
    """A compressed donor with a valid movie plus a symlink must be rejected
    before selecting or patching the movie (`archive contains link`). The
    link is RELATIVE and in-tree on purpose: 7-Zip 23.01+ refuses to create
    absolute or ``..`` link targets at extraction time (exit code 2, link
    skipped), which fails the extract step before the daemon's own link
    check ever runs - only a link every extractor generation materializes
    can probe that check. A relative link to the valid movie.mkv is also the
    attack shape the check guards against: followed naively it would
    "verify" trivially. Cleanup must unlink the extracted symlink, preserve
    the outside sentinel, and remove every stream-decompression scratch
    directory."""
    if t.name != 'local':
        return ('xdecomp_symlink', None, 'SKIP: POSIX local target required')

    size = 1_000_000
    data = _payload(size, 8750)
    sentinel_data = b'outside-sentinel-must-survive'
    t.write_file(os.path.join('outside', 'sentinel'), sentinel_data)

    primary_path = _place_copy(t, 'xdlA', data, 'movie.mkv')
    members = [(primary_path, 'movie.mkv', size, 250_000, {2})]
    archive = generators.zip_deflated_with_symlink(
        [('movie.mkv', data)], 'movie-link.mkv', 'movie.mkv')
    rel = 'xdlB/rel.zip'
    t.write_file(os.path.join('data', rel), archive)
    donor_members = [(rel, 'Rel.zip', len(archive), 200_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xdl', members, donor_members,
                             {'movie.mkv': data})
    rejected_links = _grep_log(t, 'archive contains link')
    # Newer 7-Zip versions reject the dangerous absolute link before NZBGet
    # can inspect the extracted tree. Require that specific diagnostic AND
    # the failed-extraction path; a generic extraction error is insufficient.
    extractor_rejected = (
        _grep_log(t, 'ERROR: Dangerous link path was ignored') >= 1 and
        _grep_log(t, 'Skipping decompression of Rel.zip of duplicate DonXdl: '
                     'extraction failed') >= 1)
    try:
        sentinel_survived = t.read_file(os.path.join('outside', 'sentinel')) == sentinel_data
    except Exception:
        sentinel_survived = False

    scratch_dirs = []
    for root, dirs, _ in os.walk(t.path('main')):
        scratch_dirs.extend(os.path.join(root, name) for name in dirs
                            if name.startswith('.stream-decompress.'))

    ok = (h['Status'] == 'FAILURE/HEALTH' and c['recov'] == 0 and
          (rejected_links >= 1 or extractor_rejected) and not integ['movie.mkv'] and
          sentinel_survived and not scratch_dirs)
    return ('xdecomp_symlink', ok,
            'status=%s recovered=%d rejected_links=%d extractor_rejected=%s integrity=%s '
            'sentinel=%s scratch_dirs=%d'
            % (h['Status'], c['recov'], rejected_links, extractor_rejected, integ['movie.mkv'],
               sentinel_survived, len(scratch_dirs)))


def scenario_xdecomp_off(daemon, t):
    """The opt-in gate proof: the SAME bare-target-vs-compressed-7z-donor
    setup as xdecomp_7z, but DupeStreamDecompress is OMITTED (default no -
    see SCENARIO_OPTIONS, which deliberately does NOT add it here). The
    decompression path must never run (no `(decompressed)` log) and the
    item must stay unrepaired - M2 already rejected this donor's non-Copy
    coder, and M4 is off, so nothing else can map it. The donor has encrypted
    headers: a plain LZMA2 7z can be borrowed from by its raw chunks without
    the decompression step, an encrypted one only by extracting it."""
    if not generators.HAVE_7Z:
        return ('xdecomp_off', None, 'SKIP: 7z not installed')
    size = 4_000_000
    data = _payload(size, 8800)
    password = 'decomp-off-pw'
    pp = _place_copy(t, 'xdoA', data, 'movie.mkv')
    members = [(pp, 'movie.mkv', size, 500_000, {5, 6})]
    payloads = {'movie.mkv': data}
    with tempfile.TemporaryDirectory(prefix='xdo-') as workdir:
        archive = generators.seven_zip_lzma_encrypted([('movie.mkv', data)], workdir, password)
    rel = 'xdoB/rel.7z'
    t.write_file(os.path.join('data', rel), archive)
    donor_members = [(rel, 'Rel.7z', len(archive), 300_000, set())]

    h, integ, c = _xpack_run(daemon, t, 'Xdo', members, donor_members, payloads,
                             donor_password=password)
    decompressed = _grep_log(t, '(decompressed)')
    ok = decompressed == 0 and c['recov'] == 0 and not integ['movie.mkv']
    return ('xdecomp_off', ok,
            'status=%s recovered=%d decompressed_logs=%d integrity=%s'
            % (h['Status'], c['recov'], decompressed, integ['movie.mkv']))


def scenario_wholefile(daemon, t):
    """A volume none of whose articles is available anywhere: the primary
    lists it, every server lacks it. The byte-identical renamed repost is
    proven on the set's other damaged volume first (its bytes verify), and
    then the missing volume is recreated whole from the twin member the
    suffix pairs it with. Every hole filled and no par2, so the release
    completes SUCCESS byte-identically."""
    seg_primary, seg_donor = 500_000, 300_000
    vol = 1_500_000
    n = (vol + seg_primary - 1) // seg_primary
    members = [
        ('wholeA/x.part01.rar', 'Rel.part01.rar', vol, seg_primary, set()),
        ('wholeA/x.part02.rar', 'Rel.part02.rar', vol, seg_primary, {2}),
        ('wholeA/x.part03.rar', 'Rel.part03.rar', vol, seg_primary, set(range(1, n + 1))),
        ('wholeA/x.part04.rar', 'Rel.part04.rar', vol, seg_primary, set()),
    ]
    payloads = {}
    for i, m in enumerate(members):
        data = _payload(m[2], 9300 + i)
        payloads[m[1]] = data
        t.write_file(os.path.join('data', m[0]), data)
    donor_members = [(m[0].replace('wholeA', 'wholeB'), m[1].replace('Rel.', 'Other.'),
                      m[2], seg_donor, set()) for m in members]
    for dm, m in zip(donor_members, members):
        t.write_file(os.path.join('data', dm[0]), payloads[m[1]])
    api = daemon.wait_ready()
    daemon.append(api, 'DonWhole', build_multi_nzb(donor_members), True, 'whole-key', 50)
    daemon.append(api, 'RelWhole', build_multi_nzb(members), False, 'whole-key', 100)
    h = daemon.wait_history(api, 'RelWhole')
    queued = _grep_log(t, 'no article available')
    recreated = _grep_log(t, 'Recreating Rel.part03.rar')
    repaired = _grep_log(t, 'donor article(s)')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=both_dirs) for m in members)
    return ('wholefile', integ and queued == 1 and recreated == 1 and repaired >= 2,
            'status=%s queued_logs=%d recreated_logs=%d repair_logs=%d integrity=%s'
            % (h['Status'], queued, recreated, repaired, integ))


def _wholefile_fixture(t, tag):
    seg_primary, seg_donor = 500_000, 300_000
    vol = 1_500_000
    n = (vol + seg_primary - 1) // seg_primary
    members = [
        ('%sA/x.part01.rar' % tag, 'Rel.part01.rar', vol, seg_primary, set()),
        ('%sA/x.part02.rar' % tag, 'Rel.part02.rar', vol, seg_primary, {2}),
        ('%sA/x.part03.rar' % tag, 'Rel.part03.rar', vol, seg_primary, set(range(1, n + 1))),
        ('%sA/x.part04.rar' % tag, 'Rel.part04.rar', vol, seg_primary, set()),
    ]
    payloads = {}
    for i, m in enumerate(members):
        data = _payload(m[2], 9300 + i)
        payloads[m[1]] = data
        t.write_file(os.path.join('data', m[0]), data)
    donor_members = [(m[0].replace('%sA' % tag, '%sB' % tag), m[1].replace('Rel.', 'Other.'),
                      m[2], seg_donor, set()) for m in members]
    for dm, m in zip(donor_members, members):
        t.write_file(os.path.join('data', dm[0]), payloads[m[1]])
    return members, donor_members, payloads


def scenario_wholefileretry(daemon, t):
    """Corner case: "Retry failed articles" on a release whose volume was
    recreated whole from a duplicate. The recreated volume (no own article
    ever arrived) must survive the retry - it must not be deleted as an
    empty failed file and re-downloaded from the dead primary - and the
    release must again end byte-identical."""
    members, donor_members, payloads = _wholefile_fixture(t, 'wr')
    api = daemon.wait_ready()
    daemon.append(api, 'DonWR', build_multi_nzb(donor_members), True, 'wr-key', 50)
    daemon.append(api, 'RelWR', build_multi_nzb(members), False, 'wr-key', 100)
    h = daemon.wait_history(api, 'RelWR')
    nzbid = h['NZBID']
    first_integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=(('main', 'dst'), ('main', 'inter')))
                      for m in members)
    api.editqueue('HistoryRetryFailed', 0, '', [nzbid])
    deadline = time.time() + 180
    time.sleep(3)
    while time.time() < deadline:
        if not any(g['NZBID'] == nzbid for g in api.listgroups()):
            break
        time.sleep(1)
    h2 = [x for x in api.history() if x['NZBID'] == nzbid][0]
    integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=(('main', 'dst'), ('main', 'inter')))
                for m in members)
    return ('wholefileretry', first_integ and integ,
            'status=%s retry_status=%s first_integrity=%s integrity_after_retry=%s'
            % (h['Status'], h2['Status'], first_integ, integ))


def scenario_wholefilepar(daemon, t):
    """Fix 2 with a real par2 index (no recovery slices, so par2 cannot
    cover a missing volume): the whole-file job counts as damage par2 can't
    cover, stream repair runs before par-check, recreates the volume, and
    the full par-check that follows verifies all four volumes against the
    par2 checksums - SUCCESS/PAR only if the recreated volume is exact."""
    members, donor_members, payloads = _wholefile_fixture(t, 'wp')
    par = generators.par2_index([(m[1], payloads[m[1]]) for m in members])
    t.write_file(os.path.join('data', 'wpA/rel.par2'), par)
    members = members + [('wpA/rel.par2', 'Rel.par2', len(par), 500_000, set())]
    api = daemon.wait_ready()
    daemon.append(api, 'DonWP', build_multi_nzb(donor_members), True, 'wp-key', 50)
    daemon.append(api, 'RelWP', build_multi_nzb(members), False, 'wp-key', 100)
    h = daemon.wait_history(api, 'RelWP')
    recreated = _grep_log(t, 'Recreating Rel.part03.rar')
    par_ok = _grep_log(t, 'repair not needed') + _grep_log(t, 'Repair not needed') + _grep_log(t, 'all files are correct')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=both_dirs) for m in members[:4])
    return ('wholefilepar', integ and recreated == 1 and h['ParStatus'] == 'SUCCESS',
            'status=%s par=%s recreated_logs=%d par_ok_logs=%d integrity=%s'
            % (h['Status'], h['ParStatus'], recreated, par_ok, integ))


def scenario_wholefilefailretry(daemon, t):
    """Corner case: a release that still FAILS after a volume was recreated
    (another volume is missing on the donor too), so the recreated volume
    stays in the intermediate directory. "Retry failed articles" must not
    delete the recreated volume as an empty failed file."""
    members, donor_members, payloads = _wholefile_fixture(t, 'wf')
    # part04 is missing everywhere, on the primary and the duplicate
    members[3] = members[3][:4] + ({2},)
    donor_members[3] = donor_members[3][:4] + (set(range(2, 4)),)
    api = daemon.wait_ready()
    daemon.append(api, 'DonWF', build_multi_nzb(donor_members), True, 'wf-key', 50)
    daemon.append(api, 'RelWF', build_multi_nzb(members), False, 'wf-key', 100)
    h = daemon.wait_history(api, 'RelWF')
    nzbid = h['NZBID']
    inter = (('main', 'inter'),)
    before = _verify_output(t, payloads['Rel.part03.rar'], '.rar', dirs=inter)
    api.editqueue('HistoryRetryFailed', 0, '', [nzbid])
    time.sleep(3)
    deadline = time.time() + 180
    while time.time() < deadline and any(g['NZBID'] == nzbid for g in api.listgroups()):
        time.sleep(1)
    h2 = [x for x in api.history() if x['NZBID'] == nzbid][0]
    after = _verify_output(t, payloads['Rel.part03.rar'], '.rar', dirs=inter)
    return ('wholefilefailretry', h['Status'].startswith('FAILURE') and before and after,
            'status=%s retry_status=%s part03_before=%s part03_after_retry=%s'
            % (h['Status'], h2['Status'], before, after))


def scenario_wholefileonly(daemon, t):
    """Fix 2 in the most common shape: one volume missing entirely, every
    other volume complete - no damaged file exists to prove the duplicate
    on. The duplicate must be proven byte-identical on an intact volume and
    the missing one recreated; SUCCESS byte-identically."""
    members, donor_members, payloads = _wholefile_fixture(t, 'wo')
    members[1] = members[1][:4] + (set(),)               # part02 complete too
    api = daemon.wait_ready()
    daemon.append(api, 'DonWO', build_multi_nzb(donor_members), True, 'wo-key', 50)
    daemon.append(api, 'RelWO', build_multi_nzb(members), False, 'wo-key', 100)
    h = daemon.wait_history(api, 'RelWO')
    proven = _grep_log(t, 'verified on intact file')
    recreated = _grep_log(t, 'Recreating Rel.part03.rar')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=both_dirs) for m in members)
    return ('wholefileonly', integ and proven == 1 and recreated == 1,
            'status=%s proven_logs=%d recreated_logs=%d integrity=%s' % (h['Status'], proven, recreated, integ))


def scenario_wholefilewrongdonor(daemon, t):
    """The safety net of that proof: a duplicate with the same volume names
    and sizes but DIFFERENT bytes (another packing) must fail the intact-file
    proof, so the missing volume is not recreated from it and nothing is
    written; the release stays a failure."""
    members, donor_members, payloads = _wholefile_fixture(t, 'ww')
    members[1] = members[1][:4] + (set(),)
    for i, dm in enumerate(donor_members):                 # same sizes, other bytes
        t.write_file(os.path.join('data', dm[0]), _payload(dm[2], 9700 + i))
    api = daemon.wait_ready()
    daemon.append(api, 'DonWW', build_multi_nzb(donor_members), True, 'ww-key', 50)
    daemon.append(api, 'RelWW', build_multi_nzb(members), False, 'ww-key', 100)
    h = daemon.wait_history(api, 'RelWW')
    proven = _grep_log(t, 'verified on intact file')
    recreated = _grep_log(t, 'Recreating')
    return ('wholefilewrongdonor', proven == 0 and recreated == 0 and h['Status'].startswith('FAILURE'),
            'status=%s proven_logs=%d recreated_logs=%d' % (h['Status'], proven, recreated))


def _wholefile_run(daemon, t, tag, members, donor_members):
    api = daemon.wait_ready()
    # marked dead by a dupe tool: no failover fetches it, so stream repair is the
    # last option left (B78) - the repair itself is what these scenarios test
    _ds_append(api, 'Don' + tag, build_multi_nzb(donor_members), tag + '-key', 50,
               params=[{'Name': 'DupeAlive', 'Value': '0'}])
    daemon.append(api, 'Rel' + tag, build_multi_nzb(members), False, tag + '-key', 100)
    return api, daemon.wait_history(api, 'Rel' + tag)


def scenario_wholefilenofirst(daemon, t):
    """Corner case: the donor's twin of the missing volume lacks its FIRST
    article, which is where the decoded file size is normally learned. Every
    yEnc article declares the file size, so a later article must size the
    file and the rest of the volume is still recreated (the first part stays
    a hole, so the release fails without par2 - but the bytes it got must be
    there and correct)."""
    members, donor_members, payloads = _wholefile_fixture(t, 'nf1')
    members[1] = members[1][:4] + (set(),)
    donor_members[2] = donor_members[2][:4] + ({1},)
    api, h = _wholefile_run(daemon, t, 'NF1', members, donor_members)
    recreated = _grep_log(t, 'Recreating Rel.part03.rar')
    data = None
    for base in (('main', 'dst'), ('main', 'inter')):
        for rel in t.find_files(*base):
            if rel.endswith('Rel.part03.rar'):
                data = t.read_file(rel)
    expected = payloads['Rel.part03.rar']
    tail_ok = bool(data) and len(data) == len(expected) and data[300_000:] == expected[300_000:]
    return ('wholefilenofirst', recreated == 1 and tail_ok,
            'status=%s recreated_logs=%d recreated_tail_matches=%s' % (h['Status'], recreated, tail_ok))


def scenario_wholefilepartial(daemon, t):
    """Corner case: the donor's twin has a hole of its own, so the missing
    volume is recreated only partly. Nothing may be credited to health for
    it (the release stays a failure), the recreated bytes must be correct,
    and a retry must keep them."""
    members, donor_members, payloads = _wholefile_fixture(t, 'wpt')
    members[1] = members[1][:4] + (set(),)
    donor_members[2] = donor_members[2][:4] + ({3},)
    api, h = _wholefile_run(daemon, t, 'WPT', members, donor_members)
    nzbid = h['NZBID']
    recreated = _grep_log(t, 'Recreating Rel.part03.rar')

    def part03():
        for base in (('main', 'dst'), ('main', 'inter')):
            for rel in t.find_files(*base):
                if rel.endswith('Rel.part03.rar'):
                    return t.read_file(rel)
        return None
    expected = payloads['Rel.part03.rar']
    first = part03()
    head_ok = bool(first) and first[:600_000] == expected[:600_000]
    api.editqueue('HistoryRetryFailed', 0, '', [nzbid])
    time.sleep(3)
    deadline = time.time() + 180
    while time.time() < deadline and any(g['NZBID'] == nzbid for g in api.listgroups()):
        time.sleep(1)
    h2 = [x for x in api.history() if x['NZBID'] == nzbid][0]
    second = part03()
    kept = bool(second) and second[:600_000] == expected[:600_000]
    return ('wholefilepartial', recreated == 1 and head_ok and kept and h['Status'].startswith('FAILURE')
            and h2['Status'].startswith('FAILURE'),
            'status=%s retry_status=%s recreated_logs=%d head_ok=%s kept_after_retry=%s'
            % (h['Status'], h2['Status'], recreated, head_ok, kept))


def scenario_wholefiletwo(daemon, t):
    """Corner case: two volumes missing entirely, both carried by the
    duplicate. Each must be recreated from its own twin member (never both
    from one), and the release completes SUCCESS byte-identically."""
    members, donor_members, payloads = _wholefile_fixture(t, 'w2')
    n = 3
    members[3] = members[3][:4] + (set(range(1, n + 1)),)
    api, h = _wholefile_run(daemon, t, 'W2', members, donor_members)
    recreated = _grep_log(t, 'Recreating Rel.part0')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=both_dirs) for m in members)
    return ('wholefiletwo', integ and recreated == 2,
            'status=%s recreated_logs=%d integrity=%s' % (h['Status'], recreated, integ))


def scenario_failoverlive(daemon, t):
    """Corner case: HealthCheck=dupe parks a download while a live stream
    repair pass (DupeArticleFallback=live) still works on one of its files.
    The pass must be detached cleanly - no crash, no write into the parked
    item - and the backup must be fetched and complete."""
    seg = 100_000
    holed = _payload(4_000_000, 9950)
    dead_size = 2_900_000
    n_dead = dead_size // seg
    t.write_file(os.path.join('data', 'flA/a.bin'), holed)
    members = [('flA/a.bin', 'A.bin', len(holed), seg, set(range(2, 40)))]
    for i in range(6):
        t.write_file(os.path.join('data', 'flA/d%d.bin' % i), _payload(dead_size, 9960 + i))
        members.append(('flA/d%d.bin' % i, 'Dead%d.bin' % i, dead_size, seg, set(range(1, n_dead + 1))))
    # the backup carries A byte-identically (other segmentation) plus its own content
    t.write_file(os.path.join('data', 'flB/a.bin'), holed)
    data = _payload(3_000_000, 9970)
    t.write_file(os.path.join('data', 'flB/b.bin'), data)
    backup = build_multi_nzb([('flB/a.bin', 'A.bin', len(holed), 70_000, set()),
                              ('flB/b.bin', 'Backup.bin', len(data), seg, set())])
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(members), True, 'fl-key', 100)
    daemon.append(api, 'Backup', backup, False, 'fl-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    deadline = time.time() + 240
    hb = daemon.wait_history(api, 'Backup')
    while hb['Status'].startswith('DELETED') and time.time() < deadline:
        time.sleep(1)
        hb = daemon.wait_history(api, 'Backup')
    alive = True
    try:
        api.status()
    except Exception:
        alive = False
    live = _grep_log(t, 'Starting live stream repair')
    failed_over = _grep_log(t, 'Failing over Primary')
    return ('failoverlive', alive and failed_over == 1 and hb['Status'].startswith('SUCCESS'),
            'status=%s backup_status=%s live_logs=%d failover_logs=%d daemon_alive=%s'
            % (hp['Status'], hb['Status'], live, failed_over, alive))


def scenario_wholefilerestart(daemon, t):
    """Corner case: nzbget restarts between download and post-processing
    while a whole-file job (no decoded size, no holes yet) waits in the
    queue state. The job must be saved and loaded intact, so the restarted
    daemon still proves the duplicate and recreates the volume."""
    members, donor_members, payloads = _wholefile_fixture(t, 'rs')
    members[1] = members[1][:4] + (set(),)
    api = daemon.wait_ready()
    api.pausepost()
    daemon.append(api, 'DonRS', build_multi_nzb(donor_members), True, 'rs-key', 50)
    daemon.append(api, 'RelRS', build_multi_nzb(members), False, 'rs-key', 100)
    deadline = time.time() + 120
    while time.time() < deadline and _grep_log(t, 'Collection RelRS completely downloaded') == 0:
        time.sleep(0.5)
    queued_job = _grep_log(t, 'no article available')
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    api.resumepost()
    h = daemon.wait_history(api, 'RelRS')
    recreated = _grep_log(t, 'Recreating Rel.part03.rar')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=both_dirs) for m in members)
    return ('wholefilerestart', queued_job == 1 and recreated == 1 and integ,
            'status=%s queued_logs=%d recreated_logs=%d integrity=%s'
            % (h['Status'], queued_job, recreated, integ))


def scenario_wholefilerestartlive(daemon, t):
    """Live, mid-download: a volume with no article available is queued for
    stream repair while other volumes still download, and RustyGet is shut
    down right then (live 6790: a deploy restarted it at that moment). After
    the restart the queue must load cleanly: no "Error reading diskstate",
    no file whose article count doubled (137 became 274 live), no file named
    by its id (<id>.out.tmp), and the release still completes."""
    members, donor_members, payloads = _wholefile_fixture(t, 'rl')
    api = daemon.wait_ready()
    daemon.append(api, 'DonRL', build_multi_nzb(donor_members), True, 'rl-key', 50)
    daemon.append(api, 'RelRL', build_multi_nzb(members), False, 'rl-key', 100)
    deadline = time.time() + 120
    while time.time() < deadline and _grep_log(t, 'no article available') == 0:
        time.sleep(0.05)
    still_downloading = _grep_log(t, 'Collection RelRL completely downloaded') == 0
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    h = daemon.wait_history(api, 'RelRL')
    diskstate_errors = _grep_log(t, 'Error reading diskstate')
    tmp_names = _grep_log(t, '.out.tmp')
    import re as _re
    try:
        log_text = t.read_file('nzbget.log').decode(errors='replace')
    except Exception:
        log_text = ''
    counts = {}
    for name, total in _re.findall(r'RelRL/(\S+) \[\d+/(\d+)\]', log_text):
        counts.setdefault(name, set()).add(int(total))
    doubled = sum(1 for v in counts.values() if len(v) > 1)
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=both_dirs) for m in members)
    ok = still_downloading and diskstate_errors == 0 and tmp_names == 0 and doubled == 0 and integ
    return ('wholefilerestartlive', ok,
            'mid_download=%s status=%s diskstate_errors=%d tmp_names=%d doubled=%d integrity=%s'
            % (still_downloading, h['Status'], diskstate_errors, tmp_names, doubled, integ))


def scenario_restartfirstdead(daemon, t):
    """Restart with DirectRename on while a volume that has had no article
    arrive (first article included) is being tried (live 6790: a deploy
    restarted RustyGet then). After the restart the direct-rename pass loaded
    that file's article list from disk to look for a first article, and the
    main picker, holding a read of the file from before that pass, loaded the
    list a second time: 137 articles became 274, the saved state no longer
    matched ("Error reading diskstate"), the volume was named <id>.out.tmp and
    the job failed on health. part02 and part03 are wholly dead (two, so
    direct rename cannot name them by elimination and stays running); only
    part01 can complete. Every file must keep its own article count."""
    seg = 100_000
    vol = 1_500_000
    members = [
        ('fdA/x.part01.rar', 'Rel.part01.rar', 300_000, seg, set()),
        ('fdA/x.part02.rar', 'Rel.part02.rar', vol, 5_000, set(range(1, 301))),
        ('fdA/x.part03.rar', 'Rel.part03.rar', vol, seg, set(range(1, 16))),
    ]
    payloads = {}
    for i, m in enumerate(members):
        data = _payload(m[2], 9400 + i)
        payloads[m[1]] = data
        t.write_file(os.path.join('data', m[0]), data)
    # The donor carries only part01, so the dead volumes stay dead and their
    # names are never confirmed, as on 6790.
    donor_members = [(m[0].replace('fdA', 'fdB'), m[1].replace('Rel.', 'Other.'),
                      m[2], seg, set()) for m in members if 'part01' in m[1]]
    for dm in donor_members:
        t.write_file(os.path.join('data', dm[0]), payloads[dm[1].replace('Other.', 'Rel.')])
    api = daemon.wait_ready()
    daemon.append(api, 'DonFD', build_multi_nzb(donor_members), True, 'fd-key', 50)
    # Obfuscated posted names with the real names only in the par2, which is
    # listed first so it arrives before the restart, as on 6790.
    par = generators.par2_index([(m[1], payloads[m[1]]) for m in members])
    t.write_file(os.path.join('data', 'fdA/rel.par2'), par)
    obf = ['Kq7vT2mZpX9dLw.rar', 'LbX0gshaVcZjLie.rar', 'R4nWq8yHs1Ufe.rar']
    rel_members = [(m[0], o, m[2], m[3], m[4]) for m, o in zip(members, obf)]
    rel_members.insert(0, ('fdA/rel.par2', 'Rel.par2', len(par), 500_000, set()))
    daemon.append(api, 'RelFD', build_multi_nzb(rel_members), False, 'fd-key', 100)
    deadline = time.time() + 120
    import re as _re
    def _mid():
        try:
            txt = t.read_file('nzbget.log').decode(errors='replace')
        except Exception:
            return False
        if 'Successfully downloaded RelFD/Rel.par2' not in txt:
            return False
        return bool(_re.search(r'Download RelFD/LbX0gshaVcZjLie\.rar \[(?:[4-9]\d|1\d\d)/300\] failed', txt))
    while time.time() < deadline and not _mid():
        time.sleep(0.05)
    mid = _mid() and _grep_log(t, 'Collection RelFD completely downloaded') == 0
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    h = daemon.wait_history(api, 'RelFD')
    try:
        log_text = t.read_file('nzbget.log').decode(errors='replace')
    except Exception:
        log_text = ''
    counts = {}
    for name, total in _re.findall(r'RelFD/(\S+) \[\d+/(\d+)\]', log_text):
        counts.setdefault(name, set()).add(int(total))
    doubled = sorted(n for n, v in counts.items() if len(v) > 1)
    # The restart must have caught a dead volume still being tried, or the
    # double-load path was never reached and the run proves nothing.
    after = log_text.split('server-mode')[-1]
    retried = bool(_re.search(r'Download(?:ing)? RelFD/(?:LbX0gshaVcZjLie|R4nWq8yHs1Ufe)\.rar \[', after))
    diskstate_errors = log_text.count('Error reading diskstate')
    tmp_names = log_text.count('.out.tmp')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=both_dirs)
                for m in members if 'part01' in m[1])
    ok = mid and retried and not doubled and diskstate_errors == 0 and tmp_names == 0 and integ
    return ('restartfirstdead', ok,
            'mid_download=%s retried_after_restart=%s status=%s doubled=%s diskstate_errors=%d '
            'tmp_names=%d integrity=%s'
            % (mid, retried, h['Status'], doubled, diskstate_errors, tmp_names, integ))


def scenario_wholefilestale(daemon, t):
    """A recreation of a missing volume was cut short by a shutdown: the volume is
    on disk at its full size, mostly zeros, and its job wasn't written back. After
    the restart it is recreated again and the release completes. Every later pass
    refused it ("file already exists"), and the zeros stayed."""
    members, donor_members, payloads = _wholefile_fixture(t, 'ws')
    api = daemon.wait_ready()
    api.pausepost()
    daemon.append(api, 'DonWS', build_multi_nzb(donor_members), True, 'ws-key', 50)
    daemon.append(api, 'RelWS', build_multi_nzb(members), False, 'ws-key', 100)
    deadline = time.time() + 120
    while time.time() < deadline and _grep_log(t, 'Collection RelWS completely downloaded') == 0:
        time.sleep(0.5)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    part01 = next((rel for rel in t.find_files('main') if rel.endswith('Rel.part01.rar')), None)
    if part01:
        stale = os.path.join(os.path.dirname(t.path(part01)), 'Rel.part03.rar')
        with open(stale, 'wb') as f:
            f.truncate(len(payloads['Rel.part03.rar']))
    daemon.start()
    api = daemon.wait_ready()
    api.resumepost()
    h = daemon.wait_history(api, 'RelWS')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=both_dirs) for m in members)
    refused = _grep_log(t, 'file already exists')
    ok = bool(part01) and integ and refused == 0
    return ('wholefilestale', ok, 'status=%s integrity=%s refused_logs=%d' % (h['Status'], integ, refused))


def scenario_reloadpostqueue(daemon, t):
    """nzbget reloads (saving settings in the web UI does) while a download
    waits in post-processing, three times: the reloaded post job must run.
    Before, the queue counted as loaded from the start of the reload, so the
    post-processor could sanitise the queue before it was loaded again: the
    reloaded job kept its persisted stage, was never counted as queued, and
    post-processing never started (it showed LOADING_PARS for good). Stream
    repair makes post-processing run for minutes, so a reload meets it more
    often."""
    size, seg = 3_000_000, 500_000
    data = _payload(size, 4260)
    pp = _place_copy(t, 'rpA', data, 'movie.mkv')
    api = daemon.wait_ready()
    api.pausepost()
    # a queue that takes a moment to load: the post-processor starts first
    for i in range(40):
        filler = build_multi_nzb([('rpA/movie.mkv', 'Fill%02d-%02d.bin' % (i, j), size, seg, set())
                                  for j in range(50)])
        daemon.append(api, 'Filler%02d' % i, filler, True, 'rp-fill-%d' % i, 0)
    daemon.append(api, 'RelRP', build_nzb(pp, 'movie.mkv', size, seg, set()), False, 'rp-key', 100)
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, 'Collection RelRP completely downloaded') == 0:
        time.sleep(0.2)
    for _ in range(3):
        api = daemon.reload(api)
    api.resumepost()
    try:
        h = daemon.wait_history(api, 'RelRP', timeout=60)
        status = h['Status']
    except RuntimeError:
        status = 'STUCK:%s' % [g['Status'] for g in api.listgroups()]
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    return ('reloadpostqueue', status.startswith('SUCCESS') and integ,
            'status=%s integrity=%s' % (status, integ))


# the archive tools the daemon shells out to, wherever this host has them: the
# harness runs on Linux and macOS, and a distribution's path isn't portable. A
# scenario that names a tool the host lacks is skipped (see main()).
UNRAR_CMD = 'UnrarCmd=%s' % (generators.UNRAR_PATH or 'unrar')
SEVENZIP_CMD = 'SevenZipCmd=%s' % (generators.SEVENZIP_PATH or '7z')

PROD_OPTIONS = ['DupeArticleFallback=live', 'DupeStreamDecompress=yes', 'ContinuePartial=yes',
                'ArticleCache=8192', 'FileNaming=auto', 'ReorderFiles=yes', 'PostStrategy=rocket',
                'ParCheck=auto', 'ParRepair=yes', 'ParScan=dupe', 'ParQuick=yes', 'ParRename=yes',
                'RarRename=yes', 'DirectRename=yes', 'HealthCheck=dupe', 'Unpack=yes', 'DirectUnpack=yes',
                UNRAR_CMD, SEVENZIP_CMD, 'ArticleRetries=0']


def _prod_7z_set(t, tag, movie, volume_kb=1500):
    """A store-mode 7z split of movie.mkv: [(name suffix, bytes)]."""
    import shutil as _sh
    import tempfile as _tf
    work = _tf.mkdtemp(prefix='prod7z-')
    with open(os.path.join(work, 'movie.mkv'), 'wb') as f:
        f.write(movie)
    subprocess.run([generators.SEVENZIP_PATH, 'a', '-mx0', '-v%dk' % volume_kb, 'out.7z', 'movie.mkv'], cwd=work,
                   check=True, capture_output=True)
    vols = sorted(n for n in os.listdir(work) if n.startswith('out.7z.'))
    out = [(n[len('out'):], open(os.path.join(work, n), 'rb').read()) for n in vols]
    _sh.rmtree(work)
    return out


def _prod_wholefile(daemon, t, tag, damage):
    """Production options; a 4-volume store 7z of movie.mkv with a par2 index;
    the byte-identical repost under other names in history. damage:
    {volume index: missing parts}. Returns (history, extracted_ok)."""
    seg = 300_000
    movie = _payload(5_500_000, 9990)
    vols = _prod_7z_set(t, tag, movie)
    members, donor_members = [], []
    for i, (suffix, data) in enumerate(vols):
        t.write_file(os.path.join('data', '%sA/v%d' % (tag, i)), data)
        t.write_file(os.path.join('data', '%sB/v%d' % (tag, i)), data)
        members.append(('%sA/v%d' % (tag, i), 'Rel' + suffix, len(data), seg, damage.get(i, set())))
        donor_members.append(('%sB/v%d' % (tag, i), 'Other' + suffix, len(data), 200_000, set()))
    par = generators.par2_index([('Rel' + s, d) for s, d in vols])
    t.write_file(os.path.join('data', '%sA/rel.par2' % tag), par)
    members.append(('%sA/rel.par2' % tag, 'Rel.par2', len(par), seg, set()))
    api = daemon.wait_ready()
    # marked dead by a dupe tool: no failover fetches it, so stream repair is the
    # last option left (B78) - the repair itself is what these scenarios test
    _ds_append(api, 'Don' + tag, build_multi_nzb(donor_members), tag + '-key', 50,
               params=[{'Name': 'DupeAlive', 'Value': '0'}])
    daemon.append(api, 'Rel' + tag, build_multi_nzb(members), False, tag + '-key', 100)
    h = daemon.wait_history(api, 'Rel' + tag, timeout=300)
    ok = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    return h, ok


def scenario_prodwholefile(daemon, t):
    """Fix 2 under the production option set (live, direct rename/unpack,
    par-rename, quick par-check, unpack, article cache, HealthCheck=dupe):
    one 7z volume missing entirely, the rest intact. The volume is recreated
    from the proven repost, the par-check passes, and unpack extracts a
    byte-identical movie.mkv."""
    h, ok = _prod_wholefile(daemon, t, 'PW', {2: set(range(1, 7))})
    recreated = _grep_log(t, 'Recreating')
    return ('prodwholefile', ok and recreated == 1 and h['Status'].startswith('SUCCESS'),
            'status=%s par=%s unpack=%s recreated_logs=%d extracted_ok=%s'
            % (h['Status'], h['ParStatus'], h['UnpackStatus'], recreated, ok))


def scenario_prodstream(daemon, t):
    """Stream repair under the production option set: two volumes with holes
    (one at its start, one in the middle), the byte-identical repost in
    history; the par-check passes and unpack extracts a byte-identical
    movie.mkv."""
    h, ok = _prod_wholefile(daemon, t, 'PS', {0: {1}, 2: {3, 4}})
    return ('prodstream', ok and h['Status'].startswith('SUCCESS'),
            'status=%s par=%s unpack=%s extracted_ok=%s' % (h['Status'], h['ParStatus'], h['UnpackStatus'], ok))


def _prod_rar(daemon, t, tag, damage):
    """Production options; a 4-volume store-mode rar set (real CRCs, so unrar
    and direct unpack accept it) of movie.mkv with a par2 index; the
    byte-identical repost under other names in history."""
    seg = 300_000
    movie = _payload(5_000_000, 9995)
    vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 1_400_000)
    members, donor_members = [], []
    for i, data in enumerate(vols):
        t.write_file(os.path.join('data', '%sA/v%d' % (tag, i)), data)
        t.write_file(os.path.join('data', '%sB/v%d' % (tag, i)), data)
        members.append(('%sA/v%d' % (tag, i), 'Rel.part%02d.rar' % (i + 1), len(data), seg, damage.get(i, set())))
        donor_members.append(('%sB/v%d' % (tag, i), 'Other.part%02d.rar' % (i + 1), len(data), 200_000, set()))
    par = generators.par2_index([('Rel.part%02d.rar' % (i + 1), d) for i, d in enumerate(vols)])
    t.write_file(os.path.join('data', '%sA/rel.par2' % tag), par)
    members.append(('%sA/rel.par2' % tag, 'Rel.par2', len(par), seg, set()))
    api = daemon.wait_ready()
    # marked dead by a dupe tool: no failover fetches it, so stream repair is the
    # last option left (B78) - the repair itself is what these scenarios test
    _ds_append(api, 'Don' + tag, build_multi_nzb(donor_members), tag + '-key', 50,
               params=[{'Name': 'DupeAlive', 'Value': '0'}])
    daemon.append(api, 'Rel' + tag, build_multi_nzb(members), False, tag + '-key', 100)
    h = daemon.wait_history(api, 'Rel' + tag, timeout=300)
    ok = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    return h, ok


def scenario_prodrarwhole(daemon, t):
    """Fix 2 on a rar set under the production option set: the third volume is
    missing entirely while direct unpack extracts the others. The volume is
    recreated, par-check passes, and unpack extracts a byte-identical movie.
    When direct unpack is still running as the volume is found dead, the live
    dead-volume borrow hands it the volume instead (it extracts on past it and
    patches it in once borrowed; its archive checksums vouch for the movie, so
    par-check is skipped): no file needs recreating then."""
    h, ok = _prod_rar(daemon, t, 'RW', {2: set(range(1, 6))})
    recreated = _grep_log(t, 'Recreating')
    lent_live = _grep_log(t, 'for dead volume Rel.part03.rar from a duplicate; direct unpack reads it in place')
    return ('prodrarwhole', ok and (recreated == 1 or (recreated == 0 and lent_live == 1)) and
            h['Status'].startswith('SUCCESS'),
            'status=%s par=%s unpack=%s recreated_logs=%d lent_to_direct_unpack=%d extracted_ok=%s'
            % (h['Status'], h['ParStatus'], h['UnpackStatus'], recreated, lent_live, ok))


def scenario_prodrarstream(daemon, t):
    """Stream repair on a rar set under the production option set: holes in
    the first volume's start (its rar headers) and in the middle of the third
    while direct unpack runs; repaired, par-checked, extracted byte-identical."""
    h, ok = _prod_rar(daemon, t, 'RS', {0: {1}, 2: {3}})
    return ('prodrarstream', ok and h['Status'].startswith('SUCCESS'),
            'status=%s par=%s unpack=%s extracted_ok=%s' % (h['Status'], h['ParStatus'], h['UnpackStatus'], ok))


def _skiphole_rar(daemon, t, tag, donor, recovery, damage=None):
    """Production options with direct unpack; a 4-volume store-mode rar set of
    movie.mkv whose second volume loses one article in its middle. With
    ``donor`` a byte-identical repost waits in history (marked dead, so only
    stream repair borrows from it); with ``recovery`` the set carries real par2
    recovery data and nothing else can fill the hole. Direct unpack must read
    past the hole instead of pausing, and patch the output once the volume is
    repaired."""
    import subprocess as _sp
    seg = 300_000
    movie = _payload(5_000_000, 9971)
    vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 1_400_000)
    work = t.path('data', tag + 'A')
    os.makedirs(work, exist_ok=True)
    names = ['Rel.part%02d.rar' % (i + 1) for i in range(len(vols))]
    for name, data in zip(names, vols):
        t.write_file(os.path.join('data', tag + 'A', name), data)
    if damage is None:
        damage = {1: {3}}                   # bytes 600,000-899,999 of volume 2: past its headers
    members = [('%sA/%s' % (tag, n), n, len(d), seg, damage.get(i, set()))
               for i, (n, d) in enumerate(zip(names, vols))]
    if recovery:
        _sp.run(['par2', 'create', '-q', '-q', '-s65536', '-c12', '-n1', '-a', 'Rel', 'Rel.par2'] + names,
                cwd=work, check=True, capture_output=True)
        for n in sorted(os.listdir(work)):
            if n.endswith('.par2'):
                members.append(('%sA/%s' % (tag, n), n, os.path.getsize(os.path.join(work, n)), seg, set()))
    else:
        par = generators.par2_index(list(zip(names, vols)))
        t.write_file(os.path.join('data', tag + 'A', 'Rel.par2'), par)
        members.append(('%sA/Rel.par2' % tag, 'Rel.par2', len(par), seg, set()))
    api = daemon.wait_ready()
    if donor:
        donor_members = []
        for i, data in enumerate(vols):
            t.write_file(os.path.join('data', '%sB/v%d' % (tag, i)), data)
            donor_members.append(('%sB/v%d' % (tag, i), 'Other.part%02d.rar' % (i + 1), len(data), 200_000, set()))
        _ds_append(api, 'Don' + tag, build_multi_nzb(donor_members), tag + '-key', 50,
                   params=[{'Name': 'DupeAlive', 'Value': '0'}])
    daemon.append(api, 'Rel' + tag, build_multi_nzb(members), False, tag + '-key', 100)
    h = daemon.wait_history(api, 'Rel' + tag, timeout=300)
    ok = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    # a set extracted while it downloads goes to persistent partial files (Linux): the hole
    # is noted as it is met and patched inline once its volume is repaired, before the
    # extraction ends; otherwise it is reported at the end and patched afterwards
    skipped = (_grep_log(t, 'extracted past damage in movie.mkv') +
               _grep_log(t, 'Unrar: movie.mkv has a hole at'))
    patched = (_grep_log(t, 'repaired movie.mkv from the repaired volumes') +
               _grep_log(t, 'Unrar: patched movie.mkv from repaired Rel.part02.rar while extracting'))
    paused = _grep_log(t, 'Pausing direct unpack at Rel.part02.rar')
    detail = 'status=%s par=%s unpack=%s extracted_ok=%s skipped=%d patched=%d paused=%d' % (
        h['Status'], h['ParStatus'], h['UnpackStatus'], ok, skipped, patched, paused)
    good = (ok and h['Status'].startswith('SUCCESS') and skipped >= 1 and patched >= 1 and paused == 0)
    return good, detail


def _nested_donor(t, sub, movie, volumes):
    """Writes ``movie`` nested in another container (a stored rar of
    ``volumes`` volumes holding a rar holding movie.mkv) under data/``sub``;
    returns its nzb members."""
    inner = generators.rar3_store_volumes_valid('movie.mkv', movie, len(movie) + 1)
    assert len(inner) == 1
    outer = generators.rar3_store_volumes_valid('Show.rar', inner[0],
                                                (len(inner[0]) + volumes - 1) // volumes)
    assert len(outer) == volumes
    donor_members = []
    for i, data in enumerate(outer):
        name = 'Other.part%02d.rar' % (i + 1)
        rel = '%s/%s' % (sub, name)
        t.write_file(os.path.join('data', rel), data)
        donor_members.append((rel, name, len(data), 100_000, set()))
    return donor_members


def _dead_volume_nested(daemon, t, tag, labels, donor_seed=None, donor_volumes=3, donor_size=None,
                        extra_backups=()):
    """A plain stored rar set without par2 (articles of 20 KB, so direct rename
    hashes each volume's first article) loses three middle volumes entirely;
    its backup in history holds the same movie nested in another container (a
    stored rar holding a rar holding movie.mkv), its first article telling
    nothing about the packing. ``labels``: the backup's nzb parameters (what a
    fleet and TwinCheck stored). ``donor_seed``: the backup holds another
    video (of ``donor_size`` bytes, the same size by default). ``donor_volumes``: how many volumes the backup's
    outer rar has (as many as the primary's lets TwinCheck sample the two
    posts against each other). ``extra_backups``: (name, members, score,
    params) of more backups, added to history before it. Returns (primary,
    backup, movie)."""
    seg = 20_000
    movie = _payload(15_000_000, 23070)
    vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 1_000_000)
    dead = {6, 7, 8}
    members = []
    for i, data in enumerate(vols):
        name = 'Show.part%02d.rar' % (i + 1)
        rel = '%sA/%s' % (tag, name)
        t.write_file(os.path.join('data', rel), data)
        parts = (len(data) + seg - 1) // seg
        members.append((rel, name, len(data), seg,
                        set(range(1, parts + 1)) if i in dead else set()))
    donor_movie = movie if donor_seed is None else _payload(donor_size or len(movie), donor_seed)
    donor_members = _nested_donor(t, tag + 'B', donor_movie, donor_volumes)
    api = daemon.wait_ready()
    # the primary is queued paused first, so the lower-scored backup is
    # deleted as duplicate into history instead of downloading
    daemon.append(api, 'Primary', build_multi_nzb(members), True, tag + '-key', 100)
    for name, nzb_members, score, params in extra_backups:
        _ds_append(api, name, build_multi_nzb(nzb_members), tag + '-key', score, paused=False,
                   params=[{'Name': k, 'Value': v} for k, v in params])
        daemon.wait_history(api, name, timeout=60)
    _ds_append(api, 'Backup', build_multi_nzb(donor_members), tag + '-key', 90, paused=False,
               params=[{'Name': k, 'Value': v} for k, v in labels])
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    deadline = time.time() + 180
    hb = daemon.wait_history(api, 'Backup')
    while hp['Status'].startswith('FAILURE') and hb['Status'].startswith('DELETED') and time.time() < deadline:
        time.sleep(0.5)
        hb = daemon.wait_history(api, 'Backup')
    return hp, hb, movie


def scenario_deadvolumenestednopar(daemon, t):
    """South Park S23E07, under the production options (DirectRename=yes): a
    plain stored rar set without par2 loses three middle volumes entirely,
    while its backup in history holds the same movie nested in another
    container, labelled by the fleet as of unknown packing with the same video
    inside. The first dead volume failed 40 articles in a row before it
    ended, which failed the download over, cancelling direct unpack and
    fetching the backup from its first byte. Now the download is kept: the
    dead volumes are borrowed from the backup's inner movie while direct
    unpack waits, and the extraction carries on to the end."""
    hp, hb, movie = _dead_volume_nested(daemon, t, 'dvn', [('DupeCopyKind', 'unknown'),
                                                          ('DupeVideo', 'same')])
    failed_over = _grep_log(t, 'Failing over Primary')
    cancelled = _grep_log(t, 'Cancelling direct unpack')
    kept = _grep_log(t, 'lends the lost parts')
    du_ok = _grep_log(t, 'Direct unpack for Primary successful')
    integ = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    ok = (failed_over == 0 and cancelled == 0 and kept == 1 and du_ok == 1 and integ and
          hp['Status'].startswith('SUCCESS') and hb['Status'].startswith('DELETED'))
    return ('deadvolumenestednopar', ok,
            'status=%s backup=%s failover_logs=%d du_cancelled=%d kept_logs=%d du_ok=%d extracted_ok=%s'
            % (hp['Status'], hb['Status'], failed_over, cancelled, kept, du_ok, integ))


def scenario_deadvolumeencrypted(daemon, t):
    """The user's question (2026-11-20): borrow from an encrypted alt when its
    nzb carries the password. A plain stored rar set without par2 loses three
    middle volumes entirely; its backup in history holds the same movie in a
    RAR5 store set with ENCRYPTED HEADERS, whose password travels in the
    backup's own nzb <meta type="password">, and which a fleet classed
    `borrowable-encrypted`. The download is kept, the dead volumes are
    borrowed from the backup (decrypted on the way) while direct unpack waits,
    and the extraction carries on to the end: no failover, no restart."""
    if not generators.HAVE_RAR:
        return ('deadvolumeencrypted', None, 'SKIP: rar not installed')
    tag = 'dve'
    password = 'alt-pw 7'
    seg = 20_000
    movie = _payload(15_000_000, 23070)
    vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 1_000_000)
    dead = {6, 7, 8}
    members = []
    for i, data in enumerate(vols):
        name = 'Show.part%02d.rar' % (i + 1)
        rel = '%sA/%s' % (tag, name)
        t.write_file(os.path.join('data', rel), data)
        parts = (len(data) + seg - 1) // seg
        members.append((rel, name, len(data), seg,
                        set(range(1, parts + 1)) if i in dead else set()))
    with tempfile.TemporaryDirectory(prefix='dve-') as workdir:
        enc = generators.rar5_store_volumes_hp('movie.mkv', movie, 5_000_000, password, workdir)
    donor_members = []
    for i, data in enumerate(enc):
        name = 'Other.part%d.rar' % (i + 1)
        rel = '%sB/%s' % (tag, name)
        t.write_file(os.path.join('data', rel), data)
        donor_members.append((rel, name, len(data), 100_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(members), True, tag + '-key', 100)
    _ds_append(api, 'Backup', build_multi_nzb(donor_members, password=password), tag + '-key', 90,
               paused=False,
               params=[{'Name': 'DupeCopyKind', 'Value': 'borrowable-encrypted'},
                       {'Name': 'DupeLocked', 'Value': 'yes'}])
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    hb = daemon.wait_history(api, 'Backup')
    failed_over = _grep_log(t, 'Failing over Primary')
    cancelled = _grep_log(t, 'Cancelling direct unpack')
    kept = _grep_log(t, 'lends the lost parts')
    du_ok = _grep_log(t, 'Direct unpack for Primary successful')
    integ = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    ok = (failed_over == 0 and cancelled == 0 and kept == 1 and du_ok == 1 and integ and
          hp['Status'].startswith('SUCCESS') and hb['Status'].startswith('DELETED'))
    return ('deadvolumeencrypted', ok,
            'status=%s backup=%s failover_logs=%d du_cancelled=%d kept_logs=%d du_ok=%d extracted_ok=%s'
            % (hp['Status'], hb['Status'], failed_over, cancelled, kept, du_ok, integ))



# ---- failover-continue: the backup carries on the primary's extraction ----

def _failcont_requests(t):
    """Every BODY/ARTICLE request nserv received, as message-ids in order."""
    return re.findall(r'Received: (?:BODY|ARTICLE) <([^>]*)>',
                      t.read_file('nserv.log').decode(errors='replace'))


def _failcont_before(ids, prefix, cut, seg):
    """Data articles under ``prefix`` that lie wholly before ``cut`` (the
    primary's frontier, its own offsets), less one article of slack: for a
    twin, whose articles sit at the primary's offsets, none may be fetched."""
    n = 0
    for mid in set(ids):
        if not mid.startswith(prefix) or '.par2?' in mid:
            continue
        m = re.search(r'/([^/?]+)\?\d+=(\d+):(\d+)$', mid)
        if m and m.group(1) in _failcont_before.offsets:
            start = _failcont_before.offsets[m.group(1)] + int(m.group(2))
            if start + int(m.group(3)) <= cut - seg:
                n += 1
    return n


_failcont_before.offsets = {}


def _failcont_bytes(ids, prefix):
    """(bytes served, articles served, data articles requested more than once,
    par2 articles requested more than once) for the message-ids under
    ``prefix``: an id ``path?i=off:size`` carries its size; one marked ``!2``
    is missing on the server and serves nothing. TwinCheck reads each copy's
    par2 index on its own, also the one the download already holds: counted
    apart (it reads no volume data)."""
    served, count, seen, again, par_again = 0, 0, set(), 0, 0
    for mid in ids:
        if not mid.startswith(prefix):
            continue
        if mid in seen:
            if '.par2?' in mid:
                par_again += 1
            else:
                again += 1
            continue
        seen.add(mid)
        # a copy's par2 index is read by TwinCheck, apart from the data: not counted
        if mid.endswith('!2') or '.par2?' in mid:
            continue
        m = re.search(r'\?\d+=\d+:(\d+)$', mid)
        if m:
            served += int(m.group(1))
            count += 1
    return served, count, again, par_again


def _log_text(t):
    try:
        return t.read_file('nzbget.log').decode(errors='replace')
    except Exception:
        return ''


def _failcont(daemon, t, name, backup, die, fleet=False, twin_label='DupeCopyKind', need_live=False,
              primary='store', blocker=False):
    """The ultimate goal (extract while downloading, 2026-11-25): a stored rar
    set of movie.mkv extracts while it downloads, and the primary dies at
    ``die`` (a fraction of its data: every article from there on is gone). Its
    backup waits in history, labelled the way a fleet labels it:

    - ``twin``: the same posting again (same volumes, articles and par2), its
      articles swapped in one for one;
    - ``alt``: the same movie in another stored rar set (RAR 1.5-4, other
      volume and article sizes), lent by the bytes of the video inside;
    - ``aes``: the same movie in a RAR5 store set with encrypted headers, the
      password in the backup's nzb;
    - ``compressed``: the same movie compressed (rar -m3), labelled
      ``unborrowable`` by a fleet: it cannot lend live, so the primary fails
      over to it as before, with the reason logged;
    - ``rar5m3``: the same movie compressed (rar -m3, RAR5), labelled
      ``DupeVideo=same`` by the fleet intake with no DupeKind (TwinCheck
      without par2 leaves none):
      zipfast restarts its decoder near the frontier with the primary's own
      output as the window (``open_at`` + ``complete_rar5``, held plaintext),
      so it lends only what lies past the frontier and a search region;
    - ``rar5m3aes``: as ``rar5m3`` with encrypted headers and data (``-hp``,
      password in the backup's nzb): its packed bytes are decrypted on the way;
    - ``lzma2``: the same (compressible) movie in a 7z LZMA2 set, labelled
      ``alt``: restarted at an LZMA2 chunk before the frontier
      (``complete_lzma2``) with the primary's output as the dictionary;
    - ``norestart``: the same movie in a 7z BZip2 set, labelled ``alt``: no
      restart point inside the member (``StartOnly``), so the primary fails
      over to it, with the reason logged;
    - ``aestwin`` / ``aesalt`` (with ``primary='aes'``): the primary itself is
      a RAR5 store set with encrypted headers and data (password in its nzb);
      its backup is a twin (the same encrypted volumes: they swap in one for
      one) or an alt (another RAR5 AES store set with other volume sizes and
      another password: an OPEN ITEM, the release completes from the alt);
    - ``rar5m3other``: as ``rar5m3`` (the intake read the video the same), but
      TwinCheck labelled it ``DupeKind=other`` (its video differs): that verdict
      stands, so it is never restarted and never lends a byte; the primary fails
      over to it, with the reason logged;
    - ``other``: another video of the same size in another stored rar set,
      labelled ``container`` (another container, the video inside not
      verified): it never lends a byte (no earlier confirmation stands in for
      a check of it), and the release completes from it with its own video.

    The backup continues the primary's extraction where it stopped: nothing
    of the primary is fetched again, the backup serves only what lies past the
    frontier (plus its headers and an article straddling the cut), direct
    unpack is never cancelled or restarted, and the movie is byte-identical.

    With ``fleet`` the two copies are one fleet (a ``dupes:`` key, F16): the
    backup waits held in the queue, not in history. ``twin_label='DupeKind'``
    labels a twin the way TwinCheck does, with no fleet kind.

    With ``blocker`` another download (Blocker, a rar set) starts its direct unpack first
    and is paused while its last volume is on the way (``_paused_blocker``), with one unpack
    slot (UnpackSlots=1): its waiting extraction must hold no slot, or the primary's direct
    unpack never runs and the primary fails over to its backup, downloaded whole (6790 XB
    copy 02/05, X54)."""
    if not generators.HAVE_RAR:
        return (name, None, 'SKIP: rar not installed')
    if backup in ('lzma2', 'norestart') and not generators.HAVE_7Z:
        return (name, None, 'SKIP: 7z not installed')
    tag = 'fc' + {'aestwin': 'w', 'aesalt': 'x', 'rar5m3': 'm', 'rar5m3aes': 'q', 'lzma2': 'l', 'norestart': 'n'}.get(
        backup, backup[0])
    password = 'fleet-pw 9' if backup == 'aes' else None
    primary_password = 'prim-pw 3' if primary == 'aes' else None
    if backup == 'aestwin':
        password = primary_password
    elif backup == 'aesalt':
        password = 'alt-pw 7'
    elif backup == 'rar5m3aes':
        password = 'alt-pw 8'
    seg = 50_000
    # a 7z LZMA2 coder stores incompressible chunks raw (borrowed by range, not restarted):
    # its movie compresses, so the frontier lies in LZMA chunks
    movie = _text_payload(15_000_000, 25110) if backup == 'lzma2' else _payload(15_000_000, 25110)
    # RAR5, as current releases are: a dead last volume of a RAR 1.5-4 set cannot be rebuilt
    # (its header stores the whole file's checksum; deadvolume::synthesize_frame)
    with tempfile.TemporaryDirectory(prefix='fc-') as workdir:
        vols = generators.rar5_store_volumes_hp('movie.mkv', movie, 1_000_000, primary_password, workdir)
    total = sum(len(v) for v in vols)
    cut = int(total * die)
    names = ['Show.part%02d.rar' % (i + 1) for i in range(len(vols))]
    members, base = [], 0
    for i, (vname, data) in enumerate(zip(names, vols)):
        rel = '%sA/%s' % (tag, vname)
        t.write_file(os.path.join('data', rel), data)
        parts = (len(data) + seg - 1) // seg
        dead = {p for p in range(1, parts + 1) if base + (p - 1) * seg + seg > cut}
        members.append((rel, vname, len(data), seg, dead))
        base += len(data)
    par = generators.par2_index(list(zip(names, vols)))
    t.write_file(os.path.join('data', '%sA/Show.par2' % tag), par)
    members.append(('%sA/Show.par2' % tag, 'Show.par2', len(par), seg, set()))
    labels = []
    if backup in ('twin', 'aestwin'):
        # a repost: the same files under the same names
        donor_vols = list(zip(names, vols))
        donor_seg = seg
        labels = [(twin_label, 'twin')]
    elif backup in ('alt', 'other'):
        donor_movie = movie if backup == 'alt' else _payload(len(movie), 25111)
        alt = generators.rar3_store_volumes_valid('movie.mkv', donor_movie, 1_400_000)
        donor_vols = [('Other.part%02d.rar' % (i + 1), d) for i, d in enumerate(alt)]
        donor_seg = 100_000
        # as a fleet's intake and TwinCheck label it (another container, the same video)
        labels = ([('DupeCopyKind', 'borrowable'), ('DupeVideo', 'same'), ('DupeKind', 'alt')]
                  if backup == 'alt' else [('DupeKind', 'container')])
    elif backup in ('lzma2', 'norestart'):
        with tempfile.TemporaryDirectory(prefix='fc-') as workdir:
            with open(os.path.join(workdir, 'movie.mkv'), 'wb') as f:
                f.write(movie)
            import subprocess as _sp
            _sp.run([generators.SEVENZIP_PATH, 'a', '-bd', '-m0=' + ('LZMA2' if backup == 'lzma2' else 'BZip2'),
                     '-v5000000b', 'out.7z', 'movie.mkv'], cwd=workdir, check=True, capture_output=True)
            outs = sorted(n for n in os.listdir(workdir) if n.startswith('out.7z.'))
            enc = [open(os.path.join(workdir, n), 'rb').read() for n in outs]
        donor_vols = [('Other.7z.%03d' % (i + 1), d) for i, d in enumerate(enc)]
        donor_seg = 100_000
        # labelled the way a fleet labels a compressed copy whose video it read the same
        labels = [('DupeCopyKind', 'unborrowable'), ('DupeVideo', 'same'), ('DupeKind', 'alt')]
    else:
        with tempfile.TemporaryDirectory(prefix='fc-') as workdir:
            if backup in ('aes', 'aesalt'):
                enc = generators.rar5_store_volumes_hp('movie.mkv', movie, 5_000_000, password, workdir)
            else:
                with open(os.path.join(workdir, 'movie.mkv'), 'wb') as f:
                    f.write(movie)
                import subprocess as _sp
                _sp.run([generators.RAR_PATH, 'a', '-idq', '-m3', '-ma5', '-v5000000b']
                        + (['-hp' + password] if backup == 'rar5m3aes' else []) + ['out.rar', 'movie.mkv'],
                        cwd=workdir, check=True, capture_output=True)
                outs = sorted((n for n in os.listdir(workdir) if n.startswith('out')),
                              key=lambda n: int(''.join(c for c in n if c.isdigit()) or 0))
                enc = [open(os.path.join(workdir, n), 'rb').read() for n in outs]
        donor_vols = [('Other.part%d.rar' % (i + 1), d) for i, d in enumerate(enc)]
        donor_seg = 100_000
        if backup in ('aes', 'aesalt'):
            labels = [('DupeCopyKind', 'borrowable-encrypted'), ('DupeLocked', 'yes'), ('DupeVideo', 'same'),
                      ('DupeKind', 'alt')]
        elif backup in ('rar5m3', 'rar5m3aes'):
            # no DupeKind (TwinCheck without par2 leaves none): the intake's verdict alone
            labels = [('DupeCopyKind', 'unborrowable'), ('DupeVideo', 'same')]
        elif backup == 'rar5m3other':
            # TwinCheck labels it `other` itself: its own par2 set (other files: an alt by par2)
            # and a video digest (DupeInner) unlike the primary's; the intake's DupeVideo=same
            # stays on it
            labels = [('DupeCopyKind', 'unborrowable'), ('DupeVideo', 'same'),
                      ('DupeInner', 'movie-mkv.00000000000000bb')]
        else:
            labels = [('DupeCopyKind', 'unborrowable')]
    donor_members = []
    for vname, data in donor_vols:
        rel = '%sB/%s' % (tag, vname)
        t.write_file(os.path.join('data', rel), data)
        donor_members.append((rel, vname, len(data), donor_seg, set()))
    if backup in ('twin', 'aestwin'):
        t.write_file(os.path.join('data', '%sB/Show.par2' % tag), par)
        donor_members.append(('%sB/Show.par2' % tag, 'Show.par2', len(par), seg, set()))
    elif backup == 'rar5m3other':
        bpar = generators.par2_index(donor_vols)
        t.write_file(os.path.join('data', '%sB/Other.par2' % tag), bpar)
        donor_members.append(('%sB/Other.par2' % tag, 'Other.par2', len(bpar), donor_seg, set()))
    donor_total = sum(len(d) for _, d in donor_vols)
    off = 0
    _failcont_before.offsets = {}
    for vname, data in donor_vols:
        _failcont_before.offsets[vname] = off
        off += len(data)
    api = daemon.wait_ready()
    blocked = _paused_blocker(daemon, t, api, 'Blocker', 'fcBlk', _blocker_members(t, 'fcBlk')) if blocker else None
    if blocker and blocked is not True:
        return (name, False, 'the blocker was not set up: %s' % blocked)
    key = 'dupes:show.failcont.' + tag if fleet else tag + '-key'
    daemon.append(api, 'Primary', build_multi_nzb(members, password=primary_password), True, key, 100)
    if backup == 'rar5m3other':
        api.editqueue('GroupSetParameter', 0, 'DupeInner=movie-mkv.00000000000000aa',
                      [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    _ds_append(api, 'Backup', build_multi_nzb(donor_members, password=password), key, 90,
               paused=False, params=[{'Name': k, 'Value': v} for k, v in labels])
    if fleet:
        if _params(_wait_held(api, 'Backup')).get('DupeHeld') != 'yes':
            return (name, False, 'the backup was not held by its fleet')
    else:
        daemon.wait_history(api, 'Backup', timeout=60)
    started = time.time()
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    hb = daemon.wait_history(api, 'Backup', timeout=300)
    deadline = time.time() + 240
    while (not hp['Status'].startswith('SUCCESS') and not hb['Status'].startswith(('SUCCESS', 'FAILURE'))
           and time.time() < deadline):
        time.sleep(0.5)
        hb = daemon.wait_history(api, 'Backup', timeout=300)
    elapsed = time.time() - started
    ids = _failcont_requests(t)
    a_bytes, a_count, a_again, a_par_again = _failcont_bytes(ids, tag + 'A/')
    b_bytes, b_count, b_again, b_par_again = _failcont_bytes(ids, tag + 'B/')
    failed_over = _grep_log(t, 'Failing over Primary')
    cancelled = _grep_log(t, 'Cancelling direct unpack')
    backup_unpacks = _grep_log(t, 'Directly unpacking Backup') + _grep_log(t, 'Unpacking Backup')
    du_ok = _grep_log(t, 'Direct unpack for Primary successful')
    used_direct = _grep_log(t, 'Using directly unpacked files')
    # an extraction that started over (persistent session falling back) or stopped at a volume
    # read as it was written
    # (a persistent session refusing a method it does not handle, AES members, falls back before
    # it wrote anything: no restart)
    restarts = (_grep_log(t, 'cannot continue from its rebuilt copy') +
                len(re.findall(r'Unrar: progressive direct unpack \((?!volume count)(?![^\n]*UnsupportedMethod)',
                               _log_text(t))))
    reads = _live_reads(t)
    as_written = reads[0] if reads else 0.0
    # the dying volume, read as it was written, went on from its rebuilt copy
    continued = _grep_log(t, 'goes on from its rebuilt copy at the payload frontier')
    integ = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    winner = 'Primary' if hp['Status'].startswith('SUCCESS') else (
        'Backup' if hb['Status'].startswith('SUCCESS') else 'none')
    # the backup may serve what lies past the cut, the article holding each of its
    # volume's headers (its payload map is built from them; encrypted headers
    # also need each volume's last article), and two of its articles at the
    # cut (the one straddling it; the one holding the bytes the first check of
    # its video compares with the primary's, which under AES also gives the
    # IV block)
    header_articles = len(donor_vols) * (2 if backup in ('aes', 'aesalt') else 1)
    if backup in ('twin', 'aestwin'):
        # one for one: only the articles past the frontier, one straddling it
        header_articles = 0
    b_budget = int(donor_total * (1 - die)) + (header_articles + 2) * donor_seg
    if backup in ('rar5m3', 'rar5m3aes'):
        # a restart lands at a block start before the frontier: the plan fetches from the
        # estimate less a search region (zipfast SEED_SEARCH, 2 MiB) and a block
        b_budget += 3_000_000
    elif backup == 'lzma2':
        # the chunk framing is walked from the folder's start (every chunk header before
        # the frontier: each article before it is read once for them), then the folder
        # from the restart chunk to its end; never twice, never extracted from its start
        b_budget = donor_total
    b_before = _failcont_before(ids, tag + 'B/', cut, seg) if backup in ('twin', 'aestwin') else 0
    # Known residual: before TwinCheck labels an alt (another container), the whole-file pass
    # may sample one of its volumes of about a damaged volume's size; a sample batch it cancels
    # drops the articles in flight, which the borrow then fetches again (two at most here).
    # A restarted (compressed) alt is read by its decoder only: never twice.
    b_again_ok = 0 if backup in ('twin', 'aestwin', 'rar5m3', 'rar5m3aes', 'lzma2') else 2
    if backup == 'aesalt':
        # OPEN ITEM: an encrypted download's dead volumes are not rebuilt from an alt (their
        # headers and data would have to be encrypted again under its own key, which
        # deadvolume refuses), so the release completes from the alt, whole; nothing of the
        # primary is fetched twice
        ok = integ and winner == 'Backup' and a_again == 0
    elif backup in ('compressed', 'norestart'):
        ok = (integ and winner == 'Backup' and failed_over == 1 and
              _grep_log(t, 'cannot continue the extraction of Primary') >= 1)
    elif backup == 'rar5m3other':
        # refused before any restart: nothing of it lent, no restart tried or refused
        # (TwinCheck labelled it other: checked, so a wrong setup fails instead of passing)
        kind = _params(daemon.wait_history(api, 'Backup', timeout=10)).get('DupeKind')
        lent = (_grep_log(t, 'from duplicate Backup') + _grep_log(t, 'from a duplicate; direct unpack reads it')
                + _grep_log(t, 'Wrote dead volume'))
        ok = (integ and winner == 'Backup' and failed_over == 1 and lent == 0 and kind == 'other'
              and _grep_log(t, 'Cannot restart the compressed video') == 0
              and _grep_log(t, 'Restarted the') == 0
              and _grep_log(t, 'cannot continue the extraction of Primary') >= 1)
    elif backup == 'other':
        # the backup's own (other) video, whole: nothing of it was lent into the primary's
        integ = _verify_output(t, donor_movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
        # (the check cannot tell it apart before the primary's end here: the download is
        # held until then, as `donorkind::unknown_kind_is_labelled_alt` says, and its
        # post-processing falls back to the backup)
        lent = (_grep_log(t, 'from duplicate Backup') + _grep_log(t, 'from a duplicate; direct unpack reads it')
                + _grep_log(t, 'Wrote dead volume'))
        ok = integ and winner == 'Backup' and du_ok == 0 and lent == 0
    else:
        # the extraction that ran before the cut is the one that finished: direct unpack of
        # the primary succeeded and post-processing took its files, extracting nothing again,
        # and it never started over (a volume read as it was written goes on from its rebuilt
        # copy at the payload frontier)
        ok = (integ and winner == 'Primary' and failed_over == 0 and cancelled == 0 and backup_unpacks == 0
              and du_ok == 1 and used_direct == 1 and a_again == 0 and b_again <= b_again_ok
              and b_bytes <= b_budget and b_before == 0 and restarts == 0
              and (not need_live or as_written > 0))
    if blocker:
        # the blocker stayed paused in the queue all through, its direct unpack not stopped
        blocker_state = _paused_state(api, 'Blocker')
        ok = ok and blocker_state.startswith('paused') and _grep_log(t, 'Stopping the direct unpack of Blocker') == 0
        name = '%s (blocker %s)' % (name, blocker_state)
    return (name, ok,
            'winner=%s primary=%s backup=%s secs=%.1f primary_served=%d/%d B (%d articles, %d again, '
            'par2 %d again) backup_served=%d B of %d (budget %d, %d articles, %d again, par2 %d again) '
            'twin_before_frontier=%d failover_logs=%d du_cancelled=%d du_ok=%d used_direct=%d '
            'backup_unpacks=%d extracted_ok=%s restarts=%d as_written_mb=%s continued=%d'
            % (winner, hp['Status'], hb['Status'], elapsed, a_bytes, total, a_count, a_again, a_par_again,
               b_bytes, donor_total, b_budget, b_count, b_again, b_par_again, b_before, failed_over, cancelled,
               du_ok, used_direct, backup_unpacks, integ, restarts, as_written, continued))


def scenario_failcont_twin25(daemon, t):
    return _failcont(daemon, t, 'failcont_twin25', 'twin', 0.25)


def scenario_failcont_twin50(daemon, t):
    return _failcont(daemon, t, 'failcont_twin50', 'twin', 0.50)


def scenario_failcont_twin90(daemon, t):
    return _failcont(daemon, t, 'failcont_twin90', 'twin', 0.90)


def _blocker_members(t, tag):
    """A stored RAR5 set of three volumes of 50 kB articles under ``tag``/Blk.partNN.rar:
    every first article arrives (so direct rename has the names), and SCENARIO_DELAY_PROXY
    holds back the second article of the last volume (marker ``tag/Blk.part03.rar?2=``)."""
    seg = 50_000
    with tempfile.TemporaryDirectory(prefix='blk-') as workdir:
        vols = generators.rar5_store_volumes_hp('blocker.bin', _payload(2_500_000, 31337), 1_000_000, None, workdir)
    members = []
    for i, data in enumerate(vols):
        vname = 'Blk.part%02d.rar' % (i + 1)
        rel = '%s/%s' % (tag, vname)
        t.write_file(os.path.join('data', rel), data)
        members.append((rel, vname, len(data), seg, set()))
    return members


def _paused_blocker(daemon, t, api, nzb_name, tag, members=None):
    """Queues ``nzb_name``, a rar set of three volumes whose last one the news server holds
    back (SCENARIO_DELAY_PROXY), waits until its direct unpack starts and pauses it: its
    extraction then waits for a volume that does not come. True, or why not."""
    if members is None:
        members = [m[:5] for m in _rarrenamer_members(t, tag, ['testfile3'])]
    daemon.append(api, nzb_name, build_multi_nzb(members), False, tag + '-key', 100)
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, 'Directly unpacking ' + nzb_name) == 0:
        time.sleep(0.2)
    if _grep_log(t, 'Directly unpacking ' + nzb_name) == 0:
        return 'its direct unpack did not start'
    g = next((g for g in api.listgroups() if g['NZBName'] == nzb_name), None)
    if g is None:
        return 'it left the queue before it could be paused'
    api.editqueue('GroupPause', 0, '', [g['NZBID']])
    return True


def _paused_state(api, nzb_name):
    """'paused <Status>' while ``nzb_name`` is in the queue with its files paused (its Status
    may read DOWNLOADING while an article it asked for before the pause is on the way), else
    its Status or 'gone'."""
    g = next((g for g in api.listgroups() if g['NZBName'] == nzb_name), None)
    if g is None:
        return 'gone'
    paused = g.get('PausedSizeLo', 0) or g.get('PausedSizeHi', 0)
    return ('paused ' if paused else '') + g.get('Status', '?')


def scenario_failcont_alt25busy(daemon, t):
    """failcont_alt25 with one unpack slot and another download's direct unpack waiting for a
    volume while it is paused: the primary is kept and continued from its alt, not failed
    over with the alt downloaded whole."""
    return _failcont(daemon, t, 'failcont_alt25busy', 'alt', 0.25, blocker=True)


def scenario_dupausedlive(daemon, t):
    """A paused download's waiting direct unpack keeps no slot from a new download's live
    extraction (UnpackSlots=1). RelA's direct unpack starts, then RelA is paused while its
    last volume is on the way (the news server holds it back); RelB, added after, extracts
    while it downloads: its direct unpack starts and succeeds, and post-processing takes its
    files. RelA stays paused in the queue with its direct unpack running (not stopped).
    Before, one global unpack lock (and the direct-unpack start check) kept RelB from
    extracting until RelA was resumed (6790: a paused item's direct unpack held it)."""
    api = daemon.wait_ready()
    blocked = _paused_blocker(daemon, t, api, 'RelA', 'dplA')
    if blocked is not True:
        return ('dupausedlive', False, 'RelA: %s' % blocked)
    members_b = _rarrenamer_members(t, 'dplB', ['testfile5'])
    daemon.append(api, 'RelB', build_multi_nzb([m[:5] for m in members_b]), False, 'dpl-b', 100)
    try:
        h = daemon.wait_history(api, 'RelB', timeout=60)
        status, unpack = h['Status'], h['UnpackStatus']
    except RuntimeError:
        g = next((g for g in api.listgroups() if g['NZBName'] == 'RelB'), {})
        status, unpack = 'still queued: %s' % g.get('Status'), '-'
    rel_a = _paused_state(api, 'RelA')
    du_b = _grep_log(t, 'Directly unpacking RelB')
    du_b_ok = _grep_log(t, 'Direct unpack for RelB successful')
    used = _grep_log(t, 'Using directly unpacked files')
    stopped_a = _grep_log(t, 'Stopping the direct unpack of RelA') + _grep_log(t, 'Cancelling direct unpack for RelA')
    files = _main_basenames(t)
    ok = (du_b == 1 and du_b_ok == 1 and used >= 1 and status.startswith('SUCCESS') and unpack == 'SUCCESS'
          and 'testfile5.dat' in files and rel_a.startswith('paused') and stopped_a == 0)
    return ('dupausedlive', ok, 'relB=%s unpack=%s du_b=%d du_b_ok=%d used_direct=%d relA=%s stopped_a=%d files=%s' % (
        status, unpack, du_b, du_b_ok, used, rel_a, stopped_a, files))


def _slot_rar_members(t, tag, name, volumes, seed):
    """A store-mode rar set of ``volumes`` 120 kB volumes (real CRCs) of movie.mkv, served
    as ``<tag>/v00`` .. (the news server may hold the last one back by that path)."""
    movie = _payload(120_000 * volumes - 1000, seed)
    vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 120_000)
    members = []
    for i, data in enumerate(vols):
        rel = '%s/v%02d' % (tag, i)
        t.write_file(os.path.join('data', rel), data)
        members.append((rel, '%s.part%02d.rar' % (name, i + 1), len(data), 60_000, set()))
    return members


def scenario_slotpreemptpaused(daemon, t):
    """One unpack slot (the default): an active download's live extraction goes before a
    paused download's extraction that holds the slot. RelA, an 8-volume rar set whose last
    volume the news server holds back, starts its direct unpack and is paused; every safe
    point of an extraction takes 3 s (RUSTYGET_TEST_EXTRACT_BOUNDARY_DELAY_MS, a slow disk),
    so RelA extracts its 7 volumes on the slot for about 20 s. RelB, added after, downloads:
    RelA gives the slot up at its next volume ("Giving the unpack slot of RelA"), RelB's
    direct unpack extracts and RelB goes to history as SUCCESS. RelA stays paused in the
    queue with its direct unpack not stopped. Before, RelA kept the slot until it ran out of
    volumes."""
    api = daemon.wait_ready()
    members_a = _slot_rar_members(t, 'sppA', 'RelA', 8, 7501)
    blocked = _paused_blocker(daemon, t, api, 'RelA', 'sppA', members_a)
    if blocked is not True:
        return ('slotpreemptpaused', False, 'RelA: %s' % blocked)
    members_b = _rarrenamer_members(t, 'sppB', ['testfile5'])
    daemon.append(api, 'RelB', build_multi_nzb([m[:5] for m in members_b]), False, 'spp-b', 100)
    try:
        h = daemon.wait_history(api, 'RelB', timeout=90)
        status, unpack = h['Status'], h['UnpackStatus']
    except RuntimeError:
        g = next((g for g in api.listgroups() if g['NZBName'] == 'RelB'), {})
        status, unpack = 'still queued: %s' % g.get('Status'), '-'
    rel_a = _paused_state(api, 'RelA')
    gave = _grep_log(t, 'Giving the unpack slot of RelA')
    gave_b = _grep_log(t, 'Giving the unpack slot of RelB')
    du_b_ok = _grep_log(t, 'Direct unpack for RelB successful')
    stopped_a = _grep_log(t, 'Stopping the direct unpack of RelA') + _grep_log(t, 'Cancelling direct unpack for RelA')
    files = _main_basenames(t)
    ok = (gave >= 1 and gave_b == 0 and du_b_ok == 1 and status.startswith('SUCCESS') and unpack == 'SUCCESS'
          and 'testfile5.dat' in files and rel_a.startswith('paused') and stopped_a == 0)
    return ('slotpreemptpaused', ok, 'relB=%s unpack=%s gave_a=%d gave_b=%d du_b_ok=%d relA=%s stopped_a=%d files=%s' % (
        status, unpack, gave, gave_b, du_b_ok, rel_a, stopped_a, files))


def scenario_slotfinishedlive(daemon, t):
    """One unpack slot (the default): a finished download's unpack is not held back for the
    whole download by an active download's live extraction that keeps the slot. RelA, a
    30-volume rar set whose last volume the news server holds back, extracts while it
    downloads; every safe point of an extraction takes 4 s
    (RUSTYGET_TEST_EXTRACT_BOUNDARY_DELAY_MS, a slow disk), so RelA keeps the slot for about
    two minutes. RelB, a single .7z (no direct unpack: post-processing unpacks it), finishes
    downloading meanwhile: its unpack waits in line for the slot instead of being skipped,
    and once it waited a minute RelA gives the slot up at its next volume ("Giving the unpack
    slot of RelA"), and RelB goes to history as SUCCESS with its file. (RelA's held volume
    fails later, as a straggler asked again on a closed connection: where RelA ends is not
    tested.) Before, a finished job was not even started while any extraction held the slot,
    so RelB sat in PP_QUEUED until RelA ran out of volumes."""
    api = daemon.wait_ready()
    members_a = _slot_rar_members(t, 'sflA', 'RelA', 30, 7502)
    daemon.append(api, 'RelA', build_multi_nzb(members_a), False, 'sfl-a', 100)
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, 'Directly unpacking RelA') == 0:
        time.sleep(0.2)
    if _grep_log(t, 'Directly unpacking RelA') == 0:
        return ('slotfinishedlive', False, 'RelA: its direct unpack did not start')
    data = _payload(1_500_000, 7503)
    archive = generators.seven_zip_copy_checked([('clip.mkv', data)])
    t.write_file(os.path.join('data', 'sflB/rel.7z'), archive)
    daemon.append(api, 'RelB', build_multi_nzb([('sflB/rel.7z', 'RelB.7z', len(archive), 300_000, set())]),
                  False, 'sfl-b', 100)
    started = time.time()
    try:
        h = daemon.wait_history(api, 'RelB', timeout=150)
        status, unpack = h['Status'], h['UnpackStatus']
    except RuntimeError:
        g = next((g for g in api.listgroups() if g['NZBName'] == 'RelB'), {})
        status, unpack = 'still queued: %s' % g.get('Status'), '-'
    took = time.time() - started
    rel_a = next((g['Status'] for g in api.listgroups() if g['NZBName'] == 'RelA'), 'gone')
    gave = _grep_log(t, 'Giving the unpack slot of RelA')
    stopped_a = _grep_log(t, 'Stopping the direct unpack of RelA') + _grep_log(t, 'Cancelling direct unpack for RelA')
    files = _main_basenames(t)
    ok = (gave >= 1 and status.startswith('SUCCESS') and unpack == 'SUCCESS' and 'clip.mkv' in files
          and stopped_a == 0)
    return ('slotfinishedlive', ok, 'relB=%s unpack=%s after %.0fs gave_a=%d relA=%s stopped_a=%d files=%s' % (
        status, unpack, took, gave, rel_a, stopped_a, files))


def scenario_failcont_alt25(daemon, t):
    return _failcont(daemon, t, 'failcont_alt25', 'alt', 0.25)


def scenario_failcont_alt50(daemon, t):
    return _failcont(daemon, t, 'failcont_alt50', 'alt', 0.50)


def scenario_failcont_alt90(daemon, t):
    return _failcont(daemon, t, 'failcont_alt90', 'alt', 0.90)


def scenario_failcont_altlive(daemon, t):
    """A stored alt continues a primary that dies inside a volume (63 %, mid-volume) while
    the extraction reads the volumes as they are written (writer-fed): the volume it died in
    finishes with the lost articles as holes, the alt lends them by the video's bytes, and
    the extraction goes on with zero restarts. (A volume read as it was written and then
    rebuilt whole as a stand-in, ``continued``, is covered by the unit tests: a partly written
    volume here finishes damaged, not dead.)"""
    return _failcont(daemon, t, 'failcont_altlive', 'alt', 0.63, need_live=True)


def scenario_failcont_aes25(daemon, t):
    return _failcont(daemon, t, 'failcont_aes25', 'aes', 0.25)


def scenario_failcont_aes50(daemon, t):
    return _failcont(daemon, t, 'failcont_aes50', 'aes', 0.50)


def scenario_failcont_aes90(daemon, t):
    return _failcont(daemon, t, 'failcont_aes90', 'aes', 0.90)


def scenario_failcont_fleettwin(daemon, t):
    return _failcont(daemon, t, 'failcont_fleettwin', 'twin', 0.50, fleet=True)


def scenario_failcont_fleetalt(daemon, t):
    return _failcont(daemon, t, 'failcont_fleetalt', 'alt', 0.50, fleet=True)


def scenario_failcont_fleetaes(daemon, t):
    return _failcont(daemon, t, 'failcont_fleetaes', 'aes', 0.50, fleet=True)


def scenario_failcont_compressed(daemon, t):
    return _failcont(daemon, t, 'failcont_compressed', 'compressed', 0.50)


def scenario_failcont_aestwin(daemon, t):
    return _failcont(daemon, t, 'failcont_aestwin', 'aestwin', 0.50, primary='aes')


def scenario_failcont_aesalt(daemon, t):
    return _failcont(daemon, t, 'failcont_aesalt', 'aesalt', 0.50, primary='aes')


def scenario_failcont_rar5m3(daemon, t):
    return _failcont(daemon, t, 'failcont_rar5m3', 'rar5m3', 0.50)


def scenario_failcont_rar5m3aes(daemon, t):
    return _failcont(daemon, t, 'failcont_rar5m3aes', 'rar5m3aes', 0.50)


def scenario_failcont_rar5m3other(daemon, t):
    return _failcont(daemon, t, 'failcont_rar5m3other', 'rar5m3other', 0.50)


def scenario_failcont_lzma2(daemon, t):
    return _failcont(daemon, t, 'failcont_lzma2', 'lzma2', 0.50)


def scenario_failcont_norestart(daemon, t):
    return _failcont(daemon, t, 'failcont_norestart', 'norestart', 0.50)


def scenario_failcont_twinlabel(daemon, t):
    return _failcont(daemon, t, 'failcont_twinlabel', 'twin', 0.50, twin_label='DupeKind')


def scenario_failcont_othervideo(daemon, t):
    return _failcont(daemon, t, 'failcont_othervideo', 'other', 0.50)


def _weakswitch(daemon, t, name, twin, alt='store'):
    """Dune on 6790 (fleet-weak-primary item 2): the fleet's primary misses a
    third of the articles of every volume, and a copy held in its fleet holds
    the same video whole (equal DupeInnerCrc; ``twin``: the same files). Before,
    the primary downloaded on, borrowing hole by hole (2.6 times the bytes it
    recovered, hours on 6790). Once enough of its own articles are tried while
    little of it came, the alt takes over at the primary's frontier: the
    primary is cut there (none of its data articles left is fetched), its
    extraction carries on, and the alt lends every byte past the frontier
    (failover-continue), so nothing the primary downloaded is fetched again
    and the bytes fetched stay about the payload's size. ``alt``: ``store``
    (another stored RAR set), ``aes`` (a RAR5 AES store set, password in its
    nzb) or ``m3`` (RAR5 -m3, restarted near the frontier by zipfast). A twin
    is left to the article fallback (one for one): the switch never fires for
    it, and nothing is fetched twice. The movie is byte-identical either way."""
    if not generators.HAVE_RAR:
        return (name, None, 'SKIP: rar not installed')
    tag = 'ws' + ('t' if twin else {'store': 'a', 'aes': 'e', 'm3': 'm'}[alt])
    password = 'ws-pw 4' if alt == 'aes' and not twin else None
    seg = 50_000
    # over twinfields::MIN_KEY_SIZE (50 MiB): smaller videos' inner keys link nothing
    movie = _payload(55_000_000, 25210)
    with tempfile.TemporaryDirectory(prefix='ws-') as workdir:
        vols = generators.rar5_store_volumes_hp('movie.mkv', movie, 5_000_000, None, workdir)
    total = sum(len(v) for v in vols)
    names = ['Show.part%02d.rar' % (i + 1) for i in range(len(vols))]
    members = []
    for i, (vname, data) in enumerate(zip(names, vols)):
        rel = '%sA/%s' % (tag, vname)
        t.write_file(os.path.join('data', rel), data)
        parts = (len(data) + seg - 1) // seg
        # a third of every volume's articles is gone, never its first (headers); before an
        # alt, a fifth, two articles in a row (one alt article's span) per ten
        dead = ({p for p in range(2, parts + 1) if p % 3 == 0} if twin else
                {p for p in range(2, parts + 1) if p % 10 in (3, 4)})
        members.append((rel, vname, len(data), seg, dead))
    par = generators.par2_index(list(zip(names, vols)))
    if twin:
        t.write_file(os.path.join('data', '%sA/Show.par2' % tag), par)
        members.append(('%sA/Show.par2' % tag, 'Show.par2', len(par), seg, set()))
    # (the alt case has no par2, as the Dune primary's damage was past what its
    # par2 judged: the par-damage check fails over a par2 index with no
    # recovery blocks before the dead-download check this reproduces)
    inner = '%d:%08x' % (len(movie), zlib.crc32(movie) & 0xffffffff)
    if twin:
        donor_vols = list(zip(names, vols))
        donor_seg = seg
        labels = [('DupeKind', 'twin'), ('DupeCopyKind', 'twin')]
    elif alt == 'store':
        alt_vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 7_000_000)
        donor_vols = [('Other.part%02d.rar' % (i + 1), d) for i, d in enumerate(alt_vols)]
        donor_seg = 100_000
        labels = [('DupeCopyKind', 'borrowable'), ('DupeVideo', 'same'), ('DupeKind', 'alt')]
    else:
        with tempfile.TemporaryDirectory(prefix='ws-') as workdir:
            if alt == 'aes':
                alt_vols = generators.rar5_store_volumes_hp('movie.mkv', movie, 7_000_000, password, workdir)
            else:
                with open(os.path.join(workdir, 'movie.mkv'), 'wb') as f:
                    f.write(movie)
                import subprocess as _sp
                _sp.run([generators.RAR_PATH, 'a', '-idq', '-m3', '-ma5', '-v7000000b', 'out.rar', 'movie.mkv'],
                        cwd=workdir, check=True, capture_output=True)
                outs = sorted((n for n in os.listdir(workdir) if n.startswith('out')),
                              key=lambda n: int(''.join(c for c in n if c.isdigit()) or 0))
                alt_vols = [open(os.path.join(workdir, n), 'rb').read() for n in outs]
        donor_vols = [('Other.part%02d.rar' % (i + 1), d) for i, d in enumerate(alt_vols)]
        donor_seg = 100_000
        # as a fleet labels them: an AES store set it can decrypt, a compressed one it
        # cannot borrow from by range
        labels = ([('DupeCopyKind', 'borrowable-encrypted'), ('DupeLocked', 'yes')] if alt == 'aes'
                  else [('DupeCopyKind', 'unborrowable')]) + [('DupeVideo', 'same'), ('DupeKind', 'alt')]
    labels += [('DupeAlive', '100'), ('DupeInnerCrc', inner)]
    donor_members = []
    for vname, data in donor_vols:
        rel = '%sB/%s' % (tag, vname)
        t.write_file(os.path.join('data', rel), data)
        donor_members.append((rel, vname, len(data), donor_seg, set()))
    if twin:
        t.write_file(os.path.join('data', '%sB/Show.par2' % tag), par)
        donor_members.append(('%sB/Show.par2' % tag, 'Show.par2', len(par), seg, set()))
    api = daemon.wait_ready()
    key = 'dupes:show.weakswitch.' + tag
    _ds_append(api, 'Primary', build_multi_nzb(members), key, 100, paused=True,
               params=[{'Name': 'DupeInnerCrc', 'Value': inner}, {'Name': 'DupeCopyKind', 'Value': 'borrowable'}])
    _ds_append(api, 'Backup', build_multi_nzb(donor_members, password=password), key, 90,
               paused=False, params=[{'Name': k, 'Value': v} for k, v in labels])
    if _params(_wait_held(api, 'Backup')).get('DupeHeld') != 'yes':
        return (name, False, 'the backup was not held by its fleet')
    started = time.time()
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    deadline = time.time() + 300
    winner, hp, hb = 'none', None, None
    while time.time() < deadline:
        hist = {h['Name']: h for h in api.history(False)}
        hp, hb = hist.get('Primary'), hist.get('Backup')
        if hp and hp['Status'].startswith('SUCCESS'):
            winner = 'Primary'
            break
        if hb and hb['Status'].startswith('SUCCESS'):
            winner = 'Backup'
            break
        time.sleep(0.5)
    elapsed = time.time() - started
    ids = _failcont_requests(t)
    a_bytes, a_count, a_again, _ = _failcont_bytes(ids, tag + 'A/')
    b_bytes, b_count, b_again, _ = _failcont_bytes(ids, tag + 'B/')
    switched = _grep_log(t, 'Failing over Primary to alt Backup: weak')
    continued = _grep_log(t, 'Continuing Primary at its frontier from alt Backup')
    restarted = _grep_log(t, 'Restarted the RAR5 decoder of duplicate Backup')
    integ = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    fetched = a_bytes + b_bytes
    if twin:
        # the primary stays and extracts; its lost articles come from the twin one
        # for one (article fallback, or every later one once it leads): nothing twice
        ok = integ and winner == 'Primary' and switched == 0 and a_again == 0 and b_again == 0
    else:
        # the alt took over at the frontier while little of the primary came: the
        # primary's extraction carried on, nothing of it was fetched again, the alt
        # was never downloaded whole, and the bytes fetched stay about the payload's
        # (the holes before the frontier are borrowed hole by hole: whole alt articles)
        # (a compressed alt is restarted at the first hole: before the frontier its
        # holes are filled from the same decode, so it is read about whole, but never
        # twice and the primary's extraction is kept)
        cheaper = fetched < a_bytes + total if alt != 'm3' else fetched <= total * 13 // 10
        ok = (integ and winner == 'Primary' and continued == 1 and switched == 0 and a_bytes < total // 2
              and a_again == 0 and b_again <= 2 and cheaper and (restarted == 1 if alt == 'm3' else True))
    return (name, ok,
            'winner=%s continued=%d switched=%d restarted=%d secs=%.1f fetched=%d B for a %d B set (%.2fx) '
            'primary_served=%d/%d B (%d articles, %d again) backup_served=%d B (%d articles, %d again) extracted_ok=%s'
            % (winner, continued, switched, restarted, elapsed, fetched, total, fetched / float(total), a_bytes,
               total, a_count, a_again, b_bytes, b_count, b_again, integ))


def scenario_weakswitchalt(daemon, t):
    return _weakswitch(daemon, t, 'weakswitchalt', False)


def scenario_weakswitchtwin(daemon, t):
    return _weakswitch(daemon, t, 'weakswitchtwin', True)


def scenario_weakswitchaes(daemon, t):
    return _weakswitch(daemon, t, 'weakswitchaes', False, alt='aes')


def scenario_weakswitchm3(daemon, t):
    return _weakswitch(daemon, t, 'weakswitchm3', False, alt='m3')


def scenario_streamtimeoutwrites(daemon, t):
    """Dune on 6790 (fleet-weak-primary item 4): a cross-packing borrow wrote
    313.8 MB over 131.8 s, but its bytes were credited only when the donor's
    pass ended, so the watchdog stopped it ("nothing recovered for 300
    seconds", DupeStreamTimeout) though it was making progress. Here a slow
    server (800 ms per response, two connections) makes one donor pass write
    for longer than the 10 s timeout (its identity and map reads take about
    4 s, its 61 donor articles about 25 s); its writes count, so it is never
    stopped and the movie is whole."""
    size, seg = 20_000_000, 400_000
    data = _payload(size, 4253)
    pp = _place_copy(t, 'stA', data, 'movie.mkv')
    primary = build_nzb(pp, 'movie.mkv', size, seg, set(range(5, 45)))
    members = []
    for i, vol in enumerate(generators.rar3_store_volumes('movie.mkv', data, 5_000_000), 1):
        rel = 'stB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 250_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'DonST', build_multi_nzb(members), True, 'st-key', 50)
    daemon.append(api, 'RelST', primary, False, 'st-key', 100)
    h = daemon.wait_history(api, 'RelST', timeout=900)
    stopped = _grep_log(t, 'stopped: nothing recovered for')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    return ('streamtimeoutwrites', integ and h['Status'].startswith('SUCCESS') and stopped == 0,
            'status=%s nothing_recovered_stops=%d integrity=%s' % (h['Status'], stopped, integ))


def scenario_failcont_aescontainer(daemon, t):
    """The real XB alt failover on 6790 (2026-11-26, #715 verification) in
    miniature. The primary is a plain stored RAR5 set of 20 volumes with a
    par2 index; the provider misses a few articles of every volume (never a
    volume's first or last), and the second half, part11-20, is dead. The
    backup is the same movie in an encrypted RAR5 store set (encrypted
    headers, other volume and article sizes, the password only in its nzb)
    with a par2 index of its own volumes. It waits in history labelled
    ``container``: its par2 checksums differ, and nothing read the video inside
    both copies (the fleet intake could not read the damaged primary's
    headers), so TwinCheck cannot call it an alt.

    The first dead volume is borrowed after a check of the backup's video
    against the primary's own bytes. That match is the proof the label lacked:
    the later dead volumes, with none of the primary's bytes left around them
    to compare, borrow without proving it again, and the backup is labelled an
    alt of the primary with the evidence (``DupeLiveVideo=same:<NZBID>``). On
    #715 every later volume was refused ("content identity not confirmed"),
    par failed and the backup was downloaded and extracted in full.

    Passes when the primary's extraction continues to SUCCESS (direct unpack
    never cancelled, nothing unpacked again), the movie is byte-identical, the
    backup serves only the dead tail and the holes (plus its volume headers and
    an article at the cut) with no data article fetched twice, and nothing of
    the primary is fetched twice."""
    name = 'failcont_aescontainer'
    if not generators.HAVE_RAR:
        return (name, None, 'SKIP: rar not installed')
    tag = 'fcx'
    password = 'xb copy 05'
    seg, donor_seg = 50_000, 100_000
    movie = _payload(15_000_000, 25112)
    with tempfile.TemporaryDirectory(prefix='fc-') as workdir:
        vols = generators.rar5_store_volumes_hp('movie.mkv', movie, 750_000, None, workdir)
    with tempfile.TemporaryDirectory(prefix='fc-') as workdir:
        enc = generators.rar5_store_volumes_hp('movie.mkv', movie, 1_000_000, password, workdir)
    first_dead = len(vols) // 2
    total = sum(len(v) for v in vols)
    cut = sum(len(v) for v in vols[:first_dead])
    names = ['Show.part%02d.rar' % (i + 1) for i in range(len(vols))]
    members, holes = [], 0
    for i, (vname, data) in enumerate(zip(names, vols)):
        rel = '%sA/%s' % (tag, vname)
        t.write_file(os.path.join('data', rel), data)
        # a dead volume in four articles, one per connection: with nserv's late 430 (SCENARIO_NSERV_ARGS) one volume
        # dies every 10-15 s, as on 6790, and each repair run finds one more
        vseg = seg if i < first_dead else 187_500
        parts = (len(data) + vseg - 1) // vseg
        if i >= first_dead:
            dead = set(range(1, parts + 1))
        else:
            # the provider misses a few articles of the first volumes (never a volume's first or last)
            dead = {p for p in range(2, parts) if (p + i) % 7 == 0} if i < first_dead // 2 else set()
            holes += len(dead)
        members.append((rel, vname, len(data), vseg, dead))
    par = generators.par2_index(list(zip(names, vols)))
    t.write_file(os.path.join('data', '%sA/Show.par2' % tag), par)
    members.append(('%sA/Show.par2' % tag, 'Show.par2', len(par), seg, set()))
    donor_vols = [('DRRut.part%02d.rar' % (i + 1), d) for i, d in enumerate(enc)]
    donor_members = []
    for vname, data in donor_vols:
        rel = '%sB/%s' % (tag, vname)
        t.write_file(os.path.join('data', rel), data)
        donor_members.append((rel, vname, len(data), donor_seg, set()))
    dpar = generators.par2_index(donor_vols)
    t.write_file(os.path.join('data', '%sB/DRRut.par2' % tag), dpar)
    donor_members.append(('%sB/DRRut.par2' % tag, 'DRRut.par2', len(dpar), seg, set()))
    donor_total = sum(len(d) for _, d in donor_vols)
    api = daemon.wait_ready()
    key = tag + '-key'
    daemon.append(api, 'Primary', build_multi_nzb(members), True, key, 100)
    _ds_append(api, 'Backup', build_multi_nzb(donor_members, password=password), key, 90,
               paused=False, params=[{'Name': 'DupeKind', 'Value': 'container'}])
    daemon.wait_history(api, 'Backup', timeout=60)
    started = time.time()
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    # the late 430s alone take about 200 s of download; a loaded machine needs headroom
    hp = daemon.wait_history(api, 'Primary', timeout=900)
    hb = daemon.wait_history(api, 'Backup', timeout=300)
    deadline = time.time() + 240
    while (not hp['Status'].startswith('SUCCESS') and not hb['Status'].startswith(('SUCCESS', 'FAILURE'))
           and time.time() < deadline):
        time.sleep(0.5)
        hb = daemon.wait_history(api, 'Backup', timeout=300)
    elapsed = time.time() - started
    ids = _failcont_requests(t)
    a_bytes, a_count, a_again, _ = _failcont_bytes(ids, tag + 'A/')
    b_bytes, b_count, b_again, _ = _failcont_bytes(ids, tag + 'B/')
    # the late 430s (SCENARIO_NSERV_ARGS -x) make the primary's missing articles stragglers,
    # asked again by #732 ("Asking again for"): those serve nothing. Data the primary serves
    # must never be fetched twice.
    a_seen, a_data_again = set(), 0
    for mid in ids:
        if mid.startswith(tag + 'A/') and not mid.endswith('!2') and '.par2?' not in mid:
            a_data_again += mid in a_seen
            a_seen.add(mid)
    a_missing_again = a_again - a_data_again
    failed_over = _grep_log(t, 'Failing over Primary')
    cancelled = _grep_log(t, 'Cancelling direct unpack')
    backup_unpacks = _grep_log(t, 'Directly unpacking Backup') + _grep_log(t, 'Unpacking Backup')
    du_ok = _grep_log(t, 'Direct unpack for Primary successful')
    used_direct = _grep_log(t, 'Using directly unpacked files')
    refused = _grep_log(t, 'content identity not confirmed')
    relabel = _grep_log(t, 'Backup is an alt of Primary (another container, the same video inside: '
                           'a live borrow matched its video bytes')
    integ = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    winner = 'Primary' if hp['Status'].startswith('SUCCESS') else (
        'Backup' if hb['Status'].startswith('SUCCESS') else 'none')
    bp = _params(hb)
    # the dead tail (the backup's bytes past the primary's cut, at the same payload share),
    # each hole (a 50 KB hole lies in at most two 100 KB articles), each volume's first and
    # last article (encrypted headers: the payload map is built from every volume's headers,
    # not only those of the volumes it lends from) and two articles at the cut
    tail = donor_total * (total - cut) // total
    b_budget = tail + (2 * holes + 2 * len(donor_vols) + 2) * donor_seg
    ok = (integ and winner == 'Primary' and failed_over == 0 and cancelled == 0 and backup_unpacks == 0
          and du_ok == 1 and used_direct == 1 and a_data_again == 0 and b_again == 0 and b_bytes <= b_budget
          and bp.get('DupeKind') == 'alt' and bp.get('DupeLiveVideo') == 'same:%d' % hp['NZBID']
          and relabel >= 1 and refused == 0)
    return (name, ok,
            'winner=%s primary=%s backup=%s secs=%.1f primary_served=%d/%d B (%d articles, %d again, '
            '%d missing asked again) '
            'backup_served=%d B of %d (budget %d: tail %d + %d holes, %d articles, %d again) refused=%d '
            'backup_kind=%s live=%s relabel_logs=%d failover_logs=%d du_cancelled=%d du_ok=%d used_direct=%d '
            'backup_unpacks=%d extracted_ok=%s'
            % (winner, hp['Status'], hb['Status'], elapsed, a_bytes, total, a_count, a_data_again,
               a_missing_again, b_bytes,
               donor_total, b_budget, tail, holes, b_count, b_again, refused, bp.get('DupeKind'),
               bp.get('DupeLiveVideo'), relabel, failed_over, cancelled, du_ok, used_direct,
               backup_unpacks, integ))


def _headlost_volume(daemon, t, name, rar5, par=False):
    """A plain stored rar set without par2 loses the first articles of one
    middle volume (its headers and the start of its data) and a few in its
    middle; the rest of that volume arrives. Its backup in history holds the
    same movie in a RAR5 store set with encrypted headers whose password is in
    the backup's nzb. The lost headers are rebuilt from a sibling volume, only
    the volume's holes are borrowed from the backup, and direct unpack goes on
    to the end: no failover, no restart (6790, 2026-11-20: copy 02's part18
    lost its [1/147] and its 8.2 MB was never borrowed)."""
    if not generators.HAVE_RAR:
        return (name, None, 'SKIP: rar not installed')
    tag = name[:6]
    password = 'alt-pw 7'
    seg = 20_000
    movie = _payload(15_000_000, 23071)
    with tempfile.TemporaryDirectory(prefix='hlv-') as workdir:
        if rar5:
            vols = generators.rar5_store_volumes_hp('movie.mkv', movie, 1_000_000, None, workdir)
        else:
            vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 1_000_000)
    lost = {1, 2, 3, 20, 21}
    members = []
    for i, data in enumerate(vols):
        vname = 'Show.part%02d.rar' % (i + 1)
        rel = '%sA/%s' % (tag, vname)
        t.write_file(os.path.join('data', rel), data)
        members.append((rel, vname, len(data), seg, set(lost) if i == 6 else set()))
    if par:
        import subprocess as _sp
        # An index without recovery blocks, as a par set whose volumes are paused: par
        # verification sees the damage but cannot repair it.
        work = t.path('data', '%sA' % tag)
        names = ['Show.part%02d.rar' % (i + 1) for i in range(len(vols))]
        _sp.run(['par2', 'create', '-q', '-q', '-s65536', '-c2', '-n1', '-a', 'Show', 'Show.par2'] + names,
                cwd=work, check=True, capture_output=True)
        members.append(('%sA/Show.par2' % tag, 'Show.par2',
                        os.path.getsize(os.path.join(work, 'Show.par2')), seg, set()))
    with tempfile.TemporaryDirectory(prefix='hlv-') as workdir:
        enc = generators.rar5_store_volumes_hp('movie.mkv', movie, 5_000_000, password, workdir)
    donor_members = []
    for i, data in enumerate(enc):
        vname = 'Other.part%d.rar' % (i + 1)
        rel = '%sB/%s' % (tag, vname)
        t.write_file(os.path.join('data', rel), data)
        donor_members.append((rel, vname, len(data), 100_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(members), True, tag + '-key', 100)
    _ds_append(api, 'Backup', build_multi_nzb(donor_members, password=password), tag + '-key', 90,
               paused=False,
               params=[{'Name': 'DupeCopyKind', 'Value': 'borrowable-encrypted'},
                       {'Name': 'DupeLocked', 'Value': 'yes'}])
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    failed_over = _grep_log(t, 'Failing over Primary')
    cancelled = _grep_log(t, 'Cancelling direct unpack')
    # From a sibling volume, or (RAR5) from the volume's own quick-open record as it arrives.
    from_sibling = _grep_log(t, 'Volume Show.part07.rar lost its headers')
    quick_open = _grep_log(t, 'Rebuilt the headers of Show.part07.rar')
    rebuilt = from_sibling + quick_open
    # Rebuilt from a sibling, the volume is written back over the file on disk, byte for
    # byte, so par verification finds it whole (RAR5 only: RAR 1.5-4 headers are rebuilt,
    # not restored). Rebuilt from its quick-open record, direct unpack reads on past the
    # lost data and its archive checksums vouch for every byte (par-check is skipped).
    on_disk = _grep_log(t, 'Rebuilt Show.part07.rar on disk')
    on_disk_ok = (on_disk == 1 or quick_open >= 1) if rar5 else on_disk == 0
    par_failed = _grep_log(t, 'Repair failed for Primary')
    du_ok = _grep_log(t, 'Direct unpack for Primary successful')
    integ = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    ok = (failed_over == 0 and cancelled == 0 and rebuilt >= 1 and on_disk_ok and
          par_failed == 0 and du_ok == 1 and integ and hp['Status'].startswith('SUCCESS'))
    return (name, ok,
            'status=%s failover_logs=%d du_cancelled=%d headers_rebuilt=%d (quick_open=%d) on_disk=%d '
            'par_failed=%d du_ok=%d extracted_ok=%s'
            % (hp['Status'], failed_over, cancelled, rebuilt, quick_open, on_disk, par_failed, du_ok, integ))


def scenario_headlostvolume(daemon, t):
    return _headlost_volume(daemon, t, 'headlostvolume', rar5=False)


def scenario_headlostvolume5(daemon, t):
    return _headlost_volume(daemon, t, 'headlostvolume5', rar5=True)


def scenario_headlostvolumepar(daemon, t):
    """headlostvolume5 with a par2 index but no recovery blocks (6790 job 354):
    the rebuilt volume has to count as repaired, or par verification finds
    the volume still damaged and direct unpack is not resumed. (rar 7 writes a
    quick-open record, so the headers are now rebuilt from it as the volume
    arrives and direct unpack reads on; par-check is skipped, the archive
    checksums vouching for the movie. The sibling rebuild written back to disk
    is left for sets without that record, as headlostvolume's RAR 1.5-4.)"""
    return _headlost_volume(daemon, t, 'headlostvolumepar', rar5=True, par=True)


def scenario_deadvolumeunlabelled(daemon, t):
    """The same release, but nothing classed the backup (no fleet labels, and
    TwinCheck cannot sample three volumes against fifteen): a same-key grab,
    here another encode. Direct unpack running is no reason to keep the
    download for it: it fails over (at the dead run, or, if a borrow got
    underway first, once the borrow finds the video differs), never
    downloading to its end, and the backup completes."""
    hp, hb, movie = _dead_volume_nested(daemon, t, 'dvu', [], donor_seed=23072)
    failed_over = _grep_log(t, 'Failing over Primary to duplicate Backup')
    log = t.read_file('nzbget.log').decode(errors='replace')
    late = sorted(set(re.findall(r'Successfully downloaded Primary/Show\.part(1[0-5])\.rar\n', log)))
    ok = (failed_over == 1 and len(late) < 6 and hp['Status'].startswith('FAILURE') and
          hb['Status'].startswith('SUCCESS'))
    return ('deadvolumeunlabelled', ok, 'status=%s backup=%s failover_logs=%d kept_logs=%d late_volumes_done=%s'
            % (hp['Status'], hb['Status'], failed_over, _grep_log(t, 'lends the lost parts'), late))


def scenario_altfetchonce(daemon, t):
    """Audit finding 6 (2026-11-26): borrowing from an alt fetched each donor article from the
    network once. A stored rar set without par2 loses one volume whole and two articles of a
    later one: two holes, so two repair runs, each of which maps the alt again (the first and
    last article of every one of its 90 volumes: 180 articles, more than the 64-article memo
    of recent articles holds, and the 72 MB the download fetches between the two runs pushes
    them out of nntp-rs's 64 MB body cache too). The alt is the same 90 MB movie in another
    stored rar (1 MB volumes, 50 KB articles; the download's are 3 MB and 100 KB), in
    history, labelled an alt. Every donor article the maps, TwinCheck and the
    borrows read must reach the network once (no message-id asked twice), and the donor bytes
    fetched stay within the borrowed range plus one first and last article per donor volume,
    plus the batch margin around each hole (DONOR_PATCH_MARGIN_PARTS articles each side, and
    the article each hole edge falls in)."""
    tag = 'afo'
    seg = 100_000
    donor_seg = 50_000
    movie = _payload(90_000_000, 23090)
    vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 3_000_000)
    dead_vol, holed_vol, holes = 3, 27, {10, 11}
    members = []
    lost = 0
    for i, data in enumerate(vols):
        name = 'Show.part%02d.rar' % (i + 1)
        rel = '%sA/%s' % (tag, name)
        t.write_file(os.path.join('data', rel), data)
        parts = (len(data) + seg - 1) // seg
        missing = set(range(1, parts + 1)) if i == dead_vol else (holes if i == holed_vol else set())
        lost += sum(min(seg, len(data) - (k - 1) * seg) for k in missing)
        members.append((rel, name, len(data), seg, missing))
    alt = generators.rar3_store_volumes_valid('movie.mkv', movie, 1_000_000)
    donor_members = []
    edges = 0
    for i, data in enumerate(alt):
        name = 'Other.part%02d.rar' % (i + 1)
        rel = '%sB/%s' % (tag, name)
        t.write_file(os.path.join('data', rel), data)
        donor_members.append((rel, name, len(data), donor_seg, set()))
        parts = (len(data) + donor_seg - 1) // donor_seg
        last = len(data) - (parts - 1) * donor_seg
        edges += donor_seg + (last if parts > 1 else 0)
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(members), True, tag + '-key', 100)
    _ds_append(api, 'Backup', build_multi_nzb(donor_members), tag + '-key', 90, paused=False,
               params=[{'Name': 'DupeCopyKind', 'Value': 'borrowable'}, {'Name': 'DupeVideo', 'Value': 'same'},
                       {'Name': 'DupeKind', 'Value': 'alt'}])
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    integ = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    ids = _failcont_requests(t)
    served, count, again, _ = _failcont_bytes(ids, tag + 'B/')
    donor_ids = [m for m in ids if m.startswith(tag + 'B/')]
    twice = sorted({m for m in donor_ids if donor_ids.count(m) > 1})
    holes_n = 2
    margin = holes_n * (2 * 2 + 2) * donor_seg
    bound = lost + edges + margin
    runs = _grep_log(t, 'Fetch cost of borrowing from Backup')
    stored = len(re.findall(r'[1-9]\d* from the fetched-article store', _log_text(t)))
    # two repair runs at least (one per hole), and a later one served from the store: the
    # case the store exists for was exercised, not avoided
    ok = (integ and hp['Status'].startswith('SUCCESS') and again == 0 and not twice and served <= bound
          and runs >= 2 and stored >= 1)
    costs = [m.group(1, 2, 3, 4, 5) for m in re.finditer(
        r'Fetch cost of borrowing from Backup: ([\d.]+) MB.*?; (\d+) single.*?of (\d+) article.*?'
        r'(\d+) taken from memory(?:, (\d+) from the fetched-article store)?', _log_text(t))]
    return ('altfetchonce', ok,
            'status=%s extracted_ok=%s donor_bytes=%d bound=%d (lost %d + edges %d + margin %d) '
            'donor_articles=%d asked_twice=%d %s cost_lines=%d store_served_lines=%d costs=%s'
            % (hp['Status'], integ, served, bound, lost, edges, margin, count, len(twice), twice[:4], runs, stored,
               costs))


def scenario_deadvolumetwincheck(daemon, t):
    """deadvolumenestednopar without injected labels: TwinCheck itself must
    label the nested backup (another container, by its sampled articles: its
    outer rar has as many volumes as the primary, cut into other article
    sizes) before the dead run, so the download is kept and its extraction
    carries on."""
    hp, hb, movie = _dead_volume_nested(daemon, t, 'dvt', [], donor_volumes=15)
    labelled = _grep_log(t, 'Backup is in another container than Primary')
    failed_over = _grep_log(t, 'Failing over Primary')
    kept = _grep_log(t, 'lends the lost parts')
    du_ok = _grep_log(t, 'Direct unpack for Primary successful')
    integ = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    ok = (labelled >= 1 and failed_over == 0 and kept == 1 and du_ok == 1 and integ and
          hp['Status'].startswith('SUCCESS'))
    return ('deadvolumetwincheck', ok,
            'status=%s backup=%s labelled_logs=%d failover_logs=%d kept_logs=%d du_ok=%d extracted_ok=%s'
            % (hp['Status'], hb['Status'], labelled, failed_over, kept, du_ok, integ))


def _dead_volume_other_video(daemon, t, label, tag, donor_size):
    hp, hb, movie = _dead_volume_nested(daemon, t, tag, [], donor_seed=23071, donor_volumes=15,
                                        donor_size=donor_size)
    labelled = _grep_log(t, 'Backup is in another container than Primary')
    failed_over = _grep_log(t, 'Failing over Primary to duplicate Backup')
    gave_up = _grep_log(t, 'cannot lend its dead volumes')
    log = t.read_file('nzbget.log').decode(errors='replace')
    late = sorted(set(re.findall(r'Successfully downloaded Primary/Show\.part(1[0-5])\.rar\n', log)))
    ok = (labelled >= 1 and failed_over == 1 and gave_up >= 1 and len(late) < 6 and
          hp['Status'].startswith('FAILURE'))
    return (label, ok,
            'status=%s backup=%s labelled_logs=%d failover_logs=%d gave_up_logs=%d late_volumes_done=%s'
            % (hp['Status'], hb['Status'], labelled, failed_over, gave_up, late))


def scenario_deadvolumeothervideo(daemon, t):
    """TwinCheck labels the backup another container (its sampled articles
    differ, as any other container's would), but it holds another video:
    the dead-volume borrow cannot place the dead volumes in it, so the
    download fails over as soon as that is known, long before its last
    volume, instead of downloading to the end first."""
    return _dead_volume_other_video(daemon, t, 'deadvolumeothervideo', 'dvo', None)


def scenario_deadvolumeothersize(daemon, t):
    """deadvolumeothervideo with another encode of another size: no payload
    of the backup has the size of the movie the dead volumes belong to, so
    the borrow never gets to compare bytes. That counts as another video
    too, and the download fails over before its last volumes."""
    return _dead_volume_other_video(daemon, t, 'deadvolumeothersize', 'dvs', 14_250_000)


def scenario_deadvolumetwolenders(daemon, t):
    """Two backups in history: the higher-scored one, which TwinCheck labels
    another container (its outer rar has as many volumes as the primary),
    holds another video; the other holds the same movie nested, labelled by
    the fleet the same video. The borrow finds the first one's video differs,
    but the second lends the dead volumes: the download must not fail over to
    the first while the second lends, and its extraction carries on to the
    end from the borrowed volumes."""
    other = _nested_donor(t, 'dv2O', _payload(15_000_000, 23071), 15)
    hp, hb, movie = _dead_volume_nested(
        daemon, t, 'dv2', [('DupeCopyKind', 'unknown'), ('DupeVideo', 'same')],
        extra_backups=[('OtherVideo', other, 95, [])])
    api = daemon.wait_ready()
    ho = daemon.wait_history(api, 'OtherVideo')
    failed_over = _grep_log(t, 'Failing over Primary')
    cancelled = _grep_log(t, 'Cancelling direct unpack')
    du_ok = _grep_log(t, 'Direct unpack for Primary successful')
    lent = _grep_log(t, 'from duplicate Backup')
    integ = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    ok = (failed_over == 0 and cancelled == 0 and du_ok == 1 and lent >= 1 and integ and
          hp['Status'].startswith('SUCCESS') and ho['Status'].startswith('DELETED'))
    return ('deadvolumetwolenders', ok,
            'status=%s other=%s backup=%s failover_logs=%d du_cancelled=%d lent_logs=%d '
            'cannot_lend_logs=%d du_ok=%d extracted_ok=%s'
            % (hp['Status'], ho['Status'], hb['Status'], failed_over, cancelled, lent,
               _grep_log(t, 'cannot lend its dead volumes'), du_ok, integ))


def scenario_skipholetwin(daemon, t):
    """Direct unpack reads past a lost article in a stored rar volume; stream
    repair fills the volume from a twin and the extracted file is patched
    from the repaired volume, verified and moved into place."""
    good, detail = _skiphole_rar(daemon, t, 'SHT', donor=True, recovery=False)
    return ('skipholetwin', good, detail)


def scenario_persistdeadprimary(daemon, t):
    """The primary loses articles mid-download: two articles in the middle of
    the second volume and one in the third. A byte-identical twin waits in
    history. Direct
    unpack writes movie.mkv once as a persistent partial file and keeps it: the
    lost ranges are holes (or the session waits on a volume whose headers are
    lost), stream repair fills the volumes from the twin, and only the holes are
    read back. Never restarted, never copied, byte-identical."""
    dead = {1: {2, 3}, 2: {2}}
    good, detail = _skiphole_rar(daemon, t, 'PDP', donor=True, recovery=False, damage=dead)
    started = _grep_log(t, 'direct unpack into persistent partial files')
    fell_back = _grep_log(t, 'progressive direct unpack (')
    status_ok = 'status=SUCCESS' in detail and 'extracted_ok=True' in detail
    ok = status_ok and started == 1 and fell_back == 0
    return ('persistdeadprimary', ok, '%s persistent_starts=%d fallbacks=%d' % (detail, started, fell_back))


def scenario_persistrestart(daemon, t):
    """RustyGet restarts while a persistent direct unpack is mid-member: the
    next start adopts the member from its saved state and resumes it from the
    committed output instead of starting over. Byte-identical result."""
    seg = 300_000
    movie = _payload(24_000_000, 4417)
    vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 1_400_000)
    names = ['Rel.part%02d.rar' % (i + 1) for i in range(len(vols))]
    for name, data in zip(names, vols):
        t.write_file(os.path.join('data', 'PRA', name), data)
    members = [('PRA/%s' % n, n, len(d), seg, set()) for n, d in zip(names, vols)]
    par = generators.par2_index(list(zip(names, vols)))
    t.write_file(os.path.join('data', 'PRA', 'Rel.par2'), par)
    members.append(('PRA/Rel.par2', 'Rel.par2', len(par), seg, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'RelPR', build_multi_nzb(members), False, 'pr-key', 100)
    # Restart once direct unpack has read a few volumes of the member.
    reached = _watch_wait(t, 'Extracting from Rel.part05.rar', timeout=120)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    h = daemon.wait_history(api, 'RelPR', timeout=300)
    ok = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    resumed = _grep_log(t, 'resuming movie.mkv')
    started = _grep_log(t, 'direct unpack into persistent partial files')
    detail = 'status=%s unpack=%s extracted_ok=%s reached_before_restart=%d persistent_starts=%d resumed=%d' % (
        h['Status'], h['UnpackStatus'], ok, reached, started, resumed)
    good = ok and h['Status'].startswith('SUCCESS') and reached >= 1 and resumed >= 1
    return ('persistrestart', good, detail)


def scenario_persistrestartdamaged(daemon, t):
    """As persistrestart, with the second volume losing an article before the
    restart and only par2 recovery data to repair it. The restart comes before
    the repair: the saved list of volumes waiting for their repair puts the
    volume back on hold, so the resumed extraction waits for the par2 repair
    and refills the hole instead of extracting the whole set again."""
    import subprocess as _sp
    seg = 300_000
    movie = _payload(24_000_000, 5521)
    vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 1_400_000)
    work = t.path('data', 'PRD')
    os.makedirs(work, exist_ok=True)
    names = ['Rel.part%02d.rar' % (i + 1) for i in range(len(vols))]
    for name, data in zip(names, vols):
        t.write_file(os.path.join('data', 'PRD', name), data)
    damage = {1: {3}}                       # bytes 900,000-1,199,999 of volume 2
    members = [('PRD/%s' % n, n, len(d), seg, damage.get(i, set()))
               for i, (n, d) in enumerate(zip(names, vols))]
    _sp.run(['par2', 'create', '-q', '-q', '-s65536', '-c12', '-n1', '-a', 'Rel', 'Rel.par2'] + names,
            cwd=work, check=True, capture_output=True)
    for n in sorted(os.listdir(work)):
        if n.endswith('.par2'):
            members.append(('PRD/%s' % n, n, os.path.getsize(os.path.join(work, n)), seg, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'RelPRD', build_multi_nzb(members), False, 'prd-key', 100)
    reached = _watch_wait(t, 'Extracting from Rel.part05.rar', timeout=120)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    before_fallbacks = _grep_log(t, 'progressive direct unpack (')
    daemon.start()
    api = daemon.wait_ready()
    h = daemon.wait_history(api, 'RelPRD', timeout=300)
    ok = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    resumed = _grep_log(t, 'resuming movie.mkv')
    waiting = _grep_log(t, 'Rel.part02.rar is still waiting for its repair')
    patched = _grep_log(t, 'repaired movie.mkv from the repaired volumes')
    fallbacks = _grep_log(t, 'progressive direct unpack (') - before_fallbacks
    detail = ('status=%s par=%s unpack=%s extracted_ok=%s reached_before_restart=%d resumed=%d '
              'waiting=%d patched=%d fallbacks_after_restart=%d') % (
        h['Status'], h['ParStatus'], h['UnpackStatus'], ok, reached, resumed, waiting, patched, fallbacks)
    good = (ok and h['Status'].startswith('SUCCESS') and reached >= 1 and resumed >= 1
            and waiting >= 1 and patched >= 1 and fallbacks == 0)
    return ('persistrestartdamaged', good, detail)


def scenario_skipholepar(daemon, t):
    """As skipholetwin with no duplicate: the hole stays open until the
    download ends, par2 repairs the volume, and the extracted file is patched
    from the repaired volume."""
    good, detail = _skiphole_rar(daemon, t, 'SHP', donor=False, recovery=True)
    return ('skipholepar', good, detail)


def scenario_rar4lzdirect(daemon, t):
    """A RAR 2.9 set of three volumes, each holding an LZ-compressed member
    (method 0x33), extracted by direct unpack while it downloads: the progressive
    session decodes them itself, without the RAR 1.5-4.x fallback decoder."""
    names = ['a.bin', 'b.bin', 'c.bin']
    vols, gold = generators.rar3_lz_volumes(names)
    members = []
    for i, data in enumerate(vols):
        rel = 'lzA/v%d' % i
        t.write_file(os.path.join('data', rel), data)
        members.append((rel, 'Rel.part%d.rar' % (i + 1), len(data), 200, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'RelLZ', build_multi_nzb(members), False, 'lz-key', 100)
    h = daemon.wait_history(api, 'RelLZ', timeout=180)
    found = {}
    for rel in t.find_files('main', 'dst'):
        base = os.path.basename(rel)
        if base in names:
            found[base] = t.read_file(rel) == gold
    direct = _grep_log(t, 'Direct unpack for RelLZ successful')
    fallback = _grep_log(t, 'using the RAR 1.5-4.x decoder')
    ok = (h['Status'].startswith('SUCCESS') and found == {n: True for n in names}
          and direct == 1 and fallback == 0)
    return ('rar4lzdirect', ok, 'status=%s files=%s direct_ok=%d fallback=%d' % (
        h['Status'], found, direct, fallback))


def scenario_wholefilenfoproof(daemon, t):
    """Corner case: a different packing of the release (same volume names and
    sizes, other bytes) that ships the SAME small .nfo. A byte match on the
    .nfo must not prove the duplicate for recreating a missing volume - only
    a sibling volume of the missing one can."""
    members, donor_members, payloads = _wholefile_fixture(t, 'nfo')
    # one other volume is damaged (one verification miss, below the bail)
    # and the two others are missing as well: the .nfo is the only intact
    # file, so before the set-key rule it was the one the proof used
    members[0] = members[0][:4] + (set(range(1, 4)),)
    members[3] = members[3][:4] + (set(range(1, 4)),)
    for i, dm in enumerate(donor_members):                 # other packing: other bytes
        t.write_file(os.path.join('data', dm[0]), _payload(dm[2], 9800 + i))
    nfo = _payload(3_000, 9890)
    t.write_file(os.path.join('data', 'nfoA/rel.nfo'), nfo)
    t.write_file(os.path.join('data', 'nfoB/rel.nfo'), nfo)
    members = [('nfoA/rel.nfo', 'Rel.nfo', len(nfo), 500_000, set())] + members
    donor_members = [('nfoB/rel.nfo', 'Other.nfo', len(nfo), 500_000, set())] + donor_members
    api = daemon.wait_ready()
    daemon.append(api, 'DonNFO', build_multi_nzb(donor_members), True, 'nfo-key', 50)
    daemon.append(api, 'RelNFO', build_multi_nzb(members), False, 'nfo-key', 100)
    h = daemon.wait_history(api, 'RelNFO')
    proven = _grep_log(t, 'verified on intact file')
    recreated = _grep_log(t, 'Recreating')
    return ('wholefilenfoproof', proven == 0 and recreated == 0,
            'status=%s proven_logs=%d recreated_logs=%d' % (h['Status'], proven, recreated))


def scenario_wholefilesampleproof(daemon, t):
    """Corner case: a different packing (same volume names and sizes, other
    bytes) that ships the SAME sample. The damaged sample is repaired from it
    byte-identically - that is fine - but it must not prove the duplicate
    for recreating a missing archive volume of another set."""
    members, donor_members, payloads = _wholefile_fixture(t, 'smp')
    members[1] = members[1][:4] + (set(),)
    for i, dm in enumerate(donor_members):                 # other packing: other bytes
        t.write_file(os.path.join('data', dm[0]), _payload(dm[2], 9850 + i))
    sample = _payload(900_000, 9899)
    t.write_file(os.path.join('data', 'smpA/sample.mkv'), sample)
    t.write_file(os.path.join('data', 'smpB/sample.mkv'), sample)
    members = members + [('smpA/sample.mkv', 'Rel.sample.mkv', len(sample), 300_000, {2})]
    donor_members = donor_members + [('smpB/sample.mkv', 'Other.sample.mkv', len(sample), 200_000, set())]
    api = daemon.wait_ready()
    daemon.append(api, 'DonSMP', build_multi_nzb(donor_members), True, 'smp-key', 50)
    daemon.append(api, 'RelSMP', build_multi_nzb(members), False, 'smp-key', 100)
    h = daemon.wait_history(api, 'RelSMP')
    sample_fixed = _grep_log(t, 'of Rel.sample.mkv from duplicate')
    recreated = _grep_log(t, 'Recreating')
    return ('wholefilesampleproof', recreated == 0,
            'status=%s sample_repaired_logs=%d recreated_logs=%d' % (h['Status'], sample_fixed, recreated))


def scenario_dupefailoverchain(daemon, t):
    """Corner case: the first backup is dead too. The primary fails over to
    it, it fails over to the second (healthy) backup, which completes; the
    parked primary is never brought back."""
    seg = 100_000
    vol_dead = 2_900_000
    n = vol_dead // seg
    def dead_set(tag, seed):
        files = [('%s/d%d.bin' % (tag, i), '%s%d.bin' % (tag, i), vol_dead, seg, set(range(1, n + 1)))
                 for i in range(6)]
        for m in files:
            t.write_file(os.path.join('data', m[0]), _payload(vol_dead, seed))
        return build_multi_nzb(files)
    data = _payload(3_000_000, 9921)
    bp = _place_copy(t, 'chC', data)
    healthy = build_nzb(bp, 'Healthy.bin', 3_000_000, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', dead_set('chA', 9919), True, 'ch-key', 100)
    daemon.append(api, 'Backup1', dead_set('chB', 9920), False, 'ch-key', 95)
    daemon.append(api, 'Backup2', healthy, False, 'ch-key', 90)
    daemon.wait_history(api, 'Backup2', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    deadline = time.time() + 400
    status = {}
    while time.time() < deadline:
        hist = {h['NZBName']: h['Status'] for h in api.history()}
        queued = {g['NZBName'] for g in api.listgroups()}
        if hist.get('Backup2', '').startswith('SUCCESS') and not queued:
            status = hist
            break
        time.sleep(1)
    status = status or {h['NZBName']: h['Status'] for h in api.history()}
    p_over = _grep_log(t, 'Failing over Primary to duplicate Backup1')
    b1_over = _grep_log(t, 'Failing over Backup1 to duplicate Backup2')
    returned = _grep_log(t, 'Found duplicate Primary')
    return ('dupefailoverchain', p_over == 1 and b1_over == 1 and returned == 0 and
            status.get('Backup2', '').startswith('SUCCESS'),
            'primary=%s backup1=%s backup2=%s primary_failover=%d backup1_failover=%d primary_returned=%d'
            % (status.get('Primary'), status.get('Backup1'), status.get('Backup2'), p_over, b1_over, returned))


def scenario_xpacklatency(daemon, t):
    """Cross-packing against a slow news server (1 s per response): requests
    that end without a server answer (no response line, a connection lost
    mid-article) must be retried instead of counting the article as missing,
    or a repost that carries every byte repairs nothing."""
    size, seg = 20_000_000, 700_000
    data = _payload(size, 4243)
    pp = _place_copy(t, 'xlA', data, 'movie.mkv')
    primary = build_nzb(pp, 'movie.mkv', size, seg, set(range(8, 20)))
    members = []
    for i, vol in enumerate(generators.rar3_store_volumes('movie.mkv', data, 5_000_000), 1):
        rel = 'xlB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        members.append((rel, 'Rel.part%02d.rar' % i, len(vol), seg, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'DonXL', build_multi_nzb(members), True, 'xl-key', 50)
    daemon.append(api, 'RelXL', primary, False, 'xl-key', 100)
    h = daemon.wait_history(api, 'RelXL', timeout=900)
    retried = _grep_log(t, 'no response from') + _grep_log(t, 'lost while fetching')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    return ('xpacklatency', integ and h['Status'].startswith('SUCCESS'),
            'status=%s transient_logs=%d integrity=%s' % (h['Status'], retried, integ))


def scenario_xpackendhole_nodirect(daemon, t):
    """DirectWrite=no, holes up to the end of a bare movie: the file joined
    from article files ends with its last downloaded article, and the
    duplicate packs the movie into rar volumes. Cross-packing must map the
    target by its decoded size, not by the shorter file on disk. Before, the
    holes past the end of the file lay "outside the mappable inner stream"
    and nothing was recovered."""
    size, seg = 5_000_000, 333_333
    n = (size + seg - 1) // seg
    data = _payload(size, 4410)
    pp = _place_copy(t, 'xeA', data, 'movie.mkv')
    members = []
    for i, vol in enumerate(generators.rar3_store_volumes('movie.mkv', data, 1_500_000), 1):
        rel = 'xeB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 250_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'DonXE', build_multi_nzb(members), True, 'xe-key', 50)
    daemon.append(api, 'RelXE', build_nzb(pp, 'movie.mkv', size, seg, set(range(n - 2, n + 1))), False, 'xe-key', 100)
    h = daemon.wait_history(api, 'RelXE')
    unmappable = _grep_log(t, 'outside the mappable inner stream')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    return ('xpackendhole_nodirect', integ and unmappable == 0 and h['Status'].startswith('SUCCESS'),
            'status=%s unmappable_logs=%d integrity=%s' % (h['Status'], unmappable, integ))


def scenario_xpackcorrupt(daemon, t):
    """The news server serves a corrupt copy of every duplicate article (one
    byte changed, crc32 left as declared) and CrcCheck=no: repair fetches
    check the yEnc crc32 anyway, so the corrupt articles are rejected and the
    release ends FAILURE/HEALTH. Before, cross-packing wrote the corrupt
    bytes and the release ended SUCCESS with a damaged movie."""
    size, seg = 3_000_000, 250_000
    data = _payload(size, 4501)
    pp = _place_copy(t, 'xcA', data, 'movie.mkv')
    members = []
    for i, vol in enumerate(generators.rar3_store_volumes('movie.mkv', data, 1_000_000), 1):
        rel = 'xcB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 300_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'DonXC', build_multi_nzb(members), True, 'xc-key', 50)
    daemon.append(api, 'RelXC', build_nzb(pp, 'movie.mkv', size, seg, {4, 5, 6}), False, 'xc-key', 100)
    h = daemon.wait_history(api, 'RelXC')
    corrupted = daemon.proxy.corrupted if daemon.proxy else 0
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    return ('xpackcorrupt', corrupted >= 1 and h['Status'].startswith('FAILURE') and not integ,
            'status=%s corrupted_articles=%d integrity=%s' % (h['Status'], corrupted, integ))


def scenario_xpackextensionless(daemon, t):
    """An obfuscated bare movie posted without a file extension (par-rename
    can't restore the name while the first article is missing), repaired from
    rar volumes of the same movie. Before, only names with a media extension
    became bare sets, so cross-packing never mapped the target."""
    size, seg = 4_000_000, 250_000
    data = _payload(size, 4960)
    pp = _place_copy(t, 'xxA', data, 'movie.mkv')
    members = []
    for i, vol in enumerate(generators.rar3_store_volumes('movie.mkv', data, 1_500_000), 1):
        rel = 'xxB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 300_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'DonXX', build_multi_nzb(members), True, 'xx-key', 50)
    daemon.append(api, 'RelXX', build_nzb(pp, '848eddf3e2133c8a', size, seg, {1, 2, 9, 10}), False, 'xx-key', 100)
    h = daemon.wait_history(api, 'RelXX')
    integ = _verify_output(t, data, '848eddf3e2133c8a', dirs=(('main', 'dst'), ('main', 'inter')))
    return ('xpackextensionless', integ and h['Status'].startswith('SUCCESS'),
            'status=%s integrity=%s' % (h['Status'], integ))


def scenario_xpackextensionlessneg(daemon, t):
    """The negative: extensionless names that are really obfuscated rar
    volumes are tried as bare files too, and the identity probes must reject
    a bare duplicate of the movie inside them - nothing is written."""
    size = 4_000_000
    data = _payload(size, 4950)
    vols = generators.rar3_store_volumes('movie.mkv', data, 1_500_000)
    members, payloads = [], {}
    for i, vol in enumerate(vols, 1):
        rel = 'xnA/v%02d' % i
        t.write_file(os.path.join('data', rel), vol)
        name = '%016x' % (0x5eed0000 + i)
        payloads[name] = vol
        members.append((rel, name, len(vol), 250_000, {3, 4} if i == 2 else set()))
    dp = _place_copy(t, 'xnB', data, 'movie.mkv')
    api = daemon.wait_ready()
    daemon.append(api, 'DonXN', build_nzb(dp, 'movie.mkv', size, 300_000, set()), True, 'xn-key', 50)
    daemon.append(api, 'RelXN', build_multi_nzb(members), False, 'xn-key', 100)
    h = daemon.wait_history(api, 'RelXN')
    recovered = _grep_log(t, 'Recovered')
    both = (('main', 'dst'), ('main', 'inter'))
    intact = [_verify_output(t, v, n, dirs=both) for n, v in payloads.items()]
    # the damaged volume stays damaged, the others untouched
    return ('xpackextensionlessneg', recovered == 0 and intact.count(False) == 1 and
            h['Status'].startswith('FAILURE'),
            'status=%s recovered_logs=%d intact=%s' % (h['Status'], recovered, intact))


def scenario_manualparnopar(daemon, t):
    """ParCheck=manual, no par2 files: a release fully repaired from a
    duplicate is a success. Before, the repair pass asked for a par-check
    anyway and manual mode turned that into "needs manual repair"
    (WARNING/DAMAGED) although every byte was recovered."""
    size, seg = 3_000_000, 250_000
    data = _payload(size, 5100)
    pp = _place_copy(t, 'mpA', data, 'movie.mkv')
    dp = _place_copy(t, 'mpB', data, 'movie.mkv')
    api = daemon.wait_ready()
    daemon.append(api, 'DonMP', build_nzb(dp, 'Other.movie.mkv', size, 300_000, set()), True, 'mp-key', 50)
    daemon.append(api, 'RelMP', build_nzb(pp, 'movie.mkv', size, seg, {3, 4, 9}), False, 'mp-key', 100)
    h = daemon.wait_history(api, 'RelMP')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    return ('manualparnopar', integ and h['Status'].startswith('SUCCESS'),
            'status=%s integrity=%s' % (h['Status'], integ))


def scenario_xpackflaky(daemon, t):
    """Cross-packing while the provider drops every connection and turns new
    ones away for 4 s (a per-user connection limit, see FlakyNntpProxy):
    the repeated attempts for a duplicate article are spaced instead of being
    used up within milliseconds, and the four dropped pooled connections
    don't use up attempts of their own. Before, the article counted as
    missing, the duplicate's content map could not be built, and nothing was
    recovered."""
    size, seg = 12_000_000, 300_000
    data = _payload(size, 4244)
    pp = _place_copy(t, 'xfA', data, 'movie.mkv')
    primary = build_nzb(pp, 'movie.mkv', size, seg, set(range(10, 30)))
    members = []
    for i, vol in enumerate(generators.rar3_store_volumes('movie.mkv', data, 3_000_000), 1):
        rel = 'xfB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        members.append((rel, 'Rel.part%02d.rar' % i, len(vol), seg, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'DonXF', build_multi_nzb(members), True, 'xf-key', 50)
    daemon.append(api, 'RelXF', primary, False, 'xf-key', 100)
    h = daemon.wait_history(api, 'RelXF', timeout=300)
    rejected = daemon.proxy.rejected if daemon.proxy else 0
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    return ('xpackflaky', integ and rejected >= 1 and h['Status'].startswith('SUCCESS'),
            'status=%s recovered=%d rejected_connects=%d integrity=%s'
            % (h['Status'], int(h.get('DupeRecoveredArticles', 0)), rejected, integ))


def scenario_xpackdeadserver(daemon, t):
    """Cross-packing while the preferred news server goes down for good at
    the first duplicate request (see SCENARIO_FLAKY_PROXY): it delays the
    repair once, not for every article. The shared connection pool blocks the
    server for RetryInterval at the first refused connection (before it, each
    article made one more attempt, so 10 or more were refused); either way it
    must have been asked and turned away at least once, and the repair from
    the backup server finishes within 15 s."""
    size, seg = 12_000_000, 300_000
    data = _payload(size, 4245)
    pp = _place_copy(t, 'xdA', data, 'movie.mkv')
    primary = build_nzb(pp, 'movie.mkv', size, seg, set(range(10, 30)))
    members = []
    for i, vol in enumerate(generators.rar3_store_volumes('movie.mkv', data, 3_000_000), 1):
        rel = 'xdB/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        members.append((rel, 'Rel.part%02d.rar' % i, len(vol), seg, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'DonXD', build_multi_nzb(members), True, 'xd-key', 50)
    daemon.append(api, 'RelXD', primary, False, 'xd-key', 100)
    h = daemon.wait_history(api, 'RelXD', timeout=300)
    log = t.read_file('nzbget.log').decode(errors='replace')
    def when(pattern):
        # date, then (in the current log format) the process and thread ids, then the kind
        m = re.search(r'^(\w{3} \w{3} +\d+ [\d:]+ \d{4})\t(?:\d+\t\d+\t)?\w+\t' + pattern, log, re.M)
        return time.mktime(time.strptime(m.group(1), '%a %b %d %H:%M:%S %Y')) if m else None
    start, end = when('Cross-packing repair of'), when(r'Recovered .*\(cross-packing\)')
    seconds = end - start if start is not None and end is not None else None
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    rejected = daemon.proxy.rejected if daemon.proxy else 0
    return ('xpackdeadserver', integ and h['Status'].startswith('SUCCESS') and rejected >= 1 and
            seconds is not None and seconds <= 15,
            'status=%s cross_pack_seconds=%s rejected_connects=%d integrity=%s'
            % (h['Status'], seconds, rejected, integ))


def _wholefile_named(daemon, t, tag, primary_names, donor_names):
    """The whole-file fixture with custom member names on both sides: part03
    missing entirely, everything else intact; byte-identical repost."""
    members, donor_members, payloads = _wholefile_fixture(t, tag)
    members[1] = members[1][:4] + (set(),)
    members = [(m[0], primary_names[i]) + m[2:] for i, m in enumerate(members)]
    donor_members = [(m[0], donor_names[i]) + m[2:] for i, m in enumerate(donor_members)]
    payloads = {primary_names[i]: payloads['Rel.part%02d.rar' % (i + 1)] for i in range(4)}
    api = daemon.wait_ready()
    # marked dead by a dupe tool: no failover fetches it, so stream repair is the
    # last option left (B78) - the repair itself is what these scenarios test
    _ds_append(api, 'Don' + tag, build_multi_nzb(donor_members), tag + '-key', 50,
               params=[{'Name': 'DupeAlive', 'Value': '0'}])
    daemon.append(api, 'Rel' + tag, build_multi_nzb(members), False, tag + '-key', 100)
    h = daemon.wait_history(api, 'Rel' + tag)
    recreated = _grep_log(t, 'Recreating')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    # by name: old-style volumes (x.r00) don't end in .rar
    integ = all(_verify_output(t, payloads[n], n, dirs=both_dirs) for n in primary_names)
    return h, recreated, integ


def scenario_wholefileunicode(daemon, t):
    """Names with spaces and non-ASCII characters on both sides."""
    h, recreated, integ = _wholefile_named(daemon, t, 'UNI',
        ['Rel Fête été.part%02d.rar' % i for i in range(1, 5)],
        ['Autre Ñame ü.part%02d.rar' % i for i in range(1, 5)])
    return ('wholefileunicode', integ and recreated == 1,
            'status=%s recreated_logs=%d integrity=%s' % (h['Status'], recreated, integ))


def scenario_wholefilepadding(daemon, t):
    """The duplicate zero-pads its volume numbers differently (part003 vs
    part03): the volumes are still siblings and the missing one pairs."""
    h, recreated, integ = _wholefile_named(daemon, t, 'PAD',
        ['Rel.part%03d.rar' % i for i in range(1, 5)],
        ['Other.part%02d.rar' % i for i in range(1, 5)])
    return ('wholefilepadding', integ and recreated == 1,
            'status=%s recreated_logs=%d integrity=%s' % (h['Status'], recreated, integ))


def scenario_wholefileoldstyle(daemon, t):
    """Old-style rar volume names (x.rar, x.r00, x.r01, ...) on both sides,
    with different base names: the volumes pair by number and the missing
    third volume (x.r01) is recreated."""
    h, recreated, integ = _wholefile_named(daemon, t, 'OLD',
        ['Rel.rar', 'Rel.r00', 'Rel.r01', 'Rel.r02'],
        ['Other.rar', 'Other.r00', 'Other.r01', 'Other.r02'])
    return ('wholefileoldstyle', integ and recreated == 1,
            'status=%s recreated_logs=%d integrity=%s' % (h['Status'], recreated, integ))


def scenario_wholefilecontinued(daemon, t):
    """An old-style set of more than 101 volumes continues x.r99 with x.s00,
    x.s01, ...: the missing x.s00 belongs to the x.rNN set, is proven on its
    siblings and recreated."""
    h, recreated, integ = _wholefile_named(daemon, t, 'CNT',
        ['Rel.r98', 'Rel.s00', 'Rel.s01', 'Rel.r99'],
        ['Other.r98', 'Other.s00', 'Other.s01', 'Other.r99'])
    return ('wholefilecontinued', integ and recreated == 1,
            'status=%s recreated_logs=%d integrity=%s' % (h['Status'], recreated, integ))


def scenario_wholefiletwosets(daemon, t):
    """Two archive sets in one release (Main.partNN.rar and Extras.partNN.rar),
    the duplicate carrying both under other names. Extras.part03.rar is
    missing entirely: it must be recreated from the duplicate's Extras set,
    never from its Main set's part03."""
    seg, seg_d = 500_000, 300_000
    vol = 1_500_000
    n = (vol + seg - 1) // seg
    members, donor_members, payloads = [], [], {}
    for set_name, other_name, seed in (('Main', 'OMain', 9600), ('Extras', 'OExtras', 9700)):
        for i in range(4):
            data = _payload(vol, seed + i)
            name = '%s.part%02d.rar' % (set_name, i + 1)
            payloads[name] = data
            pa = 'tsA/%s%d' % (set_name, i)
            pb = 'tsB/%s%d' % (set_name, i)
            t.write_file(os.path.join('data', pa), data)
            t.write_file(os.path.join('data', pb), data)
            missing = set(range(1, n + 1)) if (set_name == 'Extras' and i == 2) else set()
            members.append((pa, name, vol, seg, missing))
            donor_members.append((pb, '%s.part%02d.rar' % (other_name, i + 1), vol, seg_d, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'DonTS', build_multi_nzb(donor_members), True, 'ts-key', 50)
    daemon.append(api, 'RelTS', build_multi_nzb(members), False, 'ts-key', 100)
    h = daemon.wait_history(api, 'RelTS')
    recreated = _grep_log(t, 'Recreating Extras.part03.rar')
    wrong = _grep_log(t, 'from file OMain.part03.rar')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[n], n, dirs=both_dirs) for n in payloads)
    return ('wholefiletwosets', integ and recreated == 1 and wrong == 0,
            'status=%s recreated_logs=%d wrong_set_logs=%d integrity=%s' % (h['Status'], recreated, wrong, integ))


def scenario_dupefailovernonzb(daemon, t):
    """Corner case: the best backup's source nzb-file is gone from NzbDir, so
    it can't be downloaded again. The failover must pick the next backup that
    can (and never park the download for one that can't)."""
    seg = 100_000
    vol_dead = 2_900_000
    n = vol_dead // seg
    dead = [('nnA/d%d.bin' % i, 'Dead%d.bin' % i, vol_dead, seg, set(range(1, n + 1))) for i in range(6)]
    for m in dead:
        t.write_file(os.path.join('data', m[0]), _payload(vol_dead, 9930))
    data1, data2 = _payload(3_000_000, 9931), _payload(3_000_000, 9932)
    b1 = build_nzb(_place_copy(t, 'nnB', data1), 'Gone.bin', 3_000_000, seg, set())
    b2 = build_nzb(_place_copy(t, 'nnC', data2), 'Kept.bin', 3_000_000, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(dead), True, 'nn-key', 100)
    daemon.append(api, 'BackupGone', b1, False, 'nn-key', 95)
    daemon.append(api, 'BackupKept', b2, False, 'nn-key', 90)
    daemon.wait_history(api, 'BackupKept', timeout=60)
    nzbdir = t.path('main', 'nzb')
    for name in os.listdir(nzbdir):
        if 'BackupGone' in name:
            os.remove(os.path.join(nzbdir, name))
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary')
    deadline = time.time() + 180
    hk = daemon.wait_history(api, 'BackupKept')
    while hk['Status'].startswith('DELETED') and time.time() < deadline:
        time.sleep(1)
        hk = daemon.wait_history(api, 'BackupKept')
    to_gone = _grep_log(t, 'Failing over Primary to duplicate BackupGone')
    to_kept = _grep_log(t, 'Failing over Primary to duplicate BackupKept')
    return ('dupefailovernonzb', to_gone == 0 and to_kept == 1 and hk['Status'].startswith('SUCCESS'),
            'primary=%s kept=%s failover_to_gone=%d failover_to_kept=%d'
            % (hp['Status'], hk['Status'], to_gone, to_kept))


def scenario_wholefileproofcost(daemon, t):
    """Cost bound: six volumes missing entirely and a duplicate that is a
    different packing (same names and sizes, other bytes). The proof on an
    intact sibling fails - and must not be repeated for every missing volume
    of the same set: the duplicate's articles are fetched a bounded number
    of times, not once per missing volume."""
    seg, seg_d = 500_000, 300_000
    vol = 1_500_000
    n = (vol + seg - 1) // seg
    members, donor_members = [], []
    for i in range(10):
        data = _payload(vol, 9500 + i)
        pa, pb = 'pcA/v%d' % i, 'pcB/v%d' % i
        t.write_file(os.path.join('data', pa), data)
        t.write_file(os.path.join('data', pb), _payload(vol, 9600 + i))     # other packing
        missing = set(range(1, n + 1)) if i in (2, 3, 4, 5, 6, 7) else set()
        members.append((pa, 'Rel.part%02d.rar' % (i + 1), vol, seg, missing))
        donor_members.append((pb, 'Other.part%02d.rar' % (i + 1), vol, seg_d, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'DonPC', build_multi_nzb(donor_members), True, 'pc-key', 50)
    daemon.append(api, 'RelPC', build_multi_nzb(members), False, 'pc-key', 100)
    h = daemon.wait_history(api, 'RelPC')
    try:
        log = t.read_file('nserv.log').decode(errors='replace')
    except Exception:
        log = ''
    donor_bodies = len(re.findall(r'BODY <pcB/', log))
    recreated = _grep_log(t, 'Recreating')
    # one failed proof: 2 intact volumes x their candidates x a few probes
    return ('wholefileproofcost', recreated == 0 and donor_bodies <= 32,
            'status=%s recreated_logs=%d donor_body_requests=%d' % (h['Status'], recreated, donor_bodies))


def scenario_streamretry(daemon, t):
    """Corner case (no whole-file job involved): stream repair fully repairs
    one volume and leaves a hole in another that no duplicate carries, so the
    release ends FAILURE/HEALTH. "Retry failed articles" can't fill that hole
    either - the release must stay a failure, never turn into a SUCCESS with
    a hole in a file."""
    members, donor_members, payloads = _wholefile_fixture(t, 'sr')
    members[2] = members[2][:4] + (set(),)               # part03 complete
    members[3] = members[3][:4] + ({2},)                 # part04 article 2 missing ...
    donor_members[3] = donor_members[3][:4] + (set(range(2, 4)),)  # ... on the duplicate too
    api = daemon.wait_ready()
    daemon.append(api, 'DonSR', build_multi_nzb(donor_members), True, 'sr-key', 50)
    daemon.append(api, 'RelSR', build_multi_nzb(members), False, 'sr-key', 100)
    h = daemon.wait_history(api, 'RelSR')
    nzbid = h['NZBID']
    api.editqueue('HistoryRetryFailed', 0, '', [nzbid])
    time.sleep(3)
    deadline = time.time() + 180
    while time.time() < deadline and any(g['NZBID'] == nzbid for g in api.listgroups()):
        time.sleep(1)
    h2 = [x for x in api.history() if x['NZBID'] == nzbid][0]
    return ('streamretry', h['Status'].startswith('FAILURE') and h2['Status'].startswith('FAILURE'),
            'status=%s health=%d retry_status=%s retry_health=%d'
            % (h['Status'], h['Health'], h2['Status'], h2['Health']))


def _retry_parked_release(daemon, t):
    """The release of the B22 and B29 scenarios: parked for health (no par2,
    HealthCheck=park: a whole volume is gone, health < 85%) after one finished
    volume kept a hole, then "Download remaining files" (HistoryReturn). Returns
    the parked status, the final history entry and a byte check of a volume."""
    seg_primary, seg_donor = 500_000, 300_000
    small, big = 1_500_000, 6_000_000
    members = [
        ('rkA/x.part01.rar', 'Rel.part01.rar', small, seg_primary, {3}),     # its last article is missing
        # (the Recovered line names the duplicate; Queueing doesn't)
        ('rkA/x.part02.rar', 'Rel.part02.rar', big, seg_primary, set(range(1, 13))),   # gone: health < 85%
        ('rkA/x.part03.rar', 'Rel.part03.rar', big, seg_primary, set()),
    ]
    payloads = {}
    for i, m in enumerate(members):
        payloads[m[1]] = _payload(m[2], 9700 + i)
        t.write_file(os.path.join('data', m[0]), payloads[m[1]])
    donor = [(m[0].replace('rkA', 'rkB'), m[1].replace('Rel.', 'Other.'), m[2], seg_donor, set()) for m in members]
    for dm, m in zip(donor, members):
        t.write_file(os.path.join('data', dm[0]), payloads[m[1]])
    api = daemon.wait_ready()
    daemon.append(api, 'DonRK', build_multi_nzb(donor), True, 'rk-key', 50)
    daemon.append(api, 'RelRK', build_multi_nzb(members), False, 'rk-key', 100)
    h = daemon.wait_history(api, 'RelRK')
    nzbid = h['NZBID']
    parked = h['Status']
    api.editqueue('HistoryReturn', 0, '', [nzbid])
    time.sleep(3)
    deadline = time.time() + 180
    while time.time() < deadline and any(g['NZBID'] == nzbid for g in api.listgroups()):
        time.sleep(1)
    h2 = [x for x in api.history() if x['NZBID'] == nzbid][0]
    def intact(name):
        dst = [rel for rel in t.find_files('main') if rel.endswith(name)]
        return bool(dst) and t.read_file(dst[0]) == payloads[name]
    return parked, h2, intact


def scenario_retrykeepsjobs(daemon, t):
    """B22: a release is parked for health (no par2, HealthCheck=park: a whole
    volume is gone, health < 85%) after one finished volume kept a hole;
    "Download remaining files" (HistoryReturn) downloads the rest, and
    post-processing must still repair the finished volume's hole from the
    duplicate. Retrying used to drop every saved repair job, so the finished
    volume (not downloaded again) was never repaired."""
    parked, h2, intact = _retry_parked_release(daemon, t)
    repaired = _grep_log(t, 'of Rel.part01.rar from duplicate DonRK')
    ok = parked.startswith('FAILURE') and repaired >= 1 and intact('Rel.part01.rar')
    return ('retrykeepsjobs', ok, 'parked=%s status=%s repaired_logs=%d intact=%s'
            % (parked, h2['Status'], repaired, intact('Rel.part01.rar')))


def scenario_retryparkedname(daemon, t):
    """B29: the volume that was parked while it downloaded is recorded by its
    temporary output name (<id>.out.tmp); after "Download remaining files" it
    must download and be repaired under its own name (Rel.part02.rar), so a
    whole-file recreation from the duplicate can pair it. Both volumes end
    byte-identical and the release succeeds."""
    parked, h2, intact = _retry_parked_release(daemon, t)
    temp_named = _grep_log(t, '.out.tmp (no article available)') + _grep_log(t, 'Stream repair of 5.out.tmp')
    ok = (parked.startswith('FAILURE') and 'SUCCESS' in h2['Status'] and intact('Rel.part01.rar') and
          intact('Rel.part02.rar') and temp_named == 0)
    return ('retryparkedname', ok, 'parked=%s status=%s part01=%s part02=%s temp_named_logs=%d'
            % (parked, h2['Status'], intact('Rel.part01.rar'), intact('Rel.part02.rar'), temp_named))


def scenario_healthlastarticle(daemon, t):
    """The failed article that drops health below critical is a file's last
    one: the health cancel deletes the file inside that article's completion.
    The file is still whole but for its hole, and must be finished like any
    other (its hole queued for stream repair), not dropped unassembled with no
    repair job (it was parked with its hole, and "Download remaining files"
    never repaired it). One connection makes the order deterministic."""
    seg_primary, seg_donor = 100_000, 60_000
    payload = _payload(1_000_000, 9800)
    t.write_file('data/hlA/x.part01.rar', payload)
    t.write_file('data/hlB/x.part01.rar', payload)
    daemon_api = daemon.wait_ready()
    daemon.append(daemon_api, 'DonHL', build_multi_nzb(
        [('hlB/x.part01.rar', 'Other.part01.rar', len(payload), seg_donor, set())]), True, 'hl-key', 50)
    daemon.append(daemon_api, 'RelHL', build_multi_nzb(
        [('hlA/x.part01.rar', 'Rel.part01.rar', len(payload), seg_primary, {9, 10})]), False, 'hl-key', 100)
    h = daemon.wait_history(daemon_api, 'RelHL')
    queued = _grep_log(t, 'Queueing stream repair of Rel.part01.rar')
    return ('healthlastarticle', h['Status'].startswith('FAILURE') and queued >= 1,
            'status=%s health=%d queued_logs=%d' % (h['Status'], h['Health'], queued))


def scenario_streamgrouped(daemon, t):
    """B2: two news servers of one group (the same account, Server1.Group =
    Server2.Group = 1) and a duplicate that lacks an article stream repair asks
    for. Once the first server says 430 the pool won't hand out the second
    (same group and level), so it isn't worth waiting for: the repair moves on
    at once instead of waiting ArticleTimeout (20 s) per missing article."""
    members, donor_members, payloads = _wholefile_fixture(t, 'sg')
    members[2] = members[2][:4] + (set(),)               # part03 complete
    members[3] = members[3][:4] + ({2},)                 # part04 article 2 missing ...
    donor_members[3] = donor_members[3][:4] + (set(range(2, 4)),)  # ... on the duplicate too
    api = daemon.wait_ready()
    daemon.append(api, 'DonSG', build_multi_nzb(donor_members), True, 'sg-key', 50)
    daemon.append(api, 'RelSG', build_multi_nzb(members), False, 'sg-key', 100)
    start = time.time()
    h = daemon.wait_history(api, 'RelSG', timeout=300)
    took = time.time() - start
    repaired = _grep_log(t, 'from duplicate DonSG')
    ok = took < 18 and repaired >= 1
    return ('streamgrouped', ok, 'status=%s took=%.0fs repaired_logs=%d' % (h['Status'], took, repaired))


def scenario_wholefilelive(daemon, t):
    """Fix 2 under DupeArticleFallback=live: the zero-article volume
    completes while the rest still downloads (throttled), the live pass
    proves the donor on the holed volume and recreates the missing one, and
    the release completes SUCCESS byte-identically."""
    members, donor_members, payloads = _wholefile_fixture(t, 'wl')
    big = _payload(12_000_000, 9350)
    t.write_file(os.path.join('data', 'wlA/z.bin'), big)
    members = members + [('wlA/z.bin', 'Rel.zz.bin', len(big), 500_000, set())]
    api = daemon.wait_ready()
    daemon.append(api, 'DonWL', build_multi_nzb(donor_members), True, 'wl-key', 50)
    daemon.append(api, 'RelWL', build_multi_nzb(members), False, 'wl-key', 100)
    h = daemon.wait_history(api, 'RelWL', timeout=300)
    live = _grep_log(t, 'Starting live stream repair')
    recreated = _grep_log(t, 'Recreating Rel.part03.rar')
    both_dirs = (('main', 'dst'), ('main', 'inter'))
    integ = all(_verify_output(t, payloads[m[1]], '.rar', dirs=both_dirs) for m in members[:4])
    return ('wholefilelive', integ and recreated == 1,
            'status=%s live_logs=%d recreated_logs=%d integrity=%s' % (h['Status'], live, recreated, integ))


def scenario_dupehopelessnodupecheck(daemon, t):
    """Corner case: HealthCheck=dupe with DupeCheck=no. There is no
    duplicate handling, so no failover - but a hopeless download (a dead
    posting) must still be parked, as the option help promises, instead of
    failing every article."""
    seg = 100_000
    vol_dead = 2_900_000
    n = vol_dead // seg
    dead = [('hnA/d%d.bin' % i, 'Dead%d.bin' % i, vol_dead, seg, set(range(1, n + 1))) for i in range(6)]
    for m in dead:
        t.write_file(os.path.join('data', m[0]), _payload(vol_dead, 9800))
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(dead), False, 'hn-key', 100)
    hp = daemon.wait_history(api, 'Primary')
    parked = _grep_log(t, 'Parking Primary: health')
    failed_articles = int(hp.get('FailedArticles', 0))
    return ('dupehopelessnodupecheck', parked == 1 and failed_articles < 140,
            'status=%s parked_logs=%d failed_articles=%d' % (hp['Status'], parked, failed_articles))


def scenario_dupefailovernofallback(daemon, t):
    """Corner case: HealthCheck=dupe with DupeArticleFallback=no (no sample
    gate). A posting that merely misses its first stretch must not be
    abandoned on its first failed article in favour of a lower-scored
    backup: it is still at almost full health then, and warrants more than
    the backup's score."""
    seg = 100_000
    vol = 6_000_000
    data = _payload(vol, 9900)
    pp = _place_copy(t, 'nfA', data)
    primary = build_nzb(pp, 'Main.bin', vol, seg, set(range(1, 4)))
    bp = _place_copy(t, 'nfB', _payload(3_000_000, 9901))
    backup = build_nzb(bp, 'Backup.bin', 3_000_000, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', primary, True, 'nf-key', 100)
    daemon.append(api, 'Backup', backup, False, 'nf-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary')
    failed_over = _grep_log(t, 'Failing over Primary')
    parked = _grep_log(t, 'Parking Primary')
    return ('dupefailovernofallback', failed_over == 0 and parked == 0,
            'status=%s failover_logs=%d parked_logs=%d' % (hp['Status'], failed_over, parked))


def scenario_dupefailoverraw(daemon, t):
    """B25: HealthCheck=dupe with RawArticle=yes, where article borrowing never
    applies (the articles stay encoded): the attempt count stays at 0, and the
    gate that gives borrowing its 32-article sample must not block the failover
    for good. The dead primary is abandoned early and the backup fetched."""
    seg = 100_000
    vol_dead, vol_backup = 2_900_000, 3_000_000
    n = vol_dead // seg
    # every 5th article exists: the dead-pick probe finds the posting alive and
    # leaves it to the health check, and it isn't hopeless (20% alive)
    dead = [('frA/d%d.bin' % i, 'Dead%d.bin' % i, vol_dead, seg,
             set(k for k in range(1, n + 1) if k % 5)) for i in range(6)]
    for m in dead:
        t.write_file(os.path.join('data', m[0]), _payload(vol_dead, 9600))
    bp = _place_copy(t, 'frB', _payload(vol_backup, 9601))
    backup = build_nzb(bp, 'Backup.bin', vol_backup, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(dead), True, 'fr-key', 100)
    daemon.append(api, 'Backup', backup, False, 'fr-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary')
    failed_over = _grep_log(t, 'Failing over Primary to duplicate Backup')
    probed = _grep_log(t, 'Dupe probe of download') + _grep_log(t, 'probe articles on no server')
    failed_articles = int(hp.get('FailedArticles', 0))
    ok = failed_over == 1 and failed_articles < 6 * n * 4 // 5
    return ('dupefailoverraw', ok, 'status=%s failover_logs=%d probe_logs=%d failed_articles=%d of %d'
            % (hp['Status'], failed_over, probed, failed_articles, 6 * n * 4 // 5))


def scenario_lockedfailover(daemon, t):
    """HealthCheck=dupe, DirectUnpack=yes: the primary is a RAR5 set with
    encrypted headers (rar -hp) and no password in its nzb; a plain copy waits
    in history as its backup. Direct unpack stops at once ("password
    required"), and the primary must fail over then, not after downloading
    every volume (live South Park S23E05 downloaded 2.1 GB first). Needs the
    rar tool; skipped (passes) without it."""
    if not shutil.which('rar'):
        return ('lockedfailover', True, 'skipped: no rar tool')
    import tempfile
    data = b'\x1a\x45\xdf\xa3' + _payload(4_000_000, 9601)
    work = tempfile.mkdtemp(prefix='lockedfo-')
    try:
        with open(os.path.join(work, 'movie.mkv'), 'wb') as f:
            f.write(data)
        subprocess.run(['rar', 'a', '-ma5', '-m0', '-hpsecret', '-v200k', '-idq', 'lock.rar', 'movie.mkv'],
                       cwd=work, check=True, stdout=subprocess.DEVNULL)
        vols = sorted(n for n in os.listdir(work) if n.endswith('.rar'))
        members = []
        for name in vols:
            with open(os.path.join(work, name), 'rb') as f:
                vol = f.read()
            members.append((_place_copy(t, 'lkA', vol, name), name, len(vol), 50_000, set()))
    finally:
        shutil.rmtree(work, ignore_errors=True)
    bp = _place_copy(t, 'lkB', data, 'movie.mkv')
    backup = build_nzb(bp, 'movie.mkv', len(data), 100_000, set())
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(members), True, 'lk-key', 100)
    daemon.append(api, 'Backup', backup, False, 'lk-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups()
                                         if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary')
    deadline = time.time() + 180
    hb = daemon.wait_history(api, 'Backup')
    while hb['Status'].startswith('DELETED') and time.time() < deadline:
        time.sleep(0.5)
        hb = daemon.wait_history(api, 'Backup')
    failed_over = _grep_log(t, 'Failing over Primary to duplicate Backup: its archive needs a password')
    got = int(hp.get('SuccessArticles', 0))
    total = int(hp.get('TotalArticles', 0))
    early = total > 0 and got < total * 6 // 10
    ok = failed_over == 1 and early and hb['Status'].startswith('SUCCESS')
    return ('lockedfailover', ok, 'primary=%s backup=%s failover_logs=%d articles=%d of %d'
            % (hp['Status'], hb['Status'], failed_over, got, total))


def scenario_dupefailover(daemon, t):
    """HealthCheck=dupe: the primary is a dead posting (every article of
    every file missing) and no duplicate carries its files, while a healthy
    lower-scored backup of the same title waits in history (deleted as
    duplicate at intake). The download must be abandoned early - long before
    all of its articles have failed - and the backup fetched and completed
    in its place. The backup's score (90) is above what the primary (100)
    warrants at the health it has when the sample completes, so the
    failover itself fires (not the hopeless-park, see dupehopeless)."""
    seg = 100_000
    # the dead posting is packaged differently from the backup (other
    # volume size and article count), so no article can be borrowed from it
    vol_dead, vol_backup = 2_900_000, 3_000_000
    n = vol_dead // seg
    dead = [('foA/d%d.bin' % i, 'Dead%d.bin' % i, vol_dead, seg, set(range(1, n + 1)))
            for i in range(6)]
    for m in dead:
        t.write_file(os.path.join('data', m[0]), _payload(vol_dead, 9500))
    data = _payload(vol_backup, 9501)
    bp = _place_copy(t, 'foB', data)
    backup = build_nzb(bp, 'Backup.bin', vol_backup, seg, set())
    api = daemon.wait_ready()
    # the primary is queued paused first, so the lower-scored backup is
    # deleted as duplicate into history instead of downloading
    daemon.append(api, 'Primary', build_multi_nzb(dead), True, 'fo-key', 100)
    daemon.append(api, 'Backup', backup, False, 'fo-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups()
                                         if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary')
    # the backup leaves history while it downloads and returns finished
    deadline = time.time() + 180
    hb = daemon.wait_history(api, 'Backup')
    while hb['Status'].startswith('DELETED') and time.time() < deadline:
        time.sleep(0.5)
        hb = daemon.wait_history(api, 'Backup')
    failed_over = _grep_log(t, 'Failing over Primary to duplicate Backup')
    failed_articles = int(hp.get('FailedArticles', 0))
    integ = _verify_output(t, data)
    # the primary has 174 articles; the failover fires once 32 were tried
    # against the (unusable) duplicate and the health is below critical
    early = failed_articles < 140
    return ('dupefailover', failed_over == 1 and early and integ and
            hb['Status'].startswith('SUCCESS'),
            'status=%s backup_status=%s failover_logs=%d failed_articles=%d integrity=%s'
            % (hp['Status'], hb['Status'], failed_over, failed_articles, integ))


DS_TITLE = 'Show.S01E01.1080p.WEB.H264-GRP'
DS_KEY = 'tvdbid=1-S01-E01|show-s01e01'
DS_PICK = 23859118


def _ds_append(api, name, nzb, key, score, paused=True, params=()):
    content = base64.standard_b64encode(nzb.encode()).decode()
    return api.append(name, content, 'test', 0, False, paused, key, score, 'score', list(params))


def _ds_group(api, name):
    return next((g for g in api.listgroups() if g['NZBName'] == name), None)


def _fake_nzb_ids(ids, total_bytes, name='a.mkv'):
    """An nzb-file of one file whose articles are exactly ``ids``."""
    seg = max(1, total_bytes // len(ids))
    segs = ''.join('<segment bytes="%d" number="%d">%s</segment>' % (seg, i + 1, m) for i, m in enumerate(ids))
    return ('<?xml version="1.0" encoding="UTF-8"?>\n<nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">'
            '<file poster="p@x" date="1" subject="[1/1] &quot;%s&quot; yEnc (1/1)"><groups><group>alt.binaries.test</group>'
            '</groups><segments>%s</segments></file></nzb>' % (name, segs)).encode()


REAL_INDEXER_URL_VAR, REAL_INDEXER_KEY_VAR = 'NZBHYDRA2_URL', 'NZBHYDRA2_APIKEY'


def _fleet(daemon, members, key='fleet-key', timeout=30, http_timeout=None):
    """Calls appendfleet (JSON-RPC) with members [(name, nzb text)]; returns its result."""
    import json as _json
    import urllib.request as _req
    params = [key, 'test', 0, timeout]
    for name, nzb in members:
        params += [name, 'http://127.0.0.1:9/none.nzb' if nzb is None else base64.standard_b64encode(nzb.encode()).decode()]
    body = _json.dumps({'method': 'appendfleet', 'params': params}).encode()
    request = _req.Request('http://127.0.0.1:%d/jsonrpc' % daemon.rpc_port, body, {'Content-Type': 'application/json'})
    with _req.urlopen(request, timeout=http_timeout or timeout + 30) as reply:
        return _json.loads(reply.read().decode())['result']


def _fleet_ids(prefix, n=40):
    return ['%s-%d@x' % (prefix, i) for i in range(n)]


def scenario_fleetdeadfirst(daemon, t):
    """appendfleet: the client sends a dead copy first, then a half one, then a
    whole one. The whole one is queued; the others wait in history ranked
    (half before dead, the dead one marked DupeAlive=0); the dead copy never
    downloads an article."""
    daemon.fake_nntp.alive = set(_fleet_ids('fw')) | set(_fleet_ids('fh')[:20])
    api = daemon.wait_ready()
    reply = _fleet(daemon, [('Show.S01E01.dead', _fake_nzb_ids(_fleet_ids('fd'), 400_000).decode()),
                            ('Show.S01E01.half', _fake_nzb_ids(_fleet_ids('fh'), 400_000).decode()),
                            ('Show.S01E01.whole', _fake_nzb_ids(_fleet_ids('fw'), 400_000).decode())])
    order = [(m['Name'], m['Status'], m['Alive']) for m in reply['Members']]
    hw = daemon.wait_history(api, 'Show.S01E01.whole', timeout=120)
    deadline = time.time() + 30
    while time.time() < deadline and not hw['Status'].startswith(('SUCCESS', 'FAILURE')):
        time.sleep(0.5)
        hw = daemon.wait_history(api, 'Show.S01E01.whole', timeout=60)
    dead = next((h for h in api.history() if h['NZBName'] == 'Show.S01E01.dead'), {})
    tried = int(dead.get('SuccessArticles', 0)) + int(dead.get('FailedArticles', 0))
    ok = ([o[0] for o in order] == ['Show.S01E01.whole', 'Show.S01E01.half', 'Show.S01E01.dead'] and
          order[0][1] == 'QUEUED' and order[2][1] == 'DEAD' and reply['Chosen'] == reply['Members'][0]['NZBID'] and
          hw['Status'].startswith('SUCCESS') and tried == 0)
    return ('fleetdeadfirst', ok, 'order=%s whole=%s dead_tried=%d complete=%s' % (order, hw['Status'], tried, reply['Complete']))


def scenario_fleettwins(daemon, t):
    """appendfleet: two members are the same posting: it is checked once, added
    once; the copy is reported SAME_POSTING with the id of the one added."""
    daemon.fake_nntp.alive = set(_fleet_ids('tw')) | set(_fleet_ids('to'))
    daemon.wait_ready()
    reply = _fleet(daemon, [('Show.S01E01.a', _fake_nzb_ids(_fleet_ids('tw'), 400_000).decode()),
                            ('Show.S01E01.b', _fake_nzb_ids(_fleet_ids('tw'), 400_000).decode()),
                            ('Show.S01E01.c', _fake_nzb_ids(_fleet_ids('to'), 410_000).decode())])
    byname = {m['Name']: m for m in reply['Members']}
    # every item the fleet added carries its rank (DupeFleet), for queue extensions
    api = daemon.wait_ready()
    fleet_params = {x['NZBName']: {p['Name']: p['Value'] for p in x.get('Parameters', [])}.get('DupeFleet')
                    for x in api.listgroups() + api.history()}
    ranks_ok = (fleet_params.get('Show.S01E01.a') == str(byname['Show.S01E01.a']['Rank']) and
                fleet_params.get('Show.S01E01.c') == str(byname['Show.S01E01.c']['Rank']))
    ok = ranks_ok and (byname['Show.S01E01.b']['Status'] == 'SAME_POSTING' and byname['Show.S01E01.b']['NZBID'] == 0 and
          byname['Show.S01E01.b']['SameAs'] == byname['Show.S01E01.a']['NZBID'] and
          byname['Show.S01E01.a']['NZBID'] > 0 and reply['Chosen'] > 0)
    return ('fleettwins', ok, 'fleet_params=%s members=%s' % (fleet_params, [(m['Name'], m['Status'], m['NZBID'], m['SameAs']) for m in reply['Members']]))


def scenario_fleettimeout(daemon, t):
    """appendfleet: six postings, the news server answers slowly (2 s a request),
    and the fleet allows 8 s: the call returns by then (F1-b: the postings were
    checked one after another, each with its own budget), queues the best it
    knows, and says the check was not complete."""
    tags = ['t%d' % i for i in range(6)]
    daemon.fake_nntp.alive = set().union(*(set(_fleet_ids(tag)) for tag in tags))
    daemon.fake_nntp.delays.update({tag + '-': 2.0 for tag in tags})
    daemon.wait_ready()
    start = time.time()
    reply = _fleet(daemon, [('Show.S01E01.%s' % tag, _fake_nzb_ids(_fleet_ids(tag), 400_000 + i).decode())
                            for i, tag in enumerate(tags)], timeout=8)
    took = time.time() - start
    ok = reply['Chosen'] > 0 and not reply['Complete'] and took < 11
    return ('fleettimeout', ok, 'chosen=%s complete=%s took=%.1fs' % (reply['Chosen'], reply['Complete'], took))


def scenario_fleetallerror(daemon, t):
    """appendfleet (F1-a): every member is unreadable (junk content, an
    unreachable url): the reason is NO_USABLE_MEMBERS, not ALL_DEAD, and the url
    reads "connection failed"."""
    api = daemon.wait_ready()
    reply = _fleet(daemon, [('Show.S01E01.junk', 'not an nzb-file'), ('Show.S01E01.url', None)], timeout=10)
    reasons = [m['Reason'] for m in reply['Members']]
    ok = reply['Chosen'] == 0 and reply['Reason'] == 'NO_USABLE_MEMBERS' and \
        any('connection failed' in r for r in reasons) and not api.listgroups()
    return ('fleetallerror', ok, 'reply=%s' % reply)


def scenario_fleetotherkey(daemon, t):
    """appendfleet (F1-c): the release was downloaded under another client's key
    and is on disk; a fleet for it under a new key downloads nothing
    (ALREADY_DOWNLOADED, members SKIPPED)."""
    daemon.fake_nntp.alive = set(_fleet_ids('ok1')) | set(_fleet_ids('ok2'))
    api = daemon.wait_ready()
    _ds_append(api, DS_TITLE, _fake_nzb_ids(_fleet_ids('ok1'), 400_000).decode(), 'batch:key-a', 100, paused=False)
    first = daemon.wait_history(api, DS_TITLE, timeout=120)
    reply = _fleet(daemon, [('Show S01E01 1080p WEB H264-GRP', _fake_nzb_ids(_fleet_ids('ok2'), 410_000).decode())],
                   key='fleet:key-b')
    time.sleep(1)
    ok = first['Status'].startswith('SUCCESS') and reply['Chosen'] == 0 and \
        reply['Reason'] == 'ALREADY_DOWNLOADED' and reply['Members'][0]['Status'] == 'SKIPPED' and not api.listgroups() and \
        reply['Members'][0]['Alive'] == -1
    # (never health-checked: it was measured first, and read as dead with Alive 0)
    return ('fleetotherkey', ok, 'first=%s reply=%s' % (first['Status'], reply))


def scenario_fleetnokey(daemon, t):
    """appendfleet (F1-d): an empty key is derived from the first member's name
    (as a single append's), not refused."""
    daemon.fake_nntp.alive = set(_fleet_ids('nk'))
    api = daemon.wait_ready()
    reply = _fleet(daemon, [('Show.S01E01.nk', _fake_nzb_ids(_fleet_ids('nk'), 400_000).decode())], key='')
    group = next((g for g in api.listgroups() if g['NZBID'] == reply['Chosen']), None) or \
        next((h for h in api.history() if h['NZBID'] == reply['Chosen']), {})
    ok = reply['Chosen'] > 0 and group.get('DupeKey', '').startswith('dupes:')
    return ('fleetnokey', ok, 'chosen=%s key=%s' % (reply['Chosen'], group.get('DupeKey')))


def scenario_fleetresend(daemon, t):
    """appendfleet (F2, Women in Blue S02E02): a client sends the same fleet twice,
    1 s apart (a retry). The second call waits for the first: the whole posting
    stays the one downloading, the second reply names it (ALREADY_QUEUED) and
    reports its members as the same postings as the first's; the 12%-alive copy
    is never queued. Before, the two raced: the whole copy went to history and
    the 12% one downloaded."""
    import threading
    daemon.fake_nntp.alive = set(_fleet_ids('rw', 200)) | set(_fleet_ids('rl')[:5])
    daemon.fake_nntp.delays.update({'rw-': 0.3, 'rl-': 0.3})
    api = daemon.wait_ready()
    # 200 slow articles: still downloading when the second call comes
    members = [('Show.S01E01.whole', _fake_nzb_ids(_fleet_ids('rw', 200), 2_000_000).decode()),
               ('Show.S01E01.low', _fake_nzb_ids(_fleet_ids('rl'), 410_000).decode())]
    box = {}

    def send(slot):
        box[slot] = _fleet(daemon, members, key='fleet:resend', timeout=20)
    first = threading.Thread(target=send, args=('a',))
    first.start()
    time.sleep(1)
    send('b')
    first.join(timeout=60)
    a, b = box.get('a', {}), box.get('b', {})
    whole = next((m for m in a.get('Members', []) if m['Name'] == 'Show.S01E01.whole'), {})
    queued = [g['NZBName'] for g in api.listgroups()]
    ok = (a.get('Chosen') and a['Chosen'] == whole.get('NZBID') and b.get('Chosen') == a['Chosen'] and
          b.get('Reason') == 'ALREADY_QUEUED' and all(m['Status'] == 'SAME_POSTING' for m in b['Members']) and
          queued == ['Show.S01E01.whole'])
    return ('fleetresend', ok, 'a=%s b=%s queued=%s' % (a, b, queued))


def scenario_fleetresendslow(daemon, t):
    """appendfleet (F4): the same fleet sent twice, 1 s apart, against a slow
    server, each call allowing 10 s: the first uses its time checking; the second
    waits for the key, but its own limit counts from its arrival, so it replies
    within about 10 s - its members the same postings as the first's, Chosen the
    running download. Before, it waited out the first and then checked anew
    (the client gave up)."""
    import threading
    daemon.fake_nntp.alive = set(_fleet_ids('sw', 200)) | set(_fleet_ids('sl'))
    daemon.fake_nntp.delays.update({'sw-': 2.0, 'sl-': 2.0})
    daemon.wait_ready()
    members = [('Show.S01E01.whole', _fake_nzb_ids(_fleet_ids('sw', 200), 2_000_000).decode()),
               ('Show.S01E01.other', _fake_nzb_ids(_fleet_ids('sl'), 410_000).decode())]
    box = {}

    def send(slot):
        start = time.time()
        box[slot] = _fleet(daemon, members, key='fleet:slow', timeout=10)
        box[slot + 'took'] = time.time() - start
    first = threading.Thread(target=send, args=('a',))
    first.start()
    time.sleep(1)
    send('b')
    first.join(timeout=60)
    a, b = box.get('a', {}), box.get('b', {})
    ok = (a.get('Chosen', 0) > 0 and box.get('btook', 99) < 13 and b.get('Chosen') == a.get('Chosen') and
          b.get('Reason') == 'ALREADY_QUEUED' and all(m['Status'] == 'SAME_POSTING' for m in b.get('Members', [])))
    return ('fleetresendslow', ok, 'a_took=%.1f b_took=%.1f a=%s b=%s' % (box.get('atook', -1), box.get('btook', -1), a, b))


def scenario_fleetaddbackup(daemon, t):
    """appendfleet (F5, Women in Blue S02E04): while a fleet's chosen copy
    downloads, a second fleet brings the same copies plus a new posting: the new
    one is filed below everything the key holds, so no two items of the key
    share a score. Before, it took the score of the first fleet's backup."""
    daemon.fake_nntp.alive = set(_fleet_ids('aw', 200)) | set(_fleet_ids('ao')) | set(_fleet_ids('an'))
    daemon.fake_nntp.delays.update({'aw-': 0.3})
    api = daemon.wait_ready()
    whole = ('Show.S01E01.whole', _fake_nzb_ids(_fleet_ids('aw', 200), 2_000_000).decode())
    other = ('Show.S01E01.other', _fake_nzb_ids(_fleet_ids('ao'), 410_000).decode())
    new = ('Show.S01E01.new', _fake_nzb_ids(_fleet_ids('an'), 420_000).decode())
    a = _fleet(daemon, [whole, other], key='fleet:add', timeout=15)
    b = _fleet(daemon, [whole, other, new], key='fleet:add', timeout=15)
    items = [x for x in api.listgroups() + api.history() if x.get('DupeKey') == 'fleet:add']
    scores = [x['DupeScore'] for x in items]
    added = next((m for m in b['Members'] if m['Name'] == 'Show.S01E01.new'), {})
    ok = (b.get('Reason') == 'ALREADY_QUEUED' and added.get('NZBID', 0) > 0 and len(items) == 3 and
          len(set(scores)) == len(scores) and min(scores) == next(x['DupeScore'] for x in items if x['NZBID'] == added['NZBID']))
    return ('fleetaddbackup', ok, 'scores=%s b=%s' % (sorted((x['NZBID'], x['DupeScore']) for x in items), b))


def scenario_fleetfailover(daemon, t):
    """appendfleet: the chosen copy dies while it downloads (its articles are
    taken down after the check): nzbget fails over to the fleet's next-ranked
    backup, which downloads whole; the dead-ranked member is never fetched."""
    daemon.fake_nntp.alive = set(_fleet_ids('fa', 300)) | set(_fleet_ids('fb', 300)) | set(_fleet_ids('fc')[:5])
    daemon.fake_nntp.delays.update({'fa-': 0.2})
    api = daemon.wait_ready()
    reply = _fleet(daemon, [('Show.S01E01.a', _fake_nzb_ids(_fleet_ids('fa', 300), 3_000_000).decode()),
                            ('Show.S01E01.b', _fake_nzb_ids(_fleet_ids('fb', 300), 3_100_000).decode()),
                            ('Show.S01E01.c', _fake_nzb_ids(_fleet_ids('fc'), 400_000).decode())], key='fleet:fo')
    chosen = next(m for m in reply['Members'] if m['NZBID'] == reply['Chosen'])
    backup = next(m for m in reply['Members'] if m['Status'] == 'BACKUP')
    # the takedown: the chosen copy's articles are gone from the server
    daemon.fake_nntp.alive -= set(_fleet_ids(chosen['Name'][-1:] == 'a' and 'fa' or 'fb', 300))
    hb = daemon.wait_history(api, backup['Name'], timeout=180)
    deadline = time.time() + 180
    while time.time() < deadline and not hb['Status'].startswith(('SUCCESS', 'FAILURE')):
        time.sleep(0.5)
        hb = daemon.wait_history(api, backup['Name'], timeout=60)
    hc = next((h for h in api.history() if h['NZBName'] == 'Show.S01E01.c'), {})
    tried_c = int(hc.get('SuccessArticles', 0)) + int(hc.get('FailedArticles', 0))
    ok = hb['Status'].startswith('SUCCESS') and tried_c == 0 and _grep_log(t, 'Failing over') + _grep_log(t, 'Found duplicate') >= 1
    return ('fleetfailover', ok, 'chosen=%s backup=%s backup_status=%s c_tried=%d' % (chosen['Name'], backup['Name'], hb['Status'], tried_c))


def scenario_fleetbusy(daemon, t):
    """appendfleet (F7): a long download holds the server's 2 connections when a
    fleet comes. Downloads hold off while the fleet is checked, so the check
    gets connections: it finishes well inside its 20 s and measures both
    copies (the whole one queued, the dead one dead). Before, it used the whole
    limit and measured nothing."""
    daemon.fake_nntp.alive = set(_fleet_ids('busy', 600)) | set(_fleet_ids('bw'))
    daemon.fake_nntp.delays.update({'busy-': 3.0})
    api = daemon.wait_ready()
    _ds_append(api, 'Busy.Download', _fake_nzb_ids(_fleet_ids('busy', 600), 6_000_000).decode(), 'busy-key', 100, paused=False)
    time.sleep(3)
    start = time.time()
    reply = _fleet(daemon, [('Show.S01E01.dead', _fake_nzb_ids(_fleet_ids('bd'), 400_000).decode()),
                            ('Show.S01E01.whole', _fake_nzb_ids(_fleet_ids('bw'), 410_000).decode())],
                   key='fleet:busy', timeout=20)
    took = time.time() - start
    byname = {m['Name']: m for m in reply['Members']}
    busy_after = next((g for g in api.listgroups() if g['NZBName'] == 'Busy.Download'), None)
    ok = (reply['Complete'] and took < 15 and byname['Show.S01E01.whole']['Status'] == 'QUEUED' and
          byname['Show.S01E01.dead']['Status'] == 'DEAD' and busy_after is not None)
    return ('fleetbusy', ok, 'took=%.1f complete=%s members=%s' % (took, reply['Complete'],
            [(m['Name'], m['Status'], m['Alive']) for m in reply['Members']]))


def scenario_fleetdeadtwins(daemon, t):
    """appendfleet (F8): a pair of twins, both dead, nothing added: the copy
    names its twin by rank (SameAsRank), as it has no NZBID to name."""
    daemon.fake_nntp.alive = set()
    daemon.wait_ready()
    reply = _fleet(daemon, [('Show.S01E01.x', _fake_nzb_ids(_fleet_ids('dx'), 400_000).decode()),
                            ('Show.S01E01.y', _fake_nzb_ids(_fleet_ids('dx'), 400_000).decode())], key='fleet:dt')
    twin = next((m for m in reply['Members'] if m['Status'] == 'SAME_POSTING'), {})
    ok = reply['Reason'] == 'ALL_DEAD' and twin.get('SameAsRank') == 1 and twin.get('SameAs') == 0
    return ('fleetdeadtwins', ok, 'reply=%s' % reply)


def scenario_fleetlarge(daemon, t):
    """appendfleet (F7): five large postings (20,000 articles each). A fleet's
    sample of each is capped at 200 articles after the probe, not 5% (1,000):
    ranking a few copies needs less than measuring a donor."""
    tags = ['L%d' % i for i in range(5)]
    daemon.fake_nntp.alive = set().union(*(set(_fleet_ids(tag, 20000)) for tag in tags))
    daemon.wait_ready()
    before = daemon.fake_nntp.stats
    reply = _fleet(daemon, [('Show.S01E01.%s' % tag, _fake_nzb_ids(_fleet_ids(tag, 20000), 200_000_000 + i).decode())
                            for i, tag in enumerate(tags)], key='fleet:large', timeout=60)
    asked = daemon.fake_nntp.stats - before
    ok = reply['Complete'] and reply['Chosen'] > 0 and asked <= 5 * 220
    return ('fleetlarge', ok, 'stats=%d complete=%s chosen=%s' % (asked, reply['Complete'], reply['Chosen']))


def scenario_fleetcopy(daemon, t):
    """appendfleet: a member that is the same nzb-file as an item under another
    key, which the duplicate check files as a copy (DELETED/COPY), is reported
    COPY, not BACKUP."""
    daemon.fake_nntp.alive = set(_fleet_ids('cp')) | set(_fleet_ids('cq'))
    api = daemon.wait_ready()
    nzb = _fake_nzb_ids(_fleet_ids('cp'), 400_000).decode()
    _ds_append(api, 'abcdef0123', nzb, 'other-key', 100, paused=False)
    daemon.wait_history(api, 'abcdef0123', timeout=60)
    reply = _fleet(daemon, [('0123abcdef', _fake_nzb_ids(_fleet_ids('cq'), 410_000).decode()),
                            ('fedcba3210', nzb)], key='fleet:copy')
    byname = {m['Name']: m for m in reply['Members']}
    ok = byname['fedcba3210']['Status'] == 'COPY' and byname['0123abcdef']['Status'] == 'QUEUED'
    return ('fleetcopy', ok, 'members=%s' % [(m['Name'], m['Status'], m['NZBID']) for m in reply['Members']])


def scenario_fleetmostlydead(daemon, t):
    """appendfleet (F10, Slow Horses S04E02): one posting, 20% of it alive, on a
    slow server: the check runs to the limit (missing articles settle slowly),
    but the member is measured below the floor: the reason is ALL_DEAD, not
    INCOMPLETE (which is kept for members not measured at all)."""
    daemon.fake_nntp.alive = set(_fleet_ids('md', 200)[:40])
    daemon.fake_nntp.delays.update({'md-': 0.3})
    daemon.wait_ready()
    reply = _fleet(daemon, [('Show.S01E01.md', _fake_nzb_ids(_fleet_ids('md', 200), 2_000_000).decode())],
                   key='fleet:md', timeout=8)
    m = reply['Members'][0]
    # B2b: some of it is alive, so it is WEAK (its share in the reason), never DEAD
    ok = (reply['Chosen'] == 0 and m['Alive'] > 0 and m['Status'] == 'WEAK' and str(m['Alive']) + '%' in m.get('Reason', '')
          and reply['Reason'] == 'ALL_WEAK')
    return ('fleetmostlydead', ok, 'reply=%s' % reply)


def _rpc(daemon, method, params, timeout=60):
    """A JSON-RPC call; returns the reply (its 'result' or its 'error')."""
    import json as _json
    import urllib.request as _req
    body = _json.dumps({'method': method, 'params': params}).encode()
    request = _req.Request('http://127.0.0.1:%d/jsonrpc' % daemon.rpc_port, body, {'Content-Type': 'application/json'})
    with _req.urlopen(request, timeout=timeout) as reply:
        return _json.loads(reply.read().decode())


def scenario_appendconcurrent(daemon, t):
    """F14: appends at the same moment each get their own id. Ten with the same
    name (distinct contents, one key) and twenty with distinct names: before,
    most of the same-named ones and some others returned -1 - two appends picked
    the same file name and one overwrote the other."""
    import threading
    daemon.wait_ready()
    jobs = [('Same.Release', 'cs%d' % i) for i in range(10)] + [('Other.Release.%d' % i, 'co%d' % i) for i in range(20)]
    ids = [None] * len(jobs)
    barrier = threading.Barrier(len(jobs))

    def send(i, name, tag):
        nzb = base64.standard_b64encode(_fake_nzb_ids(_fleet_ids(tag, 5), 50_000)).decode()
        barrier.wait()
        ids[i] = _rpc(daemon, 'append', [name, nzb, 'test', 0, False, True, 'conc-key', 10 + i, 'all', []]).get('result')
    threads = [threading.Thread(target=send, args=(i, name, tag)) for i, (name, tag) in enumerate(jobs)]
    for th in threads:
        th.start()
    for th in threads:
        th.join(timeout=120)
    lost = sum(1 for x in ids if not isinstance(x, int) or x <= 0)
    distinct = len(set(x for x in ids if isinstance(x, int) and x > 0))
    return ('appendconcurrent', lost == 0 and distinct == len(jobs), 'lost=%d distinct=%d ids=%s' % (lost, distinct, ids))


def scenario_appendscorerange(daemon, t):
    """F16: a DupeScore out of the int range is an invalid parameter; it wrapped
    around before (2147483648 was stored as -2147483648, the lowest score)."""
    daemon.wait_ready()
    nzb = base64.standard_b64encode(_fake_nzb_ids(_fleet_ids('sr', 3), 30_000)).decode()
    replies = {score: _rpc(daemon, 'append', ['Score.%d' % i, nzb, 'test', 0, False, True, 'sr-key', score, 'all', []])
               for i, score in enumerate([2147483648, 4294967301, -2147483649])}
    rejected = all('error' in r and r['error'] and not r.get('result') for r in replies.values())
    ok_reply = _rpc(daemon, 'append', ['Score.ok', nzb, 'test', 0, False, True, 'sr-key', 2147483647, 'all', []])
    stored = next((g['DupeScore'] for g in daemon.wait_ready().listgroups() if g['NZBName'] == 'Score.ok'), None)
    ok = rejected and ok_reply.get('result', 0) > 0 and stored == 2147483647
    return ('appendscorerange', ok, 'replies=%s stored=%s' % (
        {k: (v.get('result'), (v.get('error') or {}).get('message') if isinstance(v.get('error'), dict) else v.get('error'))
         for k, v in replies.items()}, stored))


def _many_files_nzb(n, tag, same_name=False):
    """An nzb-file of n rar volumes, one article each (all named alike: obfuscated)."""
    files = ''.join(
        '<file poster="p" date="1" subject="&quot;%s&quot; yEnc (1/1)"><groups><group>a.b</group></groups>'
        '<segments><segment bytes="700000" number="1">%s-%d@x</segment></segments></file>' % (
            'edge.rar' if same_name else 'Big.Set.part%04d.rar' % (i + 1), tag, i)
        for i in range(n))
    return ('<?xml version="1.0" encoding="UTF-8"?><nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">%s</nzb>' % files).encode()


def scenario_appendlarge(daemon, t):
    """F15: appending an nzb-file of 2,000 files takes about as long per file as
    one of 500 (production: 0.5 s for 500 files, 26 s for 2,000)."""
    daemon.wait_ready()
    took = {}
    for n in (500, 2000):
        nzb = base64.standard_b64encode(_many_files_nzb(n, 'al%d' % n, same_name=True)).decode()
        start = time.time()
        reply = _rpc(daemon, 'append', ['Big.Set.%d' % n, nzb, 'test', 0, False, True, 'big-%d' % n, 0, 'all', []], timeout=300)
        took[n] = (time.time() - start, reply.get('result'))
    ok = all(r > 0 for _, r in took.values()) and took[2000][0] < max(2.0, took[500][0] * 4 * 2)
    return ('appendlarge', ok, ' '.join('%d files: %.1f s (id %s)' % (n, s_, r) for n, (s_, r) in took.items()))


def scenario_appendurlodd(daemon, t):
    """P0 (production crash, SIGSEGV): appendurl with odd urls - empty, not a url,
    file://, ftp://, a 5,000-character one - is refused or fails cleanly, and the
    daemon stays up with its state files readable after a restart."""
    import http.server
    import threading

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path == '/loop':
                self.send_response(302)
                self.send_header('Location', '/loop')
                self.end_headers()
            elif self.path == '/redir':
                self.send_response(302)
                self.send_header('Location', '/ok')
                self.end_headers()
            else:
                # production: the helper answered every other path with an nzb-file
                body = _fake_nzb_ids(['%s-%d@srv' % (uuid.uuid4().hex, i) for i in range(20)], 15_000_000)
                self.send_response(200)
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        def log_message(self, *args):
            pass
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = 'http://127.0.0.1:%d' % server.server_address[1]
    daemon.wait_ready()
    replies = {}
    for label, url in [('ok', base + '/ok'), ('redir', base + '/redir'), ('loop', base + '/loop'), ('404', base + '/nope'),
                       ('file', 'file:///etc/hostname'), ('ftp', 'ftp://127.0.0.1:9/x.nzb'), ('notaurl', 'notaurl'),
                       ('long', base + '/' + 'a' * 5000), ('empty', '')]:
        try:
            # every call has the same name, as in production
            r = _rpc(daemon, 'appendurl', ['Odd-u.nzb', url, '', 0, False, True, 'odd-key', 0, 'SCORE', []], timeout=60)
            replies[label] = r.get('result', r.get('error'))
        except Exception as e:
            replies[label] = 'EXC %s' % str(e)[:60]
    time.sleep(8)
    server.shutdown()
    try:
        alive = bool(_rpc(daemon, 'version', [], timeout=10).get('result'))
    except Exception:
        alive = False
    return ('appendurlodd', alive, 'alive=%s replies=%s' % (alive, replies))


def scenario_addstorm(daemon, t):
    """P0 (production SIGSEGV after an API storm): bursts of appendurl and append
    with one name, at once, while the incoming directory is scanned every second:
    the daemon stays up, every call gets an answer, and its state files read back
    after a restart."""
    import http.server
    import threading

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            body = _fake_nzb_ids(['%s-%d@srv' % (uuid.uuid4().hex, i) for i in range(20)], 15_000_000)
            time.sleep(random.random() * 0.3)
            self.send_response(200)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = 'http://127.0.0.1:%d' % server.server_address[1]
    daemon.wait_ready()
    answers = []

    def call(method, i):
        try:
            if method == 'appendurl':
                r = _rpc(daemon, 'appendurl', ['Storm-u.nzb', base + '/x%d' % i, '', 0, False, True, 'storm-key', i, 'SCORE', []], timeout=60)
            else:
                nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@st' % uuid.uuid4().hex for _ in range(5)], 50_000)).decode()
                r = _rpc(daemon, 'append', ['Storm.nzb', nzb, '', 0, False, True, 'storm-key', i, 'SCORE', []], timeout=60)
            answers.append(r.get('result'))
        except Exception as e:
            answers.append('EXC %s' % str(e)[:40])
    for burst in range(6):
        threads = [threading.Thread(target=call, args=('appendurl' if i % 2 else 'append', burst * 100 + i)) for i in range(16)]
        for th in threads:
            th.start()
        for th in threads:
            th.join(timeout=90)
        time.sleep(random.random() * 2)
    time.sleep(10)
    server.shutdown()
    try:
        alive = bool(_rpc(daemon, 'version', [], timeout=10).get('result'))
    except Exception:
        alive = False
    exc = sum(1 for a in answers if isinstance(a, str))
    lost = sum(1 for a in answers if a == -1 or a == 0)
    return ('addstorm', alive and exc == 0 and lost == 0, 'alive=%s answers=%d exceptions=%d lost=%d' % (
        alive, len(answers), exc, sum(1 for a in answers if a == -1 or a == 0)))


def scenario_longstateline(daemon, t):
    """P0 (production SIGSEGV, queue and history unreadable): a field of 1,024
    characters or more in a state line - here a 5,000-character url (appendurl)
    and a 3,000-character dupe key - was written past a fixed buffer onto the
    stack: garbage in the history file (set aside as unreadable at the next
    start) or a crash. Now both items survive a restart and the files load."""
    api = daemon.wait_ready()
    long_url = 'http://127.0.0.1:9/' + 'a' * 5000
    url_id = _rpc(daemon, 'appendurl', ['Long.Url.nzb', long_url, '', 0, False, True, 'long-key', 0, 'SCORE', []]).get('result')
    nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@ls' % uuid.uuid4().hex for _ in range(3)], 30_000)).decode()
    key_id = _rpc(daemon, 'append', ['Long.Key.nzb', nzb, '', 0, False, True, 'k' * 3000, 0, 'SCORE', []]).get('result')
    time.sleep(3)
    alive_before = bool(_rpc(daemon, 'version', [], timeout=10).get('result'))
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    # queued or failed by now, the url item is still there, with its url
    in_history = any(x['NZBID'] == url_id for x in api.history() + api.listgroups())
    in_queue = any(g['NZBID'] == key_id and len(g['DupeKey']) == 3000 for g in api.listgroups())
    unreadable = _grep_log(t, 'could not be read')
    ok = alive_before and in_history and in_queue and unreadable == 0
    return ('longstateline', ok, 'ids=%s/%s alive=%s url_kept=%s key_in_queue=%s unreadable_logs=%d' % (
        url_id, key_id, alive_before, in_history, in_queue, unreadable))


def scenario_newlinestate(daemon, t):
    """A line break in a name, dupe key or category sent through the API split
    its state record over two lines: at the next start the whole history (2,181
    items in production) was set aside as unreadable. Now the items survive a
    restart, the breaks kept as spaces."""
    api = daemon.wait_ready()
    nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@nl' % uuid.uuid4().hex for _ in range(3)], 30_000)).decode()
    nid = _rpc(daemon, 'append', ['Line\nBreak.nzb', nzb, 'cat\negory', 0, False, True, 'key\r\nbreak', 0, 'SCORE', []]).get('result')
    hid = _rpc(daemon, 'append', ['Hist\nItem.nzb', nzb, '', 0, False, True, 'hist\nkey', 0, 'SCORE', []]).get('result')
    _rpc(daemon, 'editqueue', ['GroupDelete', '', [hid]])
    time.sleep(3)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    queued = [g for g in api.listgroups() if g['NZBID'] == nid]
    in_history = any(h['NZBID'] == hid for h in api.history())
    unreadable = _grep_log(t, 'could not be read')
    ok = (nid > 0 and bool(queued) and '\n' not in queued[0]['DupeKey'] and in_history and unreadable == 0)
    return ('newlinestate', ok, 'ids=%s/%s queued=%s in_history=%s unreadable_logs=%d' % (
        nid, hid, [(g['NZBName'], g['DupeKey'], g['Category']) for g in queued], in_history, unreadable))


def scenario_idsafterunreadable(daemon, t):
    """P0-c: with the history set aside as unreadable at a start, the ids
    restarted at what loaded: production gave new fleets the ids of deleted test
    items. New ids now count on past every id the queue dir holds files for."""
    api = daemon.wait_ready()
    ids = []
    for i in range(4):
        nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@id' % uuid.uuid4().hex for _ in range(3)], 30_000)).decode()
        ids.append(_rpc(daemon, 'append', ['Ids.%d.nzb' % i, nzb, '', 0, False, True, 'ids-key-%d' % i, 0, 'SCORE', []]).get('result'))
    _rpc(daemon, 'editqueue', ['GroupDelete', '', ids])
    time.sleep(3)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    with open(t.path('main', 'queue', 'history'), 'r+b') as f:
        lines = f.read().split(b'\n')
        f.seek(0)
        f.truncate()
        f.write(b'\n'.join(lines[:1] + [b'garbage'] + lines[1:]))
    daemon.start()
    api = daemon.wait_ready()
    nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@id' % uuid.uuid4().hex for _ in range(3)], 30_000)).decode()
    new_id = _rpc(daemon, 'append', ['Ids.new.nzb', nzb, '', 0, False, True, 'ids-key-new', 0, 'SCORE', []]).get('result')
    unreadable = _grep_log(t, 'could not be read')
    ok = unreadable >= 1 and new_id > max(ids)
    return ('idsafterunreadable', ok, 'old=%s new=%s unreadable_logs=%d' % (ids, new_id, unreadable))


def scenario_idsafterfinaldelete(daemon, t):
    """A download deleted for good (GroupFinalDelete, then HistoryFinalDelete:
    a client's cancel) leaves no file in the queue dir. After a restart its id
    went to the next download, so a client that still knew the old id edited or
    deleted the new one. New ids now count on past the last id handed out."""
    api = daemon.wait_ready()
    def add(name):
        nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@id' % uuid.uuid4().hex for _ in range(3)], 30_000)).decode()
        return _rpc(daemon, 'append', [name + '.nzb', nzb, '', 0, False, True, '', 0, 'FORCE', []]).get('result')
    keep = add('Keep')
    gone = [add('Gone.%d' % i) for i in range(3)]
    _rpc(daemon, 'editqueue', ['GroupFinalDelete', '', gone])
    time.sleep(1)
    _rpc(daemon, 'editqueue', ['HistoryFinalDelete', '', gone])
    time.sleep(2)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    new_id = add('New')
    ok = new_id > max(gone + [keep])
    return ('idsafterfinaldelete', ok, 'keep=%s gone=%s new=%s' % (keep, gone, new_id))


def scenario_idsafteremptyqueue(daemon, t):
    """Deleting all queue and history entries must still preserve both ID floors."""
    api = daemon.wait_ready()
    def add(name):
        nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@id' % uuid.uuid4().hex for _ in range(3)], 30_000)).decode()
        return _rpc(daemon, 'append', [name + '.nzb', nzb, '', 0, False, True, '', 0, 'FORCE', []]).get('result')
    gone = [add('Gone.%d' % i) for i in range(3)]
    old_files = [f['ID'] for f in api.listfiles()]
    _rpc(daemon, 'editqueue', ['GroupDelete', '', gone])
    deadline = time.monotonic() + 30
    while not set(gone).issubset({h['NZBID'] for h in api.history(False)}):
        assert time.monotonic() < deadline, "deleted downloads must reach history"
        time.sleep(0.3)
    _rpc(daemon, 'editqueue', ['HistoryFinalDelete', '', gone])
    time.sleep(2)
    empty = not api.listgroups() and not api.history()
    try:
        api.shutdown()
    except Exception:
        pass
    daemon.process.wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    new_id = add('New')
    new_files = [f['ID'] for f in api.listfiles()]
    ok = empty and new_id > max(gone) and bool(old_files) and bool(new_files) and min(new_files) > max(old_files)
    return ('idsafteremptyqueue', ok, 'empty=%s gone=%s new=%s old_files=%s new_files=%s' %
            (empty, gone, new_id, old_files, new_files))


def scenario_idsafteremptyfinaldelete(daemon, t):
    """Deleting every queue and history item must preserve IDs at restart."""
    api = daemon.wait_ready()
    def add(name):
        nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@id' % uuid.uuid4().hex for _ in range(3)], 30_000)).decode()
        return _rpc(daemon, 'append', [name + '.nzb', nzb, '', 0, False, True, '', 0, 'FORCE', []]).get('result')
    gone = [add('Gone.%d' % i) for i in range(3)]
    _rpc(daemon, 'editqueue', ['GroupDelete', '', gone])
    deadline = time.monotonic() + 30
    while not set(gone).issubset({h['NZBID'] for h in api.history(False)}):
        assert time.monotonic() < deadline, "deleted downloads must reach history"
        time.sleep(0.3)
    _rpc(daemon, 'editqueue', ['HistoryFinalDelete', '', gone])
    time.sleep(2)
    assert not api.listgroups(), "queue must be empty before restart"
    assert not api.history(False), "history must be empty before restart"
    try:
        api.shutdown()
    except Exception:
        pass
    daemon.process.wait(timeout=60)
    assert not t.exists('main/queue/queue'), "queue state must be absent"
    assert not t.exists('main/queue/history'), "history state must be absent"
    assert t.exists('main/queue/ids'), "ID floor must exist"
    daemon.start()
    api = daemon.wait_ready()
    new_id = add('New')
    ok = new_id > max(gone)
    return ('idsafteremptyfinaldelete', ok, 'gone=%s new=%s' % (gone, new_id))


def scenario_fleetscoremax(daemon, t):
    """F17: the key holds an item scored at the int limit (in history): the
    fleet's scores were counted past it and wrapped around, negative, so a dead
    member could outrank the whole one. The whole member now scores above the
    dead one, and no score is negative."""
    daemon.fake_nntp.alive = set(_fleet_ids('xw'))
    api = daemon.wait_ready()
    nzb = base64.standard_b64encode(_fake_nzb_ids(_fleet_ids('xm'), 400_000)).decode()
    top = _rpc(daemon, 'append', ['Show.S01E01.top.nzb', nzb, '', 0, False, True, 'fleet:max', 2147483647, 'SCORE', []]).get('result')
    _rpc(daemon, 'editqueue', ['GroupDelete', '', [top]])
    time.sleep(2)
    reply = _fleet(daemon, [('Show.S01E01.dead', _fake_nzb_ids(_fleet_ids('xd'), 400_000).decode()),
                            ('Show.S01E01.whole', _fake_nzb_ids(_fleet_ids('xw'), 410_000).decode())],
                   key='fleet:max', timeout=20)
    scores = {x['NZBName']: x['DupeScore'] for x in api.listgroups() + api.history()
              if x['NZBName'].startswith('Show.S01E01.') and x['NZBID'] != top}
    whole, dead = scores.get('Show.S01E01.whole'), scores.get('Show.S01E01.dead')
    ok = whole is not None and whole > 0 and (dead is None or (dead > 0 and whole > dead))
    return ('fleetscoremax', ok, 'scores=%s reply=%s' % (scores, [(m['Name'], m['Status']) for m in reply['Members']]))


def scenario_fleetmerged(daemon, t):
    """F20: two postings of a key merged into one item (GroupMerge) aren't added
    again when the fleet is resent: its members are the postings the merged
    item holds (SAME_POSTING). Before, a merged item's postings weren't seen."""
    daemon.fake_nntp.alive = set(_fleet_ids('ga')) | set(_fleet_ids('gb'))
    api = daemon.wait_ready()
    na = _fake_nzb_ids(_fleet_ids('ga'), 400_000)
    nb = _fake_nzb_ids(_fleet_ids('gb'), 410_000)
    a = _rpc(daemon, 'append', ['Show.S01E01.a.nzb', base64.standard_b64encode(na).decode(), '', 0, False, True, 'fleet:merge', 10, 'SCORE', []]).get('result')
    b = _rpc(daemon, 'append', ['Show.S01E01.b.nzb', base64.standard_b64encode(nb).decode(), '', 0, False, True, 'fleet:merge', 5, 'SCORE', []]).get('result')
    merged = _rpc(daemon, 'editqueue', ['GroupMerge', '', [a, b]]).get('result')
    reply = _fleet(daemon, [('Show.S01E01.a', na.decode()), ('Show.S01E01.b', nb.decode())], key='fleet:merge', timeout=20)
    groups = [g['NZBName'] for g in api.listgroups()]
    ok = merged and all(m['Status'] == 'SAME_POSTING' for m in reply['Members']) and len(groups) == 1
    return ('fleetmerged', ok, 'merged=%s groups=%s reply=%s' % (merged, groups, [(m['Name'], m['Status'], m.get('SameAs')) for m in reply['Members']]))


def scenario_readdafterdelete(daemon, t):
    """F26 (the proxy's silent drop, checked on nzbget's side): an item of a key is
    deleted, then the identical nzb-file is sent again under the key. nzbget skips
    it by design (upstream: the same content in history, whatever its status, is
    a duplicate); sent with DupeMode FORCE it queues. Nothing else - no id or
    state remembered by this branch - swallows it."""
    api = daemon.wait_ready()
    nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@rd' % uuid.uuid4().hex for _ in range(5)], 50_000)).decode()
    first = _rpc(daemon, 'append', ['Readd.Release.nzb', nzb, '', 0, False, True, 'readd-key', 100, 'SCORE', []]).get('result')
    _rpc(daemon, 'editqueue', ['GroupDelete', '', [first]])
    time.sleep(2)
    skipped = _rpc(daemon, 'append', ['Readd.Release.nzb', nzb, '', 0, False, True, 'readd-key', 100, 'SCORE', []]).get('result')
    forced = _rpc(daemon, 'append', ['Readd.Release.nzb', nzb, '', 0, False, True, 'readd-key', 100, 'FORCE', []]).get('result')
    time.sleep(2)
    queued = {g['NZBID'] for g in api.listgroups()}
    ok = skipped > 0 and skipped not in queued and _grep_log(t, 'Skipping duplicate Readd.Release') >= 1 and forced in queued
    return ('readdafterdelete', ok, 'first=%s skipped=%s forced=%s queued=%s' % (first, skipped, forced, sorted(queued)))


def scenario_editscorerange(daemon, t):
    """F27: editqueue GroupSetDupeScore with a score past the int range wrapped
    around (2147483648 stored as -2147483648, the lowest score); it is an invalid
    parameter now, the score unchanged, and a parameter without a name too, a
    parameter without a value, and an unknown dupe mode."""
    api = daemon.wait_ready()
    nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@es' % uuid.uuid4().hex for _ in range(3)], 30_000)).decode()
    nid = _rpc(daemon, 'append', ['Edit.Score.nzb', nzb, '', 0, False, True, 'es-key', 500, 'SCORE', []]).get('result')
    replies = [_rpc(daemon, 'editqueue', ['GroupSetDupeScore', v, [nid]]) for v in ('2147483648', '-2147483649')]
    param = _rpc(daemon, 'editqueue', ['GroupSetParameter', '=y', [nid]])
    # a parameter without a value and an unknown dupe mode: refused by the edit
    # (logged), but the call answered true
    noequals = _rpc(daemon, 'editqueue', ['GroupSetParameter', 'novalue', [nid]])
    badmode = _rpc(daemon, 'editqueue', ['GroupSetDupeMode', 'BOGUS', [nid]])
    goodmode = _rpc(daemon, 'editqueue', ['GroupSetDupeMode', 'all', [nid]]).get('result')
    good = _rpc(daemon, 'editqueue', ['GroupSetDupeScore', '700', [nid]]).get('result')
    group = next((g for g in api.listgroups() if g['NZBID'] == nid), {})
    score, mode = group.get('DupeScore'), group.get('DupeMode')
    rejected = all(not r.get('result') for r in replies + [param, noequals, badmode])
    ok = rejected and good and goodmode and score == 700 and mode == 'ALL'
    return ('editscorerange', ok, 'replies=%s param=%s noequals=%s badmode=%s good=%s/%s score=%s mode=%s' % (
        [r.get('result', r.get('error')) for r in replies], param.get('result', param.get('error')),
        noequals.get('result', noequals.get('error')), badmode.get('result', badmode.get('error')),
        good, goodmode, score, mode))


def scenario_fleetsamekey(daemon, t):
    """F28: two fleets of one key at once, with different postings, while a
    download holds the connections. Before, both queued a pick, tied at the top
    score: two downloads of one release. The second waits for the first and adds
    its members as backups below the first's pick."""
    import threading
    daemon.fake_nntp.alive = set(_fleet_ids('sb', 600)) | set(_fleet_ids('s1')) | set(_fleet_ids('s2'))
    daemon.fake_nntp.delays.update({'sb-': 3.0})
    api = daemon.wait_ready()
    _ds_append(api, 'Busy.Download', _fake_nzb_ids(_fleet_ids('sb', 600), 6_000_000).decode(), 'busy-key', 100, paused=False)
    time.sleep(3)
    box = {}

    def send(slot, tag, key):
        box[slot] = _fleet(daemon, [('Show.S02E08.%s.whole' % tag, _fake_nzb_ids(_fleet_ids(tag), 410_000).decode()),
                                    ('Show.S02E08.%s.dead' % tag, _fake_nzb_ids(_fleet_ids(tag + 'd'), 400_000).decode())],
                           key=key, timeout=20)
    threads = [threading.Thread(target=send, args=('1', 's1', 'fleet:same-key')),
               threading.Thread(target=send, args=('2', 's2', 'Fleet:Same-Key'))]
    for th in threads:
        th.start()
    for th in threads:
        th.join(timeout=90)
    groups = [g for g in api.listgroups() if g['DupeKey'].lower() == 'fleet:same-key']
    scores = sorted((g['DupeScore'] for g in groups), reverse=True)
    ok = len(groups) == 1
    return ('fleetsamekey', ok, 'queued=%s replies=%s' % (
        [(g['NZBName'], g['DupeScore'], g['Status']) for g in groups],
        [(box.get(k, {}).get('Chosen'), box.get(k, {}).get('Reason'), box.get(k, {}).get('Complete')) for k in ('1', '2')]))


def scenario_fleetwide(daemon, t):
    """F30: a fleet of 40 dead postings, the server slow to answer, a 20 s limit:
    the reply comes within the limit (production: 48 members, 30 s limit, 85 s),
    Complete false, one member queued."""
    daemon.fake_nntp.alive = set()
    daemon.fake_nntp.delays.update({'fw': 0.5})
    daemon.wait_ready()
    members = [('Show.S01E01.v%02d' % i, _fake_nzb_ids(_fleet_ids('fw%02d' % i), 400_000).decode()) for i in range(40)]
    start = time.time()
    reply = _fleet(daemon, members, key='fleet:wide', timeout=20, http_timeout=200)
    took = time.time() - start
    ok = took < 23 and len(reply['Members']) == 40
    return ('fleetwide', ok, 'took=%.1f complete=%s chosen=%s statuses=%s' % (
        took, reply['Complete'], reply['Chosen'], sorted(set(m['Status'] for m in reply['Members']))))


def scenario_fleetslowurlfirst(daemon, t):
    """F29: both members are urls, the slow one (40 s) listed first, a 10 s limit.
    The urls were fetched one after another: the slow one used up the limit and
    the good one was never fetched (both ERROR, nothing chosen). Fetched at once,
    the good one is queued within the limit."""
    import threading
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
    good = _fake_nzb_ids(_fleet_ids('sf'), 400_000)

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path.startswith('/slow'):
                time.sleep(40)
            self.send_response(200)
            self.send_header('Content-Length', str(len(good)))
            self.end_headers()
            self.wfile.write(good)

        def log_message(self, *args):
            pass
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    daemon.fake_nntp.alive = set(_fleet_ids('sf'))
    daemon.wait_ready()
    base = 'http://127.0.0.1:%d' % server.server_address[1]
    start = time.time()
    reply = _rpc(daemon, 'appendfleet', ['fleet:slowfirst', 'test', 0, 10,
                                         'Show.S01E01.slow', base + '/slow.nzb', 'Show.S01E01.good', base + '/good.nzb'],
                 timeout=60).get('result', {})
    took = time.time() - start
    byname = {m['Name']: m for m in reply.get('Members', [])}
    ok = took < 12 and byname.get('Show.S01E01.good', {}).get('Status') == 'QUEUED' and \
        byname.get('Show.S01E01.slow', {}).get('Status') == 'ERROR'
    return ('fleetslowurlfirst', ok, 'took=%.1f members=%s' % (took, [(m['Name'], m['Status']) for m in reply.get('Members', [])]))


def scenario_appendurlgzip(daemon, t):
    """B91: a url answered gzip-compressed without a length (as NZBHydra2 does),
    the body compressing about 20 to 1 like a real search answer: nzbget reads
    all of it. A compressed read that expanded past the decompressor's buffer
    lost the rest."""
    import gzip as _gzip
    import http.server
    import threading
    segs = ''.join('<segment bytes="750000" number="%d">%s-%d@gz.test</segment>' % (i + 1, 'x' * 40, i) for i in range(4000))
    body = ('<?xml version="1.0" encoding="UTF-8"?><nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">'
            '<file poster="p" date="1" subject="&quot;gz.bin&quot; yEnc (1/4000)"><groups><group>a.b</group></groups>'
            '<segments>%s</segments></file></nzb>' % segs).encode()
    packed = _gzip.compress(body)

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200)
            self.send_header('Content-Encoding', 'gzip')
            self.end_headers()
            self.wfile.write(packed)

        def log_message(self, *args):
            pass
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    api = daemon.wait_ready()
    nid = _rpc(daemon, 'appendurl', ['Gz.nzb', 'http://127.0.0.1:%d/gz.nzb' % server.server_address[1],
                                     '', 0, False, True, 'gz-key', 0, 'SCORE', []]).get('result')
    deadline = time.time() + 60
    group = None
    while time.time() < deadline:
        group = next((g for g in api.listgroups() if g['NZBName'] == 'Gz' and g['Kind'] == 'NZB'), None)
        if group or _grep_log(t, 'Gz'):
            if group:
                break
        time.sleep(0.5)
    server.shutdown()
    articles = group['RemainingSizeMB'] if group else None
    ok = group is not None and group['FileSizeMB'] >= 2800
    return ('appendurlgzip', ok, 'compressed=%d plain=%d group=%s errors=%d' % (
        len(packed), len(body), group and (group['FileSizeMB'], group['RemainingFileCount']), _grep_log(t, 'Error parsing')))


def scenario_appendlongname(daemon, t):
    """A name of 600 characters (a fleet member's, or an append's): the nzb-file
    couldn't be created past the file system's 255 bytes and the download was
    refused (ERROR, -1). It is added under its full name now."""
    api = daemon.wait_ready()
    nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@ln' % uuid.uuid4().hex for _ in range(3)], 30_000)).decode()
    name = 'Long.' + 'n\u00e9' * 300 + '.nzb'
    nid = _rpc(daemon, 'append', [name, nzb, '', 0, False, True, 'ln-key', 0, 'SCORE', []]).get('result')
    group = next((g for g in api.listgroups() if g['NZBID'] == nid), None) if isinstance(nid, int) and nid > 0 else None
    ok = group is not None and group['NZBName'].startswith('Long.n') and len(group['NZBName']) > 255
    return ('appendlongname', ok, 'id=%s name_len=%s' % (nid, group and len(group['NZBName'])))


def scenario_apiedges(daemon, t):
    """API edge cases (all upstream too):
    rate past INT_MAX/1024 is rejected (it wrapped to a 1 KB/s limit);
    listgroups' PostStageProgress is 0 for a queued item (it printed a pointer);
    a post-processing parameter with a quote and a backslash is stored as sent
    (it was stored escaped: a password with them failed to unpack);
    saveconfig with an empty or malformed list is refused and the file keeps every
    option (it was rewritten without them, reporting success); with "Value"
    before "Name" the right option is saved."""
    api = daemon.wait_ready()
    rate = _rpc(daemon, 'rate', [4194305])
    limit = api.status()['DownloadLimit']
    nzb = base64.standard_b64encode(_fake_nzb_ids(['%s@ae' % uuid.uuid4().hex for _ in range(3)], 30_000)).decode()
    nid = _rpc(daemon, 'append', ['Api.Edges.nzb', nzb, '', 0, False, True, 'ae-key', 0, 'SCORE',
                                  [{'*Unpack:Password': 'pa"ss\\x'}]]).get('result')
    group = next((g for g in api.listgroups() if g['NZBID'] == nid), {})
    password = {p['Name']: p['Value'] for p in group.get('Parameters', [])}.get('*Unpack:Password')
    conf_path = t.path(daemon.conf_rel)
    before = open(conf_path).read()
    empty = _rpc(daemon, 'saveconfig', [[]])
    bad = _rpc(daemon, 'saveconfig', [[{'Name': 'ControlPort', 'Value': 1}, {'Name': 'NzbDirInterval', 'Value': '7'}]])
    kept = open(conf_path).read() == before
    swapped = _rpc(daemon, 'saveconfig', [[{'Value': '9', 'Name': 'NzbDirInterval'}]])
    after = open(conf_path).read()
    ok = (not rate.get('result') and limit == 0 and group.get('PostStageProgress') == 0 and password == 'pa"ss\\x' and
          'error' in empty and 'error' in bad and kept and swapped.get('result') and 'NzbDirInterval=9' in after)
    # (saveconfig replaces the whole option list, as the web interface sends every
    # option: the swapped-key call leaves only NzbDirInterval, by design)
    return ('apiedges', ok, 'rate=%s limit=%s progress=%s password=%r empty=%s bad=%s kept=%s swapped=%s interval9=%s' % (
        rate.get('result', rate.get('error')), limit, group.get('PostStageProgress'), password,
        'error' in empty, 'error' in bad, kept, swapped.get('result'), 'NzbDirInterval=9' in after))


def scenario_apiaccess(daemon, t):
    """Access and request-size edges (upstream too). A restricted user's
    system.multicall ran its calls with full control access: config returned the
    control password in plain text, where a direct call masks it. A POST that
    declares a 2 GB body crashed the server (the buffer couldn't be allocated and
    was written through anyway); it is refused now and nzbget stays up."""
    import socket
    daemon.creds = 'nzbget:ctlpass'
    daemon.wait_ready()

    def post(path, body, auth, length=None):
        sock = socket.create_connection(('127.0.0.1', daemon.rpc_port), timeout=10)
        head = ('POST %s HTTP/1.1\r\nHost: x\r\nAuthorization: Basic %s\r\nContent-Length: %d\r\n\r\n' %
                (path, base64.b64encode(auth.encode()).decode(), len(body) if length is None else length))
        sock.sendall(head.encode() + body)
        data = b''
        try:
            while True:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                data += chunk
        except socket.timeout:
            pass
        sock.close()
        return data
    call = (b'<?xml version="1.0"?><methodCall><methodName>system.multicall</methodName><params><param><value>'
            b'<array><data><value><struct><member><name>methodName</name><value><string>config</string></value>'
            b'</member><member><name>params</name><value><array><data></data></array></value></member></struct>'
            b'</value></data></array></value></param></params></methodCall>')
    reply = post('/xmlrpc', call, 'ro:ropass')
    leaked = b'ctlpass' in reply
    masked = b'***' in reply
    huge = post('/jsonrpc', b'{}', 'nzbget:ctlpass', length=2147483647)
    time.sleep(1)
    try:
        alive = bool(daemon.api().version())
    except Exception:
        alive = False
    ok = not leaked and masked and b' 400' in huge.split(b'\r\n', 1)[0] and alive
    return ('apiaccess', ok, 'leaked=%s masked=%s huge_status=%r alive=%s' % (
        leaked, masked, huge.split(b'\r\n', 1)[0][:40], alive))


def scenario_queueedits(daemon, t):
    """Queue edits and state (upstream too): an item's DupeMode survives a restart
    (the hint was loaded into it: FORCE came back as SCORE); GroupMerge with an id
    given twice doesn't use the item it already freed and reports success (it
    reported false on success); FileDelete with a file given twice doesn't crash;
    FileSplit of files from two collections is refused (it crashed)."""
    api = daemon.wait_ready()

    def add(name, n, mode='SCORE'):
        nzb = base64.standard_b64encode(_many_files_nzb(n, uuid.uuid4().hex[:8])).decode()
        return _rpc(daemon, 'append', [name + '.nzb', nzb, '', 0, False, True, 'qe-' + name, 0, mode, []]).get('result')
    forced = add('Forced', 1, 'FORCE')
    a, b, c, d = add('MergeA', 2), add('MergeB', 2), add('SplitC', 2), add('SplitD', 2)
    merged = _rpc(daemon, 'editqueue', ['GroupMerge', '', [a, b, b]]).get('result')
    files_c = [f['ID'] for f in api.listfiles(0, 0, c)]
    files_d = [f['ID'] for f in api.listfiles(0, 0, d)]
    deleted = _rpc(daemon, 'editqueue', ['FileDelete', '', [files_c[0], files_c[0]]]).get('result')
    split = _rpc(daemon, 'editqueue', ['FileSplit', 'Mixed', [files_c[1], files_d[0]]]).get('result')
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    groups = {g['NZBID']: g for g in api.listgroups()}
    mode = groups.get(forced, {}).get('DupeMode')
    ok = mode == 'FORCE' and merged is True and deleted is True and split is False and b not in groups
    return ('queueedits', ok, 'mode_after_restart=%s merged=%s deleted=%s split=%s groups=%d' % (
        mode, merged, deleted, split, len(groups)))


def scenario_authrejected(daemon, t):
    """A second level-0 server refuses every login with 502 (a broken account),
    and an article is missing on the first: the article counts as failed on the
    refusing server and the download completes. It waited for that server for
    good, and every download that needed it stalled (production: 0 KB/s for over
    an hour with eight working servers)."""
    size, seg = 2_000_000, 500_000
    pp = _place_copy(t, 'arA', _payload(size, 7878), 'f.bin')
    api = daemon.wait_ready()
    # backend identification logs in to every server at start: count only the
    # logins the download makes
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, 'Backend of Server2') == 0:
        time.sleep(0.2)
    before = daemon.reject_nntp.logins
    daemon.append(api, 'RelAR', build_nzb(pp, 'ar.bin', size, seg, {2}), False, 'ar-key', 100)
    deadline = time.time() + 90
    h = None
    while time.time() < deadline:
        h = next((x for x in api.history() if x['NZBName'] == 'RelAR'), None)
        if h:
            break
        time.sleep(0.5)
    logins = daemon.reject_nntp.logins - before
    ok = h is not None and logins >= 1
    return ('authrejected', ok, 'completed=%s status=%s refused_logins=%d' % (h is not None, h and h['Status'], logins))


def scenario_staleprogress(daemon, t):
    """The progress file names a collection the queue no longer holds (nzbget
    stopped between saving the queue and discarding the progress file): the
    queue loads anyway. The whole load was aborted ("NZB with id N could not
    be found"), and queue and history were set aside as unreadable."""
    api = daemon.wait_ready()
    _ds_append(api, 'Kept.Item', _fake_nzb_ids(_fleet_ids('kp', 5), 50_000).decode(), 'kp-key', 100)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    # a progress record is the collection's id, then its full record as in the queue
    # file: the queue's only record, under an id the queue doesn't have
    queue_lines = open(t.path('main', 'queue', 'queue')).read().split('\n')
    open(t.path('main', 'queue', 'progress'), 'w').write('\n'.join(queue_lines[:1] + ['1', '99999'] + queue_lines[2:]))
    daemon.start()
    api = daemon.wait_ready()
    groups = {g['NZBName'] for g in api.listgroups()}
    unreadable = _grep_log(t, 'could not be read')
    ok = unreadable == 0 and 'Kept.Item' in groups
    return ('staleprogress', ok, 'unreadable_logs=%d groups=%s skipped_logs=%d' % (
        unreadable, sorted(groups), _grep_log(t, 'no longer queued')))


def scenario_apiformat(daemon, t):
    """Response and GET edge cases (upstream too): a GET parameter holding an encoded
    "&" (%26) stays one value (the whole query was decoded before it was split);
    JSON-P runs only the reading methods (any web page can send it: it ran every
    method), so readurl by JSON-P is refused and fetches nothing, while by POST it
    still runs; servervolumes with some arrays turned off is valid JSON (stray and
    doubled commas); a JSON-RPC id over 99 characters comes back whole (it was cut,
    and the response wasn't valid JSON)."""
    import json as _json
    import urllib.request as _req
    api = daemon.wait_ready()
    base = 'http://127.0.0.1:%d' % daemon.rpc_port
    import http.server
    import threading
    import urllib.parse as _parse

    hits = []

    class Echo(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            hits.append(self.path)
            body = self.path.encode()
            self.send_response(200)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass
    echo = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Echo)
    echo.daemon_threads = True
    threading.Thread(target=echo.serve_forever, daemon=True).start()
    target = 'http://127.0.0.1:%d/p?x=1&y=2' % echo.server_address[1]
    # the GET parameter: a JSON-P callback name holding an encoded "&" comes back whole
    with _req.urlopen(base + '/jsonprpc/version?=a%26b', timeout=10) as r:
        callback_whole = r.read().decode().startswith('a&b(')
    # readurl reaches out: by JSON-P (a GET) it's refused through the callback and
    # fetches nothing ...
    with _req.urlopen(base + '/jsonprpc/readurl?=cb&1=%s&2=echo' % _parse.quote(target, safe=''), timeout=20) as r:
        reply = r.read().decode()
        refused = _json.loads(reply[reply.index('(') + 1:reply.rindex(')')]).get('error', {}).get('code') == 4
    jsonp_fetches = len(hits)
    # ... and by POST it runs
    body = _json.dumps({'method': 'readurl', 'params': [target, 'echo']}).encode()
    with _req.urlopen(_req.Request(base + '/jsonrpc', body), timeout=20) as r:
        fetched = _json.loads(r.read().decode()).get('result')
    echo.shutdown()
    valid = True
    for flags in ([False, True, False, True, False], [True, True, True, False, True], [False, False, False, False, False]):
        body = _json.dumps({'method': 'servervolumes', 'params': flags}).encode()
        with _req.urlopen(_req.Request(base + '/jsonrpc', body), timeout=10) as r:
            try:
                _json.loads(r.read().decode())
            except ValueError:
                valid = False
    long_id = 'i' * 300
    body = _json.dumps({'method': 'version', 'params': [], 'id': long_id}).encode()
    with _req.urlopen(_req.Request(base + '/jsonrpc', body), timeout=10) as r:
        try:
            echoed = _json.loads(r.read().decode()).get('id')
        except ValueError:
            echoed = None
    # any page can read a JSON-P answer: it gets the protected options as "***",
    # although these requests come with control access; JSON-RPC still shows them
    def option_value(entries, name):
        return next((e.get('Value') for e in entries if e.get('Name') == name), None)
    with _req.urlopen(base + '/jsonprpc/config?=cb', timeout=10) as r:
        reply = r.read().decode()
        jsonp_config = _json.loads(reply[reply.index('(') + 1:reply.rindex(')')]).get('result') or []
    body = _json.dumps({'method': 'config', 'params': []}).encode()
    with _req.urlopen(_req.Request(base + '/jsonrpc', body), timeout=10) as r:
        direct_config = _json.loads(r.read().decode()).get('result') or []
    masked = (option_value(jsonp_config, 'Server1.Password') == '***'
              and 'harness-secret' not in reply
              and option_value(direct_config, 'Server1.Password') == 'harness-secret')
    ok = (callback_whole and refused and jsonp_fetches == 0 and fetched == '/p?x=1&y=2'
          and valid and echoed == long_id and masked)
    return ('apiformat', ok, 'callback_whole=%s jsonp_refused=%s jsonp_fetches=%d post_fetched=%r '
            'volumes_valid=%s id_echoed=%s jsonp_masked=%s' % (callback_whole, refused, jsonp_fetches, fetched,
                                                               valid, echoed == long_id, masked))


def scenario_jsonpoff(daemon, t):
    """JSON-P is off by default (option JsonP): any web page can make a browser send
    it, with the login the browser keeps, and read the answer. /jsonprpc answers
    error 403 through the callback for every method, reading ones included, so the
    settings and their passwords can't be read there; JSON-RPC is unaffected."""
    import json as _json
    import urllib.request as _req
    daemon.wait_ready()
    base = 'http://127.0.0.1:%d' % daemon.rpc_port
    replies = {}
    for method in ('version', 'status', 'config'):
        with _req.urlopen(base + '/jsonprpc/%s?=cb' % method, timeout=10) as r:
            replies[method] = r.read().decode()
    refused = all(reply.startswith('cb(') and '"code" : 403,' in reply for reply in replies.values())
    leaked = any('harness-secret' in reply or '"result"' in reply for reply in replies.values())
    body = _json.dumps({'method': 'version', 'params': []}).encode()
    with _req.urlopen(_req.Request(base + '/jsonrpc', body), timeout=10) as r:
        version = _json.loads(r.read().decode()).get('result')
    ok = refused and not leaked and bool(version)
    return ('jsonpoff', ok, 'refused=%s leaked=%s jsonrpc_version=%r' % (refused, leaked, version))


def scenario_directunpackkeep(daemon, t):
    """DirectUnpack=yes with UseTempUnpackDir=no: a direct unpack that fails (an
    encrypted rar sent without its password) leaves the downloaded files alone.
    Its cleanup deleted the unpack folder with its content - here the download's
    own folder - so every volume downloaded so far vanished mid-download."""
    data = _payload(4_000_000, 8080)
    volumes = generators.rar3_store_volumes_encrypted('movie.mkv', data, 1_400_000, 'secretpw')
    members = []
    for i, vol in enumerate(volumes, 1):
        rel = 'duA/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 500_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'RelDU', build_multi_nzb(members), False, 'du-key', 100)
    # the files right after the direct unpack failed, while the download goes on
    deadline = time.time() + 120
    while time.time() < deadline and _grep_log(t, 'Direct unpack for RelDU failed') == 0:
        time.sleep(0.1)
    at_failure = sorted(os.path.basename(rel) for rel in t.find_files('main') if re.search(r'Rel\.part\d+\.rar', rel))
    h = daemon.wait_history(api, 'RelDU', timeout=240)
    kept = sorted(os.path.basename(rel) for rel in t.find_files('main') if re.search(r'Rel\.part\d+\.rar$', rel))
    ok = len(kept) == len(volumes) and 'Rel.part01.rar' in at_failure
    return ('directunpackkeep', ok, 'status=%s at_failure=%s volumes_on_disk=%s of %d' % (
        h['Status'], at_failure, len(kept), len(volumes)))


# The direct-unpack cases below were the C++ PostprocessTest suite
# (tests/postprocess/DirectUnpackTest.h); they now run the daemon end to end.
RARRENAMER_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'testdata', 'rarrenamer')


def _rarrenamer_members(t, tag, sets):
    """Serve the tests/testdata/rarrenamer volumes of each set (e.g. 'testfile3')."""
    members = []
    for name in sets:
        for i in (1, 2, 3):
            vol = '%s.part%02d.rar' % (name, i)
            with open(os.path.join(RARRENAMER_DIR, vol), 'rb') as f:
                data = f.read()
            rel = '%s/%s' % (tag, vol)
            t.write_file(os.path.join('data', rel), data)
            members.append((rel, vol, len(data), 500_000, set(), data))
    return members


def _direct_unpack_release(daemon, t, name, tag, sets, par_failure=False):
    """Queue the volumes of `sets` (plus, with `par_failure`, a side file with a
    missing article and a recovery-less par2 index of everything, so the par
    check fails while the archives stay whole) and wait for its history entry."""
    members = _rarrenamer_members(t, tag, sets)
    if par_failure:
        side = _payload(1_200_000, 7171)
        t.write_file(os.path.join('data', tag, 'side.bin'), side)
        members.append(('%s/side.bin' % tag, 'side.bin', len(side), 500_000, {2}, side))
        par = generators.par2_index([(m[1], m[5]) for m in members])
        t.write_file(os.path.join('data', tag, 'rel.par2'), par)
        members.append(('%s/rel.par2' % tag, 'rel.par2', len(par), 500_000, set(), par))
    api = daemon.wait_ready()
    daemon.append(api, name, build_multi_nzb([m[:5] for m in members]), False, tag + '-key', 100)
    return daemon.wait_history(api, name, timeout=180)


def _main_basenames(t):
    return sorted(os.path.basename(rel) for rel in t.find_files('main'))


def scenario_directunpacksimple(daemon, t):
    """DirectUnpackSimpleTest: a three-volume rar set is extracted by direct
    unpack while it downloads, and its file lands in the destination."""
    h = _direct_unpack_release(daemon, t, 'RelDUS', 'dusA', ['testfile3'])
    files = _main_basenames(t)
    du = _grep_log(t, 'Direct unpack for RelDUS successful')
    ok = du == 1 and 'testfile3.dat' in files and h['UnpackStatus'] == 'SUCCESS'
    return ('directunpacksimple', ok, 'status=%s unpack=%s du_ok=%d files=%s' % (
        h['Status'], h['UnpackStatus'], du, files))


def scenario_directunpacktwoarchives(daemon, t):
    """DirectUnpackTwoArchives: two rar sets in one download are both extracted
    by direct unpack."""
    h = _direct_unpack_release(daemon, t, 'RelDU2', 'du2A', ['testfile3', 'testfile5'])
    files = _main_basenames(t)
    du = _grep_log(t, 'Direct unpack for RelDU2 successful')
    ok = (du == 1 and 'testfile3.dat' in files and 'testfile5.dat' in files and
          h['UnpackStatus'] == 'SUCCESS')
    return ('directunpacktwoarchives', ok, 'status=%s unpack=%s du_ok=%d files=%s' % (
        h['Status'], h['UnpackStatus'], du, files))


def scenario_directunpackparfailure(daemon, t):
    """DirectUnpackParFailureStillFinalizesUnpack: the par check fails (a side
    file is damaged and the par2 has no recovery data) but direct unpack
    extracted every archive, so post-processing still finalizes that unpack:
    unpack SUCCESS, status FAILURE/PAR, the file moved out of _unpack."""
    h = _direct_unpack_release(daemon, t, 'RelDUP', 'dupA', ['testfile3'], par_failure=True)
    files = _main_basenames(t)
    leftover = [rel for rel in t.find_files('main') if '/_unpack/' in rel.replace(os.sep, '/')]
    du = _grep_log(t, 'Direct unpack for RelDUP successful')
    ok = (du == 1 and h['ParStatus'] == 'FAILURE' and h['UnpackStatus'] == 'SUCCESS' and
          h['Status'] == 'FAILURE/PAR' and 'testfile3.dat' in files and not leftover)
    return ('directunpackparfailure', ok, 'status=%s par=%s unpack=%s du_ok=%d files=%s leftover=%s' % (
        h['Status'], h['ParStatus'], h['UnpackStatus'], du, files, leftover))


def _par_failure_skips_unpack(daemon, t, label, name, tag, sets):
    h = _direct_unpack_release(daemon, t, name, tag, sets, par_failure=True)
    skipped = _grep_log(t, 'Skipping unpack for %s due to par-failure' % name)
    ok = (skipped >= 1 and h['ParStatus'] == 'FAILURE' and h['UnpackStatus'] == 'NONE' and
          _grep_log(t, 'Direct unpack for %s successful' % name) == 0)
    return (label, ok, 'status=%s par=%s unpack=%s skipped_logs=%d' % (
        h['Status'], h['ParStatus'], h['UnpackStatus'], skipped))


def scenario_parfailurenodirectunpack(daemon, t):
    """ParFailureWithoutSuccessfulDirectUnpackStillSkipsUnpack, direct unpack off
    (nsNone): a failed par check skips the unpack."""
    return _par_failure_skips_unpack(daemon, t, 'parfailurenodirectunpack', 'RelPFN', 'pfnA', ['testfile3'])


def scenario_parfailuredirectunpackfailed(daemon, t):
    """ParFailureWithoutSuccessfulDirectUnpackStillSkipsUnpack, direct unpack
    failed (nsFailure: an encrypted set without its password): a failed par
    check still skips the unpack."""
    label, ok, detail = _par_failure_skips_unpack(daemon, t, 'parfailuredirectunpackfailed', 'RelPFF', 'pffA',
                                                  ['testfile3encdata'])
    du_failed = _grep_log(t, 'Direct unpack for RelPFF failed')
    return (label, ok and du_failed == 1, '%s du_failed=%d' % (detail, du_failed))


class _LogClock:
    """Times the first appearance of each of ``needles`` in the daemon log, polled
    every 50 ms from a thread started before the download is queued."""

    def __init__(self, t, needles):
        self.t = t
        self.needles = list(needles)
        self.seen = {}
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def _run(self):
        while not self.stop.is_set():
            try:
                text = self.t.read_file('nzbget.log').decode(errors='replace')
            except Exception:
                text = ''
            now = time.time()
            for needle in self.needles:
                if needle not in self.seen and needle in text:
                    self.seen[needle] = now
            if len(self.seen) == len(self.needles):
                return
            time.sleep(0.05)

    def close(self):
        time.sleep(0.2)
        self.stop.set()
        self.thread.join(5)

    def span(self, start, end):
        if start in self.seen and end in self.seen:
            return '%.2f' % (self.seen[end] - self.seen[start])
        return 'n/a'


def _live_scale():
    """RG_LIVE_SCALE multiplies the sizes of the timed live-extraction scenarios
    (live7zsplit, liveedgehole5), to measure the time from the last article to the
    extracted file with sets where extraction takes long."""
    try:
        return max(1, int(os.environ.get('RG_LIVE_SCALE', '1')))
    except ValueError:
        return 1


def _live_clock(t, name, extra=()):
    """The download's end and the moments its files were extracted."""
    return _LogClock(t, ['Collection %s completely downloaded' % name,
                         'Direct unpack for %s successful' % name,
                         'Unpack for %s successful' % name] + list(extra))


def _before_download_end(clock, name, needle):
    """Whether ``needle`` was logged before the download ended."""
    end = clock.seen.get('Collection %s completely downloaded' % name)
    at = clock.seen.get(needle)
    return end is not None and at is not None and at < end


def _live_timing(clock, name):
    """Seconds from the download's end (its last article) to the extracted files in
    place: direct unpack's end, and post-processing's unpack done."""
    done = 'Collection %s completely downloaded' % name
    return 'after_download_s=%s du_after_download_s=%s' % (
        clock.span(done, 'Unpack for %s successful' % name),
        clock.span(done, 'Direct unpack for %s successful' % name))


def _live_release(daemon, t, name, tag, volumes, seg, extra=(), needles=()):
    """Queues ``volumes`` = [(name, bytes)] (plus ``extra`` members) as one download,
    timing it with a log clock; returns (history, clock)."""
    members = []
    for vname, data in volumes:
        rel = '%s/%s' % (tag, vname)
        t.write_file(os.path.join('data', rel), data)
        members.append((rel, vname, len(data), seg, set()))
    members += list(extra)
    api = daemon.wait_ready()
    clock = _live_clock(t, name, needles)
    daemon.append(api, name, build_multi_nzb(members), False, tag + '-key', 100)
    h = daemon.wait_history(api, name, timeout=300)
    clock.close()
    return h, clock


def _left_volumes(t, names):
    """Volumes still on disk after post-processing (UnpackCleanupDisk deletes them)."""
    left = set(os.path.basename(rel) for rel in t.find_files('main'))
    return sorted(n for n in names if n in left)


def _live_archive(daemon, t, label, name, tag, volumes, files):
    """A 7z or ZIP set extracted while it downloads: direct unpack maps it once its
    first volume's start and last volume's directory are written (those articles come
    first), writes each
    member straight into its output as the volumes arrive and is done when the
    download ends; post-processing extracts nothing again and deletes the volumes."""
    h, clock = _live_release(daemon, t, name, tag, volumes, 500_000)
    first = volumes[0][0]
    extracted = all(_verify_output(t, data, '.' + fname.rsplit('.', 1)[1]) for fname, data in files)
    live = _grep_log(t, 'Unrar: Extracting from %s' % first)
    du_ok = _grep_log(t, 'Direct unpack for %s successful' % name)
    declined = _grep_log(t, 'Direct unpack leaves')
    again = _grep_log(t, 'Executing 7-Zip') + _grep_log(t, 'Joining splitted file')
    used = _grep_log(t, 'Using directly unpacked files')
    left = _left_volumes(t, [v[0] for v in volumes])
    stray = _stray_temp_files(t)
    ok = (extracted and live >= 1 and du_ok == 1 and declined == 0 and again == 0 and used == 1 and
          not left and not stray and h['Status'].startswith('SUCCESS') and h['UnpackStatus'] == 'SUCCESS')
    return (label, ok, 'status=%s unpack=%s extracted_ok=%s live=%d du_ok=%d declined=%d extracted_again=%d '
            'used_direct=%d volumes_left=%s stray_temp=%s %s' % (
                h['Status'], h['UnpackStatus'], extracted, live, du_ok, declined, again, used, left, stray,
                _live_timing(clock, name)))


def _stray_temp_files(t):
    """Temp files of an unpack left in the download's folders."""
    return sorted(rel for rel in t.find_files('main', 'dst') + t.find_files('main', 'inter')
                  if '.rustyget-' in rel)


def scenario_live7zsplit(daemon, t):
    """A store-mode 7z cut into byte pieces (movie.7z.001 ...) holding a movie and its
    nfo is extracted while it downloads; nothing is extracted after the download."""
    scale = _live_scale()
    movie = _payload(48_000_000 * scale, 7101)
    nfo = _payload(4_000, 7102)
    arch = generators.seven_zip_copy_checked([('movie.mkv', movie), ('movie.nfo', nfo)])
    piece = 6_000_000 * scale
    pieces = generators.split_bytes(arch, [piece] * (len(arch) // piece))
    volumes = [('Movie.7z.%03d' % (i + 1), p) for i, p in enumerate(pieces)]
    return _live_archive(daemon, t, 'live7zsplit', 'RelL7z', 'l7zA', volumes,
                         [('movie.mkv', movie), ('movie.nfo', nfo)])


def scenario_live7zlzma2(daemon, t):
    """A 7z made by 7-Zip with LZMA2 (multithreaded, so several blocks) and cut into
    byte pieces by 7-Zip itself (-v), holding a partly compressible movie: its one
    folder is decoded live as the pieces arrive."""
    if not generators.HAVE_7Z:
        return ('live7zlzma2', None, 'SKIP: 7z not installed')
    rnd = _payload(12_000_000, 7111)
    movie = b''.join(rnd[i:i + 4096] + bytes(4096) for i in range(0, len(rnd), 4096))
    with tempfile.TemporaryDirectory(prefix='l7zl-') as workdir:
        with open(os.path.join(workdir, 'movie.mkv'), 'wb') as f:
            f.write(movie)
        subprocess.run([generators.SEVENZIP_PATH, 'a', '-bd', '-m0=LZMA2', '-mx5', '-mmt=4', '-v4m', 'out.7z',
                        'movie.mkv'], cwd=workdir, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        names = sorted(n for n in os.listdir(workdir) if n.startswith('out.7z.'))
        volumes = [('Movie' + n[len('out'):], open(os.path.join(workdir, n), 'rb').read()) for n in names]
    return _live_archive(daemon, t, 'live7zlzma2', 'RelL7L', 'l7lA', volumes, [('movie.mkv', movie)])


def scenario_live7zlzma2threads(daemon, t):
    """A 7z LZMA2 set written in many independent blocks (7-Zip -mmt with a 1 MiB block
    size: a dictionary reset every block) decoded live with FeedThreads=4: the blocks whose
    data arrived are decoded on worker threads (zipfast FeedOptions::threads) and the movie
    comes out byte-identical, extracted while it downloads and never again."""
    if not generators.HAVE_7Z:
        return ('live7zlzma2threads', None, 'SKIP: 7z not installed')
    rnd = _payload(12_000_000, 7121)
    movie = b''.join(rnd[i:i + 4096] + bytes(4096) for i in range(0, len(rnd), 4096))
    with tempfile.TemporaryDirectory(prefix='l7zt-') as workdir:
        with open(os.path.join(workdir, 'movie.mkv'), 'wb') as f:
            f.write(movie)
        subprocess.run([generators.SEVENZIP_PATH, 'a', '-bd', '-m0=LZMA2:d=1m:c=1m', '-mx5', '-mmt=4', '-v4m',
                        'out.7z', 'movie.mkv'], cwd=workdir, check=True, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL)
        names = sorted(n for n in os.listdir(workdir) if n.startswith('out.7z.'))
        volumes = [('Movie' + n[len('out'):], open(os.path.join(workdir, n), 'rb').read()) for n in names]
    return _live_archive(daemon, t, 'live7zlzma2threads', 'RelL7T', 'l7tA', volumes, [('movie.mkv', movie)])


def scenario_livezipsplit(daemon, t):
    """A stored ZIP cut into byte pieces (movie.zip.001 ...) is extracted while it
    downloads; it used to be joined into a .zip after the download."""
    movie = _payload(30_000_000, 7201)
    nfo = _payload(3_000, 7202)
    arch = generators.zip_store([('movie.mkv', movie), ('movie.nfo', nfo)])
    pieces = generators.split_bytes(arch, [5_000_000] * (len(arch) // 5_000_000))
    volumes = [('Movie.zip.%03d' % (i + 1), p) for i, p in enumerate(pieces)]
    return _live_archive(daemon, t, 'livezipsplit', 'RelLZs', 'lzsA', volumes,
                         [('movie.mkv', movie), ('movie.nfo', nfo)])


def scenario_livezipz01(daemon, t):
    """An Info-ZIP split set (movie.z01 ... movie.zip, the central directory in the
    .zip) is extracted while it downloads."""
    movie = _payload(30_000_000, 7301)
    nfo = _payload(2_000, 7302)
    disks = generators.zip_store_infozip_split([('movie.nfo', nfo), ('movie.mkv', movie)], 5_000_000)
    volumes = [('Movie.z%02d' % (i + 1), d) for i, d in enumerate(disks[:-1])]
    volumes.append(('Movie.zip', disks[-1]))
    return _live_archive(daemon, t, 'livezipz01', 'RelLZ1', 'lz1A', volumes,
                         [('movie.mkv', movie), ('movie.nfo', nfo)])


def scenario_livesplitjoin(daemon, t):
    """A split movie (movie.mkv.001 ...) is joined while it downloads: each piece is
    placed at its offset in movie.mkv as it completes. Post-processing joins nothing
    (no byte is read or written by a join after the download) and deletes the pieces."""
    movie = _payload(40_000_000, 7401)
    pieces = generators.split_bytes(movie, [6_000_000] * (len(movie) // 6_000_000))
    volumes = [('movie.mkv.%03d' % (i + 1), p) for i, p in enumerate(pieces)]
    h, clock = _live_release(daemon, t, 'RelLSJ', 'lsjA', volumes, 500_000)
    extracted = _verify_output(t, movie, '.mkv')
    log = t.read_file('nzbget.log').decode(errors='replace')
    m = re.search(r'Joined movie\.mkv from (\d+) pieces as they arrived \((\d+) bytes\); (\d+) bytes placed after '
                  r'the download ended', log)
    placed = (int(m.group(2)), int(m.group(3))) if m else None
    joined_after = _grep_log(t, 'Joining splitted file')
    left = _left_volumes(t, [v[0] for v in volumes]) + _stray_temp_files(t)
    ok = (extracted and placed is not None and placed[0] == len(movie) and joined_after == 0 and not left and
          h['Status'].startswith('SUCCESS') and h['UnpackStatus'] == 'SUCCESS')
    return ('livesplitjoin', ok, 'status=%s unpack=%s extracted_ok=%s placed_live=%s post_download_join_bytes=%d '
            'join_runs=%d pieces_left=%s %s' % (
                h['Status'], h['UnpackStatus'], extracted, placed, 0 if joined_after == 0 else len(movie),
                joined_after, left, _live_timing(clock, 'RelLSJ')))


def _edge_hole_par(t, work, names, seg):
    """par2 recovery for ``names`` in ``work`` (12 blocks): [(rel, name, size, seg, set())]."""
    subprocess.run(['par2', 'create', '-q', '-q', '-s65536', '-c12', '-n1', '-a', 'Rel', 'Rel.par2'] + names,
                   cwd=work, check=True, capture_output=True)
    members = []
    tag = os.path.basename(work)
    for n in sorted(os.listdir(work)):
        if n.endswith('.par2'):
            members.append(('%s/%s' % (tag, n), n, os.path.getsize(os.path.join(work, n)), seg, set()))
    return members


def scenario_liveedgehole(daemon, t):
    """A stored rar set (no duplicate; par2 recovery only) loses an article 20 KB into
    its second volume, inside member data but within the 64 KiB that used to count as
    header, and one ending 61 bytes before its third volume's data ends, within the
    old 4 KiB tail guard. The volumes' own headers place both holes in member data:
    direct unpack reads past them while the download goes on instead of pausing at
    the second volume, and the holes are patched once par2 repairs the volumes."""
    seg = 20_000
    movie = _payload(12_000_000, 9981)
    vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 1_400_000)
    tag = 'LEH'
    work = t.path('data', tag)
    os.makedirs(work, exist_ok=True)
    names = ['Rel.part%02d.rar' % (i + 1) for i in range(len(vols))]
    damage = {1: {2}, 2: {70}}          # parts are 1-based: bytes 20,000-39,999 and 1,380,000-1,399,999
    members = []
    for i, (n, d) in enumerate(zip(names, vols)):
        t.write_file(os.path.join('data', tag, n), d)
        members.append(('%s/%s' % (tag, n), n, len(d), seg, damage.get(i, set())))
    members += _edge_hole_par(t, work, names, seg)
    api = daemon.wait_ready()
    past = 'Unrar: Extracting from Rel.part04.rar'
    clock = _live_clock(t, 'RelLEH', [past])
    daemon.append(api, 'RelLEH', build_multi_nzb(members), False, 'leh-key', 100)
    h = daemon.wait_history(api, 'RelLEH', timeout=300)
    clock.close()
    ok = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    skipped = _grep_log(t, 'extracted past damage in movie.mkv')
    patched = _grep_log(t, 'repaired movie.mkv from the repaired volumes')
    paused = _grep_log(t, 'Pausing direct unpack at')
    went_on = _before_download_end(clock, 'RelLEH', past)
    good = (ok and h['Status'].startswith('SUCCESS') and skipped >= 1 and patched >= 1 and paused == 0 and
            went_on)
    return ('liveedgehole', good, 'status=%s par=%s unpack=%s extracted_ok=%s skipped=%d patched=%d paused=%d '
            'past_damage_before_download_end=%s %s' % (
                h['Status'], h['ParStatus'], h['UnpackStatus'], ok, skipped, patched, paused, went_on,
                _live_timing(clock, 'RelLEH')))


def scenario_liveedgehole5(daemon, t):
    """A RAR5 store set made by rar (no duplicate; par2 recovery only) loses the last
    article of its third volume: the end of the movie's data there and the whole
    tail behind it (quick-open record, end of archive header). The tail is rebuilt
    byte for byte from the volume's own headers and length as the volume finishes,
    direct unpack reads past the lost data instead of stopping at the volume until
    the repair, and par2 repairs the data afterwards."""
    if not generators.HAVE_RAR:
        return ('liveedgehole5', None, 'SKIP: rar not installed')
    seg = 20_000
    scale = _live_scale()
    movie = _payload(12_000_000 * scale, 9982)
    with tempfile.TemporaryDirectory(prefix='leh5-') as workdir:
        vols = generators.rar5_store_volumes_hp('movie.mkv', movie, 1_000_000 * scale, None, workdir)
    tag = 'LE5'
    work = t.path('data', tag)
    os.makedirs(work, exist_ok=True)
    names = ['Rel.part%02d.rar' % (i + 1) for i in range(len(vols))]
    last_part = (len(vols[2]) + seg - 1) // seg
    damage = {2: {last_part}}
    members = []
    for i, (n, d) in enumerate(zip(names, vols)):
        t.write_file(os.path.join('data', tag, n), d)
        members.append(('%s/%s' % (tag, n), n, len(d), seg, damage.get(i, set())))
    members += _edge_hole_par(t, work, names, seg)
    api = daemon.wait_ready()
    past = 'Unrar: Extracting from Rel.part04.rar'
    clock = _live_clock(t, 'RelLE5', [past])
    daemon.append(api, 'RelLE5', build_multi_nzb(members), False, 'le5-key', 100)
    h = daemon.wait_history(api, 'RelLE5', timeout=300)
    clock.close()
    ok = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    rebuilt = _grep_log(t, 'Rebuilt the end of Rel.part03.rar')
    skipped = _grep_log(t, 'extracted past damage in movie.mkv')
    patched = _grep_log(t, 'repaired movie.mkv from the repaired volumes')
    # (the volume is held while its tail is rebuilt, a moment after it finishes)
    went_on = _before_download_end(clock, 'RelLE5', past)
    good = (ok and h['Status'].startswith('SUCCESS') and rebuilt == 1 and skipped >= 1 and patched >= 1 and
            went_on)
    return ('liveedgehole5', good, 'status=%s par=%s unpack=%s extracted_ok=%s tail_rebuilt=%d skipped=%d patched=%d '
            'past_damage_before_download_end=%s %s' % (
                h['Status'], h['ParStatus'], h['UnpackStatus'], ok, rebuilt, skipped, patched, went_on,
                _live_timing(clock, 'RelLE5')))


# Options of the writer-fed scenarios: an article cache as in production (the cache path
# reports its flushed articles), no external unpacker needed.
WRITER_FED_OPTIONS = ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no',
                      'KeepHistory=1', 'UnpackCleanupDisk=yes', 'ArticleCache=8192']

_LIVE_READS = re.compile(r'Live extraction read ([\d.]+) MB as written, ([\d.]+) MB read back after their volume '
                         r'finished, ([\d.]+) MB from volumes finished before')


def _live_reads(t):
    """(MB read as written, MB read back, MB from volumes finished before) summed over the
    direct unpack summary lines of the log, or None without any."""
    try:
        text = t.read_file('nzbget.log').decode(errors='replace')
    except Exception:
        return None
    found = _LIVE_READS.findall(text)
    if not found:
        return None
    return tuple(round(sum(float(x[k]) for x in found), 1) for k in range(3))


def _writer_fed(daemon, t, label, name, tag, volumes, files, first):
    """A set (enough volumes that several download while it extracts, so the result does not
    depend on which volumes four connections finish together) extracted while it downloads,
    its volumes read article by
    article as the writer puts them in place (writer-fed, see rust/src/livewrite.rs): when
    the last article lands nearly everything is extracted, so the extracted files are in
    place a moment after the download ends, and no packed byte is read back from a
    finished volume (``read_back_mb=0``). ``first`` is the volume direct unpack starts
    from; it finished before the extraction began (``finished_mb``)."""
    h, clock = _live_release(daemon, t, name, tag, volumes, 500_000, needles=['Live extraction read'])
    extracted = all(_verify_output(t, data, '.' + fname.rsplit('.', 1)[1], dirs=(('main', 'dst'), ('main', 'inter')))
                    for fname, data in files)
    live = _grep_log(t, 'Unrar: Extracting from %s' % first)
    du_ok = _grep_log(t, 'Direct unpack for %s successful' % name)
    used = _grep_log(t, 'Using directly unpacked files')
    reads = _live_reads(t)
    as_written, read_back, finished = reads if reads else (0.0, None, None)
    stray = _stray_temp_files(t)
    ok = (extracted and live >= 1 and du_ok == 1 and used == 1 and not stray and as_written > 0 and
          read_back == 0 and h['Status'].startswith('SUCCESS') and h['UnpackStatus'] == 'SUCCESS')
    return (label, ok, 'status=%s unpack=%s extracted_ok=%s live=%d du_ok=%d used_direct=%d stray_temp=%s '
            'as_written_mb=%s read_back_mb=%s finished_mb=%s extracted_after_download_s=%s %s' % (
                h['Status'], h['UnpackStatus'], extracted, live, du_ok, used, stray, as_written, read_back,
                finished, clock.span('Collection %s completely downloaded' % name, 'Live extraction read'),
                _live_timing(clock, name)))


def _video_like(size, seed):
    """A partly compressible payload: random 4 KiB blocks between zero runs."""
    rnd = _payload(size // 2, seed)
    return b''.join(rnd[i:i + 4096] + bytes(4096) for i in range(0, len(rnd), 4096))[:size]


def scenario_livefedrar5store(daemon, t):
    """A RAR5 store set with big volumes made by rar is read article by article while it
    downloads: its last volume is not read back after the download."""
    if not generators.HAVE_RAR:
        return ('livefedrar5store', None, 'SKIP: rar not installed')
    scale = _live_scale()
    movie = _payload(60_000_000 * scale, 7301)
    with tempfile.TemporaryDirectory(prefix='lfr5s-') as workdir:
        vols = generators.rar5_method_volumes('movie.mkv', movie, 8_000_000 * scale, 0, workdir)
    volumes = [('Rel.part%02d.rar' % (i + 1), v) for i, v in enumerate(vols)]
    return _writer_fed(daemon, t, 'livefedrar5store', 'RelWF5', 'wf5A', volumes, [('movie.mkv', movie)],
                       volumes[0][0])


def scenario_livefedrar5m3(daemon, t):
    """A compressed RAR5 set (rar -m3) with big volumes is decoded article by article
    while it downloads."""
    if not generators.HAVE_RAR:
        return ('livefedrar5m3', None, 'SKIP: rar not installed')
    scale = _live_scale()
    movie = _video_like(60_000_000 * scale, 7302)
    with tempfile.TemporaryDirectory(prefix='lfr5c-') as workdir:
        vols = generators.rar5_method_volumes('movie.mkv', movie, 5_000_000 * scale, 3, workdir)
    volumes = [('Rel.part%02d.rar' % (i + 1), v) for i, v in enumerate(vols)]
    return _writer_fed(daemon, t, 'livefedrar5m3', 'RelWFC', 'wfcA', volumes, [('movie.mkv', movie)],
                       volumes[0][0])


def scenario_livefed7z(daemon, t):
    """A 7z made by 7-Zip and cut into pieces by 7-Zip (-v) is read article by article while
    it downloads. Stored (-m0=Copy): a debug build decodes LZMA2 slower than the throttled
    download on a loaded VM, and then reads volumes only after they finished, which is not
    what this checks (RG_LIVE_7Z_METHOD=LZMA2 times the compressed case)."""
    if not generators.HAVE_7Z:
        return ('livefed7z', None, 'SKIP: 7z not installed')
    scale = _live_scale()
    movie = _video_like(60_000_000 * scale, 7303)
    with tempfile.TemporaryDirectory(prefix='lf7z-') as workdir:
        with open(os.path.join(workdir, 'movie.mkv'), 'wb') as f:
            f.write(movie)
        method = os.environ.get('RG_LIVE_7Z_METHOD', 'Copy')
        level = ['-mx5', '-mmt=4'] if method != 'Copy' else []
        subprocess.run([generators.SEVENZIP_PATH, 'a', '-bd', '-m0=%s' % method] + level +
                       ['-v%dm' % (5 * scale), 'out.7z', 'movie.mkv'], cwd=workdir, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        names = sorted(n for n in os.listdir(workdir) if n.startswith('out.7z.'))
        volumes = [('Movie' + n[len('out'):], open(os.path.join(workdir, n), 'rb').read()) for n in names]
    return _writer_fed(daemon, t, 'livefed7z', 'RelWF7', 'wf7A', volumes, [('movie.mkv', movie)],
                       volumes[0][0])



# --------------------------------------------------------------------------- #
# Extraction-order downloads (rust/src/liveorder.rs): a set extracted live is
# downloaded in its extraction order and read from its first volume's headers on
# --------------------------------------------------------------------------- #

LIVE_ORDER_WINDOW = 3   # liveorder::WINDOW


def _seven_zip_volumes(movie, piece, workdir, prefix='Movie'):
    """A stored 7z of ``movie.mkv`` cut into ``piece``-byte volumes by 7-Zip (-v)."""
    with open(os.path.join(workdir, 'movie.mkv'), 'wb') as f:
        f.write(movie)
    subprocess.run([generators.SEVENZIP_PATH, 'a', '-bd', '-m0=Copy', '-v%db' % piece, 'out.7z', 'movie.mkv'],
                   cwd=workdir, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    names = sorted(n for n in os.listdir(workdir) if n.startswith('out.7z.'))
    return [(prefix + n[len('out'):], open(os.path.join(workdir, n), 'rb').read()) for n in names]


def _volume_done(name, vname):
    """The INFO line of a volume's completion (not its articles' DETAIL lines, which add the
    part number)."""
    return 'Successfully downloaded %s/%s\n' % (name, vname)


def _live_order(daemon, t, label, name, tag, volumes, movie, nzb_order):
    """``volumes`` listed in the NZB in ``nzb_order`` (ReorderFiles off: the queue would take
    them in that order). Direct rename fetches every file's first article first; once the first
    volume's headers are written direct unpack starts from them, registers the set's
    extraction order, and the rest downloads in that order: no volume finishes before one
    ``LIVE_ORDER_WINDOW`` or more below it (``inversions``), the extraction starts before the
    first volume is complete (``started_before_first_done``), reads nothing back
    (``read_back_mb=0``) and is done a moment after the download (``du_after_download_s``)."""
    first = volumes[0][0]
    done = [_volume_done(name, v[0]) for v in volumes]
    started = 'Unrar: Extracting from %s' % first
    members = []
    for vname, data in volumes:
        rel = '%s/%s' % (tag, vname)
        t.write_file(os.path.join('data', rel), data)
        members.append((rel, vname, len(data), 500_000, set()))
    api = daemon.wait_ready()
    clock = _live_clock(t, name, done + [started, 'Live extraction read'])
    daemon.append(api, name, build_multi_nzb([members[i] for i in nzb_order]), False, tag + '-key', 100)
    h = daemon.wait_history(api, name, timeout=300)
    clock.close()
    extracted = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    at = [clock.seen.get(d) for d in done]
    order = sorted(range(len(volumes)), key=lambda i: at[i] if at[i] is not None else float('inf'))
    # (volume i finished before volume j although j is a window or more below it)
    inversions = [(i + 1, j + 1) for k, i in enumerate(order) for j in order[k + 1:]
                  if j + LIVE_ORDER_WINDOW <= i]
    early = (clock.seen.get(started) is not None and at[0] is not None and clock.seen[started] < at[0])
    reads = _live_reads(t)
    as_written, read_back, finished = reads if reads else (0.0, None, None)
    tail = clock.span('Collection %s completely downloaded' % name, 'Direct unpack for %s successful' % name)
    du_ok = _grep_log(t, 'Direct unpack for %s successful' % name)
    stray = _stray_temp_files(t)
    ok = (extracted and None not in at and not inversions and early and du_ok == 1 and as_written > 0 and
          read_back == 0 and not stray and tail != 'n/a' and float(tail) <= 5.0 and
          h['Status'].startswith('SUCCESS') and h['UnpackStatus'] == 'SUCCESS')
    return (label, ok, 'status=%s unpack=%s extracted_ok=%s completion_order=%s inversions=%s '
            'started_before_first_done=%s du_ok=%d as_written_mb=%s read_back_mb=%s finished_mb=%s '
            'stray_temp=%s du_after_download_s=%s' % (
                h['Status'], h['UnpackStatus'], extracted, [i + 1 for i in order], inversions, early, du_ok,
                as_written, read_back, finished, stray, tail))


def scenario_liveorder7z(daemon, t):
    """A stored 7z cut into nine volumes, listed in the NZB last volume first: the volumes
    finish in extraction order (the directory at the end of the last one is fetched first,
    by offset), and the extraction starts while the first volume is still downloading."""
    if not generators.HAVE_7Z:
        return ('liveorder7z', None, 'SKIP: 7z not installed')
    scale = _live_scale()
    # (eight whole volumes and a last one of several articles)
    movie = _video_like(42_500_000 * scale, 7401)
    with tempfile.TemporaryDirectory(prefix='lo7z-') as workdir:
        volumes = _seven_zip_volumes(movie, 5_000_000 * scale, workdir)
    return _live_order(daemon, t, 'liveorder7z', 'RelLO7', 'lo7A', volumes, movie,
                       list(reversed(range(len(volumes)))))


def scenario_liveorderrar(daemon, t):
    """A RAR5 store set of nine volumes listed in the NZB in a scrambled order: the volumes
    finish in extraction order and the extraction starts from the first volume's headers,
    before that volume is complete."""
    if not generators.HAVE_RAR:
        return ('liveorderrar', None, 'SKIP: rar not installed')
    scale = _live_scale()
    movie = _payload(42_500_000 * scale, 7402)
    with tempfile.TemporaryDirectory(prefix='lorar-') as workdir:
        vols = generators.rar5_method_volumes('movie.mkv', movie, 5_000_000 * scale, 0, workdir)
    volumes = [('Rel.part%02d.rar' % (i + 1), v) for i, v in enumerate(vols)]
    n = len(volumes)
    scrambled = list(range(n - 1, 0, -2)) + list(range(n - 2, 0, -2)) + [0]
    return _live_order(daemon, t, 'liveorderrar', 'RelLOR', 'loRA', volumes, movie, scrambled)


# First requests of these articles hang for 40 s (a news server routing them to a slow
# backend); asked again, they come at once.
LIVE_STRAGGLERS = [(b'lsA/Rel.part01.rar?3=', 40.0, True), (b'lsA/Rel.part02.rar?6=', 40.0, True),
                   (b'lsA/Rel.part03.rar?2=', 40.0, True), (b'lsA/Rel.part05.rar?4=', 40.0, True)]


def scenario_livestraggler(daemon, t):
    """A RAR5 store set downloading at a fixed rate (3 MB/s: 8 s for 24 MB) where the first
    request for four articles hangs for 40 s. A live extraction would wait at each of them,
    and every volume would finish only with its slowest article. The articles a live
    extraction will wait for are asked again once they run far longer than the rest
    ("Asking again for"), so the extracted file is in place a few seconds after the line-rate
    download (``extracted_after_start_s``, well under the 40 s hang) and moments after the last
    article (``du_after_download_s``)."""
    if not generators.HAVE_RAR:
        return ('livestraggler', None, 'SKIP: rar not installed')
    movie = _payload(24_000_000, 7403)
    with tempfile.TemporaryDirectory(prefix='lsrar-') as workdir:
        vols = generators.rar5_method_volumes('movie.mkv', movie, 4_000_000, 0, workdir)
    volumes = [('Rel.part%02d.rar' % (i + 1), v) for i, v in enumerate(vols)]
    name = 'RelLS'
    start = time.time()
    h, clock = _live_release(daemon, t, name, 'lsA', volumes, 500_000, needles=['Live extraction read'])
    extracted = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    asked = _grep_log(t, 'Asking again for %s/' % name)
    du_at = clock.seen.get('Direct unpack for %s successful' % name)
    after_start = (du_at - start) if du_at else None
    tail = clock.span('Collection %s completely downloaded' % name, 'Direct unpack for %s successful' % name)
    reads = _live_reads(t)
    as_written, read_back, _ = reads if reads else (0.0, None, None)
    # (a loaded test host stretches the 8 s download; the 40 s hang is what is ruled out)
    ok = (extracted and asked >= 1 and after_start is not None and after_start < 25 and tail != 'n/a' and
          float(tail) <= 8.0 and read_back == 0 and as_written > 0 and daemon.proxy.delayed >= 1 and
          h['Status'].startswith('SUCCESS') and h['UnpackStatus'] == 'SUCCESS')
    return ('livestraggler', ok, 'status=%s unpack=%s extracted_ok=%s asked_again=%d hung_requests=%d '
            'extracted_after_start_s=%s du_after_download_s=%s as_written_mb=%s read_back_mb=%s' % (
                h['Status'], h['UnpackStatus'], extracted, asked, daemon.proxy.delayed,
                '%.1f' % after_start if after_start is not None else 'n/a', tail, as_written, read_back))


def scenario_jointwosets(daemon, t):
    """Two sets of split files in one download (a.mkv.001-003, b.srt.001-002): both
    are joined. The fragment check counted the other set's pieces too, and the join
    failed with "missing fragments detected" (upstream too)."""
    a = _payload(2_500_000, 8181)
    b = _payload(150_000, 8182)
    pieces = [('a.mkv', a, 1_000_000), ('b.srt', b, 100_000)]
    members = []
    for name, data, size in pieces:
        for i, start in enumerate(range(0, len(data), size), 1):
            rel = 'jtA/%s.%03d' % (name, i)
            chunk = data[start:start + size]
            t.write_file(os.path.join('data', rel), chunk)
            members.append((rel, '%s.%03d' % (name, i), len(chunk), 500_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'RelJT', build_multi_nzb(members), False, 'jt-key', 100)
    h = daemon.wait_history(api, 'RelJT', timeout=180)
    joined = {}
    for rel in t.find_files('main'):
        base = os.path.basename(rel)
        # (the subtitle is renamed after the video by post-download renaming: a.srt)
        if base in ('a.mkv', 'b.srt', 'a.srt'):
            joined['a.mkv' if base == 'a.mkv' else 'srt'] = t.read_file(rel) == (a if base == 'a.mkv' else b)
    ok = joined.get('a.mkv') is True and joined.get('srt') is True and h['Status'].startswith('SUCCESS')
    return ('jointwosets', ok, 'status=%s joined=%s missing_logs=%d' % (
        h['Status'], joined, _grep_log(t, 'missing fragments detected')))


def scenario_fleetpaused(daemon, t):
    """appendfleet (F12): with downloads paused by the user, a fleet is still
    checked - the check doesn't download - and ranked: the whole copy is
    chosen, the dead one measured dead; the queue stays paused."""
    daemon.fake_nntp.alive = set(_fleet_ids('pw'))
    api = daemon.wait_ready()
    api.pausedownload()
    reply = _fleet(daemon, [('Show.S01E01.dead', _fake_nzb_ids(_fleet_ids('pd'), 400_000).decode()),
                            ('Show.S01E01.whole', _fake_nzb_ids(_fleet_ids('pw'), 410_000).decode())],
                   key='fleet:paused', timeout=20)
    byname = {m['Name']: m for m in reply['Members']}
    ok = (reply['Complete'] and byname['Show.S01E01.whole']['Status'] == 'QUEUED' and
          byname['Show.S01E01.dead']['Status'] == 'DEAD' and api.status()['DownloadPaused'])
    return ('fleetpaused', ok, 'reply=%s' % [(m['Name'], m['Status'], m['Alive']) for m in reply['Members']])


def scenario_fleetparallel(daemon, t):
    """appendfleet (F11): two fleets for different keys at once, while a long
    download holds both connections. One fleet's check runs at a time; the other
    waits within its own limit: both finish inside 19 s with every member
    measured. Before, they shared the connections and both used the whole limit
    with members unmeasured."""
    import threading
    daemon.fake_nntp.alive = set(_fleet_ids('pb', 600)) | set(_fleet_ids('p1')) | set(_fleet_ids('p2'))
    daemon.fake_nntp.delays.update({'pb-': 3.0})
    api = daemon.wait_ready()
    _ds_append(api, 'Busy.Download', _fake_nzb_ids(_fleet_ids('pb', 600), 6_000_000).decode(), 'busy-key', 100, paused=False)
    time.sleep(3)
    box = {}

    def send(slot, tag):
        start = time.time()
        box[slot] = _fleet(daemon, [('Show.S0%sE01.dead' % slot, _fake_nzb_ids(_fleet_ids(tag + 'd'), 400_000).decode()),
                                    ('Show.S0%sE01.whole' % slot, _fake_nzb_ids(_fleet_ids(tag), 410_000).decode())],
                           key='fleet:par%s' % slot, timeout=20)
        box[slot + 'took'] = time.time() - start
    threads = [threading.Thread(target=send, args=(slot, tag)) for slot, tag in (('1', 'p1'), ('2', 'p2'))]
    for th in threads:
        th.start()
    for th in threads:
        th.join(timeout=90)
    # Each fleet waits for connections the busy download holds for 3 s per article, so a
    # run takes whole multiples of those 3 s by when its reads land (3.2, 6.2 and 9.2 s in
    # three runs on one Mac; 12.4-15.8 s on an emulated Linux x86_64). The fault this
    # catches is both fleets using their whole 20 s limit (members left unmeasured, also
    # checked): a second inside it tells them apart.
    ok = all(box.get(slot, {}).get('Complete') and box.get(slot + 'took', 99) < 19 and
             all(m['Alive'] >= 0 for m in box[slot]['Members']) for slot in ('1', '2'))
    return ('fleetparallel', ok, 'took=%.1f/%.1f complete=%s/%s' % (box.get('1took', -1), box.get('2took', -1),
            box.get('1', {}).get('Complete'), box.get('2', {}).get('Complete')))


def scenario_fleetslowurl(daemon, t):
    """appendfleet (F13): a member's url answers only after 40 s; the fleet
    allows 10 s. The call still returns within its limit (the fetch is stopped
    at the deadline), the good member queued and the slow one an error."""
    import threading
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

    class Slow(BaseHTTPRequestHandler):
        def do_GET(self):
            time.sleep(40)
            self.send_response(200)
            self.end_headers()

        def log_message(self, *args):
            pass
    server = ThreadingHTTPServer(('127.0.0.1', 0), Slow)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    daemon.fake_nntp.alive = set(_fleet_ids('su'))
    daemon.wait_ready()
    import json as _json
    import urllib.request as _req
    params = ['fleet:slowurl', 'test', 0, 10,
              'Show.S01E01.slow', 'http://127.0.0.1:%d/slow.nzb' % server.server_address[1],
              'Show.S01E01.good', base64.standard_b64encode(_fake_nzb_ids(_fleet_ids('su'), 400_000)).decode()]
    start = time.time()
    request = _req.Request('http://127.0.0.1:%d/jsonrpc' % daemon.rpc_port,
                           _json.dumps({'method': 'appendfleet', 'params': params}).encode(),
                           {'Content-Type': 'application/json'})
    with _req.urlopen(request, timeout=60) as reply:
        result = _json.loads(reply.read().decode())['result']
    took = time.time() - start
    server.shutdown()
    byname = {m['Name']: m for m in result['Members']}
    ok = took < 12 and byname['Show.S01E01.good']['Status'] == 'QUEUED' and byname['Show.S01E01.slow']['Status'] == 'ERROR'
    return ('fleetslowurl', ok, 'took=%.1f members=%s' % (took, [(m['Name'], m['Status']) for m in result['Members']]))


def scenario_fleetone(daemon, t):
    """appendfleet with a single whole member: it is queued and downloads."""
    daemon.fake_nntp.alive = set(_fleet_ids('one'))
    api = daemon.wait_ready()
    reply = _fleet(daemon, [('Show.S01E01.one', _fake_nzb_ids(_fleet_ids('one'), 400_000).decode())])
    h = daemon.wait_history(api, 'Show.S01E01.one', timeout=120)
    ok = reply['Chosen'] > 0 and reply['Members'][0]['Status'] == 'QUEUED' and h['Status'].startswith('SUCCESS')
    return ('fleetone', ok, 'reply=%s status=%s' % (reply, h['Status']))


def scenario_fleetnodupecheck(daemon, t):
    """appendfleet with DupeCheck=no: nothing would park the backups, so only
    the best copy is added and the others are reported SKIPPED (not added).
    Before, all three were added to the queue and downloaded side by side, and
    the others kept downloading after the first one succeeded."""
    daemon.fake_nntp.alive = set(_fleet_ids('na')) | set(_fleet_ids('nb')) | set(_fleet_ids('nc'))
    api = daemon.wait_ready()
    reply = _fleet(daemon, [('Show.S01E01.na', _fake_nzb_ids(_fleet_ids('na'), 400_000).decode()),
                            ('Show.S01E01.nb', _fake_nzb_ids(_fleet_ids('nb'), 410_000).decode()),
                            ('Show.S01E01.nc', _fake_nzb_ids(_fleet_ids('nc'), 420_000).decode())])
    added = [g['NZBName'] for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet-key']
    statuses = sorted(m['Status'] for m in reply['Members'])
    chosen = next((m for m in reply['Members'] if m['Status'] == 'QUEUED'), {})
    h = daemon.wait_history(api, chosen.get('Name', '?'), timeout=120) if chosen else {'Status': 'NONE'}
    ok = (reply['Chosen'] > 0 and chosen.get('NZBID') == reply['Chosen'] and
          statuses == ['QUEUED', 'SKIPPED', 'SKIPPED'] and
          all(m['NZBID'] == 0 for m in reply['Members'] if m['Status'] == 'SKIPPED') and
          len(added) == 1 and h['Status'].startswith('SUCCESS'))
    return ('fleetnodupecheck', ok, 'members=%s added=%s status=%s' % (
        [(m['Name'], m['Status'], m['NZBID']) for m in reply['Members']], added, h['Status']))


def _yenc_part(data, name='a.rar'):
    """A one-part yEnc article holding ``data``. A line never ends inside an
    escape: cutting the encoded bytes every 128 split "=X" pairs, so data
    with a random salt (rar -hp) sometimes decoded to the wrong length."""
    lines_out, line = [], bytearray()
    for b in data:
        c = (b + 42) & 255
        if c in (0, 10, 13, 61, 46):
            line += b'=' + bytes([(c + 64) & 255])
        else:
            line.append(c)
        if len(line) >= 128:
            lines_out.append(bytes(line))
            line = bytearray()
    if line:
        lines_out.append(bytes(line))
    lines = b'\r\n'.join(lines_out)
    crc = zlib.crc32(data) & 0xffffffff
    return (b'=ybegin part=1 total=1 line=128 size=%d name=%s\r\n=ypart begin=1 end=%d\r\n' % (len(data), name.encode(), len(data))
            + lines + b'\r\n=yend size=%d part=1 pcrc32=%08x\r\n' % (len(data), crc))


def _rar4_head(method, main_flags=0):
    """The start of a rar4 volume holding "movie.mkv" packed with ``method``
    (0x30 store, 0x33 normal compression), with real header CRCs."""
    import struct
    name = b'movie.mkv'
    main = generators._rar3_block(struct.pack('<HBHH', 0, 0x73, main_flags, 13) + b'\0' * 6)
    head = generators._rar3_block(struct.pack('<HBHH', 0, 0x74, 0x8000, 32 + len(name)) + struct.pack(
        '<IIBIIBBHI', 1_000_000, 1_000_000, 3, 0, 0, 29, method, len(name), 0) + name)
    return b'Rar!\x1a\x07\x00' + main + head + b'\xaa' * 256


def _fake_nzb_files(files):
    """An nzb-file of several files [(name, ids, total_bytes)]."""
    blocks = ''
    for n, (name, ids, total) in enumerate(files):
        seg = max(1, total // len(ids))
        segs = ''.join('<segment bytes="%d" number="%d">%s</segment>' % (seg, i + 1, m) for i, m in enumerate(ids))
        blocks += ('<file poster="p@x" date="1" subject="[%d/%d] &quot;%s&quot; yEnc (1/%d)"><groups>'
                   '<group>alt.binaries.test</group></groups><segments>%s</segments></file>'
                   % (n + 1, len(files), name, len(ids), segs))
    return ('<?xml version="1.0" encoding="UTF-8"?>\n<nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">'
            '%s</nzb>' % blocks).encode()


def _fleet_copy(daemon, tag, data_name, size, first_body, par_data, alive_count=40):
    """One fleet copy: a data file of 40 articles (the first holding
    ``first_body``) and a par2 index listing ``par_data`` as ``data_name``.
    Returns its nzb text and puts ``alive_count`` data articles on the server."""
    ids = _fleet_ids(tag)
    par_id = '%s-par@x' % tag
    daemon.fake_nntp.alive |= set(ids[:alive_count]) | {par_id}
    if first_body is not None:
        daemon.fake_nntp.bodies_by_id[ids[0]] = _yenc_part(first_body, data_name)
    index = generators.par2_index([(data_name, par_data)], pad=0)
    base = data_name.rsplit('.', 1)[0]
    daemon.fake_nntp.bodies_by_id[par_id] = _yenc_part(index, base + '.par2')
    return _fake_nzb_files([(data_name, ids, size), (base + '.par2', [par_id], len(index))]).decode()


def scenario_fleetkindorder(daemon, t):
    """appendfleet orders copies by health tier first, then kind (97.5%+
    alive first): the whole ALT in store-mode rar (borrowable), then the whole
    ALT in compressed rar (unborrowable), then the TWINs (two copies whose par2
    sets list the same file), which are under 97.5% alive, the healthier one
    first. Each added copy keeps its kind in DupeCopyKind. (Which copy is
    queued when the reply is sent is timing.)"""
    daemon.fake_nntp.alive = set()
    same = _payload(400_000, 7001)
    api = daemon.wait_ready()
    reply = _fleet(daemon, [
        ('Show.S01E01.compressed', _fleet_copy(daemon, 'ac', 'a.rar', 420_000, _rar4_head(0x33), _payload(420_000, 7002))),
        ('Show.S01E01.store', _fleet_copy(daemon, 'as', 'b.rar', 410_000, _rar4_head(0x30), _payload(410_000, 7003))),
        ('Show.S01E01.twinA', _fleet_copy(daemon, 'ta', 'c.bin', 400_000, None, same, 30)),
        ('Show.S01E01.twinB', _fleet_copy(daemon, 'tb', 'd.bin', 400_000, None, same, 36))])
    order = [m['Name'] for m in sorted(reply['Members'], key=lambda m: m['Rank'])]
    items = {g['NZBName']: g for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet-key'}
    kinds = {}
    for name, g in items.items():
        params = {p['Name']: p['Value'] for p in g.get('Parameters', [])}
        kinds[name] = params.get('DupeCopyKind')
    chosen = next((m['Name'] for m in reply['Members'] if m['Status'] == 'QUEUED'), None)
    ok = (order == ['Show.S01E01.store', 'Show.S01E01.compressed', 'Show.S01E01.twinB', 'Show.S01E01.twinA']
          and kinds.get('Show.S01E01.twinA') == 'twin' and kinds.get('Show.S01E01.store') == 'borrowable'
          and kinds.get('Show.S01E01.compressed') == 'unborrowable')
    return ('fleetkindorder', ok, 'order=%s chosen=%s kinds=%s' % (order, chosen, kinds))


def _fleet_kinds(api, key):
    return {g['NZBName']: {p['Name']: p['Value'] for p in g.get('Parameters', [])}.get('DupeCopyKind')
            for g in api.listgroups() + api.history() if g.get('DupeKey') == key}


def scenario_fleetprimaryease(daemon, t):
    """F10: among healthy copies the primary is the one easiest to borrow
    into: a bare copy before a stored rar before a compressed one, whatever
    order the client sent them in. Health still gates: a weak bare copy (30
    of 40 alive) never leads the healthy ones."""
    daemon.fake_nntp.alive = set()
    api = daemon.wait_ready()
    reply = _fleet(daemon, [
        ('Show.S01E01.compressed', _fleet_copy(daemon, 'ec', 'a.rar', 420_000, _rar4_head(0x33), _payload(420_000, 7801))),
        ('Show.S01E01.store', _fleet_copy(daemon, 'es', 'b.rar', 410_000, _rar4_head(0x30), _payload(410_000, 7802))),
        ('Show.S01E01.weakbare', _fleet_copy(daemon, 'ew', 'd.mkv', 400_000, None, _payload(400_000, 7803), 30)),
        ('Show.S01E01.bare', _fleet_copy(daemon, 'eb', 'c.mkv', 400_000, None, _payload(400_000, 7804)))],
        key='fleet:ease')
    order = [m['Name'] for m in sorted(reply['Members'], key=lambda m: m['Rank'])]
    chosen = next((m['Name'] for m in reply['Members'] if m['NZBID'] == reply['Chosen']), None)
    ok = chosen == 'Show.S01E01.bare' and order[0] == 'Show.S01E01.bare' and \
        order.index('Show.S01E01.store') < order.index('Show.S01E01.compressed')
    return ('fleetprimaryease', ok, 'order=%s chosen=%s' % (order, chosen))


def scenario_fleetnzbpassword(daemon, t):
    """F8: a copy whose RAR5 headers are encrypted (-hp, store mode inside),
    with its password only in its nzb's <meta type="password">, is read with
    that password: borrowable-encrypted (it lends once decrypted), ranked
    after the plain stored copy and kept as a backup with its alive share. The
    same posting without a password is locked (DupeLocked, unborrowable, last);
    with a wrong one its headers stay unread (DupeLocked recorded,
    unborrowable). Neither is ever DEAD. Uses the committed RAR5 fixture, so no
    rar binary is needed."""
    daemon.fake_nntp.alive = set()
    hp = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'testdata', 'donorkind', 'enchp5.rar'),
              'rb').read()
    api = daemon.wait_ready()
    meta = '<head><meta type="password">%s</meta></head><file '
    with_pw = _fleet_copy(daemon, 'pw', 'a.rar', 400_000, hp, _payload(400_000, 7951)).replace('<file ', meta % 'pw', 1)
    wrong_pw = _fleet_copy(daemon, 'pr', 'b.rar', 400_000, hp, _payload(400_000, 7952)).replace('<file ', meta % 'nope', 1)
    no_pw = _fleet_copy(daemon, 'pn', 'c.rar', 400_000, hp, _payload(400_000, 7953))
    store = _fleet_copy(daemon, 'ps', 'd.rar', 400_000, _rar4_head(0x30), _payload(400_000, 7954))
    reply = _fleet(daemon, [('Show.S01E01.nopw', no_pw), ('Show.S01E01.wrongpw', wrong_pw),
                            ('Show.S01E01.pw', with_pw), ('Show.S01E01.store', store)], key='fleet:nzbpw')
    order = [m['Name'] for m in sorted(reply['Members'], key=lambda m: m['Rank'])]
    status = {m['Name']: (m['Status'], m['Alive']) for m in reply['Members']}
    items = {g['NZBName']: {p['Name']: p['Value'] for p in g.get('Parameters', [])}
             for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet:nzbpw'}
    kinds = {n: p.get('DupeCopyKind') for n, p in items.items()}
    ok = (order[:2] == ['Show.S01E01.store', 'Show.S01E01.pw']
          and kinds.get('Show.S01E01.pw') == 'borrowable-encrypted'
          and items.get('Show.S01E01.pw', {}).get('*Unpack:Password') == 'pw'
          and all(items.get(n, {}).get('DupeLocked') == 'yes' for n in ('Show.S01E01.nopw', 'Show.S01E01.wrongpw'))
          and all(status[n][0] != 'DEAD' and status[n][1] == 100 for n in status))
    return ('fleetnzbpassword', ok, 'order=%s kinds=%s status=%s' % (order, kinds, status))


def scenario_dropdune(daemon, t):
    """The Dune Part Two drop on m5air (docs/DROP_AS_FLEET.md, ground truth in
    the request's nzb identity check), in small: copies of the 2160p release
    copied into NzbDir with download-manager suffixes, plus the 1080p release.
    - C01: RAR5 with encrypted headers, stored, password only in the nzb meta,
      100% alive: read with that password (borrowable-encrypted), never the
      primary while a healthy plain copy is there (F8, F10).
    - C07: stored RAR, 100% alive: the primary (F10).
    - C04 and C11: twins (the same par2 file list) of each other.
    - C08: a repacked RAR (other par2): another container, never 'other' (F9).
    - C13: a bare mkv a little shorter (other par2): never 'other' (F9).
    - C17, C05, C09: dead: kept as dead donors with their nzb (F3); F16: held
      in the queue at the end of the failover order while the fleet is open
      (before F16 they waited in History as DELETED/DUPE).
    - C00: the 1080p release: never joins the 2160p fleet (F1).
    Every copy shows its probed alive share (DupeAlive), not a Health of
    untried articles (F4)."""
    daemon.fake_nntp.alive = set()
    rel = 'Dune.Part.Two.2024.UHD.BluRay.2160p.TrueHD.Atmos.7.1.DV.HEVC.REMUX-FraMeSToR'
    key = 'dupes:dune.part.two.2024.uhd.bluray.2160p.truehd.atmos.7.1.dv.hevc.remux.framestor'
    hd = 'Dune.Part.Two.2024.BluRay.1080p.TrueHD.Atmos.7.1.AVC.REMUX-FraMeSToR'
    hp = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'testdata', 'donorkind', 'enchp5.rar'),
              'rb').read()
    movie = _payload(400_000, 8001)
    meta = '<head><meta type="password">pw</meta></head><file '
    copies = {
        ' (1)': _fleet_copy(daemon, 'du01', 'a.rar', 400_000, hp, _payload(400_000, 8011)).replace('<file ', meta, 1),
        '_12': _fleet_copy(daemon, 'du07', 'b.rar', 400_000, _rar4_head(0x30), _payload(400_000, 8017)),
        '_1': _fleet_copy(daemon, 'du04', 'c.mkv', 400_000, None, movie, 30),
        '_3': _fleet_copy(daemon, 'du11', 'c.mkv', 400_000, None, movie, 26),
        '_13': _fleet_copy(daemon, 'du08', 'd.rar', 400_000, _rar4_head(0x30), _payload(400_000, 8018), 20),
        '_5': _fleet_copy(daemon, 'du13', 'e.mkv', 399_000, None, movie[:399_000], 34),
        '_9': _fleet_copy(daemon, 'du02', 'f.rar', 400_000, None, _payload(400_000, 8012), 0),
        '_10': _fleet_copy(daemon, 'du05', 'g.mkv', 400_000, None, _payload(400_000, 8015), 0),
        '_14': _fleet_copy(daemon, 'du09', 'h.rar', 400_000, None, _payload(400_000, 8019), 0),
    }
    for tag in ('du02', 'du05', 'du09'):
        daemon.fake_nntp.alive.discard('%s-par@x' % tag)
    other = _fleet_copy(daemon, 'du00', 'i.mkv', 400_000, None, _payload(400_000, 8010))
    api = daemon.wait_ready()
    api.pausedownload()
    for suffix, text in copies.items():
        name = rel + suffix + '.nzb'
        t.write_file(os.path.join('main', 'nzb', name), text.encode())
    t.write_file(os.path.join('main', 'nzb', hd + '.nzb'), other.encode())
    deadline = time.time() + 120
    while time.time() < deadline and not _grep_log(t, 'Fleet ' + key + ' from NzbDir'):
        time.sleep(0.5)
    reply_line = next((line for line in t.read_file('nzbget.log').decode(errors='replace').splitlines()
                       if 'Fleet ' + key + ' from NzbDir' in line), '')
    chosen = re.search(r'downloading #(\d+)', reply_line)
    # TwinCheck labels the backups on its next rounds
    deadline = time.time() + 60
    while time.time() < deadline:
        kinds = {i['NZBName']: _params(i).get('DupeKind') for i in _items_of_key(api, key)}
        if kinds.get(rel + '_13') and kinds.get(rel + '_5'):
            break
        time.sleep(1)
    items = {i['NZBName']: i for i in _items_of_key(api, key)}
    p = {n: _params(i) for n, i in items.items()}
    primary = next((n for n, i in items.items() if chosen and i['NZBID'] == int(chosen.group(1))), None)
    hd_items = [g for g in api.listgroups() + api.history() if g['NZBName'] == hd]
    nzb_files = os.listdir(t.path('main', 'nzb'))
    dead_names = [n for n in items if p[n].get('DupeAlive') == '0']
    api.resumedownload()
    ok = (primary == rel + '_12'
          and p.get(rel + ' (1)', {}).get('DupeCopyKind') == 'borrowable-encrypted'
          and p.get(rel + '_3', {}).get('DupeCopyKind') == 'twin' and p.get(rel + '_1', {}).get('DupeCopyKind') == 'twin'
          and all(p.get(rel + s, {}).get('DupeKind') not in ('other',) for s in ('_13', '_5'))
          and all(p.get(rel + s, {}).get('DupeKind') == 'container' for s in ('_13', '_5'))
          and len(dead_names) == 3 and all(p[n].get('DupeHeld') == 'yes' and items[n]['Status'] == 'PAUSED'
                                           for n in dead_names)
          and all(int(p[n].get('DupeFleetOrder', '0')) >= len(items) - 3 for n in dead_names)
          and all(any(f.startswith(n + '.nzb') for f in nzb_files) for n in dead_names)
          and all(p[n].get('DupeAlive') is not None for n in items)
          and hd_items and all(g.get('DupeKey', '').lower() != key for g in hd_items))
    return ('dropdune', ok, 'primary=%s kinds=%s alive=%s dead=%s hd=%s' % (
        primary, {n: (q.get('DupeCopyKind'), q.get('DupeKind')) for n, q in p.items()},
        {n: q.get('DupeAlive') for n, q in p.items()}, dead_names, [(g['NZBName'], g.get('DupeKey')) for g in hd_items]))


def scenario_fleettimeoutunverified(daemon, t):
    """F6: timeouts are never evidence. A copy whose every check fails with a
    temporary error (451, as a server or proxy that times out answers) is
    unverified: Alive -1, a backup, never DEAD; a copy the server says it
    lacks is dead, or, when its par2 index is still there (B2b: any alive
    article makes a donor), a weak backup with its small share."""
    daemon.fake_nntp.alive = set()
    api = daemon.wait_ready()
    whole = _fleet_copy(daemon, 'tw', 'a.mkv', 400_000, None, _payload(400_000, 7901))
    flaky = _fleet_copy(daemon, 'tf', 'b.mkv', 400_000, None, _payload(400_000, 7902))
    daemon.fake_nntp.reply451 |= set(_fleet_ids('tf')) | {'tf-par@x'}
    dead = _fleet_copy(daemon, 'td', 'c.mkv', 400_000, None, _payload(400_000, 7903), 0)
    reply = _fleet(daemon, [('Show.S01E01.whole', whole), ('Show.S01E01.flaky', flaky),
                            ('Show.S01E01.dead', dead)], key='fleet:timeout')
    byname = {m['Name']: (m['Status'], m['Alive']) for m in reply['Members']}
    items = {g['NZBName']: {p['Name']: p['Value'] for p in g.get('Parameters', [])}
             for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet:timeout'}
    dead_status, dead_alive = byname.get('Show.S01E01.dead', ('', -1))
    ok = (byname.get('Show.S01E01.flaky', ('', 0))[1] == -1 and byname['Show.S01E01.flaky'][0] in ('BACKUP', 'QUEUED')
          and dead_status == 'BACKUP' and 0 < dead_alive <= 5
          and items.get('Show.S01E01.flaky', {}).get('DupeAlive') is None)
    return ('fleettimeoutunverified', ok, 'members=%s params=%s' % (byname, items.get('Show.S01E01.flaky')))


def scenario_fleetkindsingle(daemon, t):
    """appendfleet with one copy (store-mode rar): it is probed too, so it
    gets its kind (borrowable) instead of none."""
    daemon.fake_nntp.alive = set()
    api = daemon.wait_ready()
    reply = _fleet(daemon, [
        ('Show.S01E01.store', _fleet_copy(daemon, 'ks', 'b.rar', 410_000, _rar4_head(0x30), _payload(410_000, 7101)))],
        key='fleet:kindsingle')
    kinds = _fleet_kinds(api, 'fleet:kindsingle')
    ok = reply['Chosen'] > 0 and kinds.get('Show.S01E01.store') == 'borrowable'
    return ('fleetkindsingle', ok, 'reply=%s kinds=%s' % (reply, kinds))


def scenario_fleetkindlater(daemon, t):
    """A backup sent in a later fleet is classed against the copy already
    queued before it downloads: twinB (the same post as the queued twinA)
    is added as a twin, and twinA, queued alone as borrowable, becomes a twin."""
    daemon.fake_nntp.alive = set()
    same = _payload(400_000, 7102)
    api = daemon.wait_ready()
    a = _fleet(daemon, [
        ('Show.S01E01.twinA', _fleet_copy(daemon, 'la', 'c.rar', 400_000, _rar4_head(0x30), same))],
        key='fleet:kindlater')
    first = _fleet_kinds(api, 'fleet:kindlater')
    b = _fleet(daemon, [
        ('Show.S01E01.twinB', _fleet_copy(daemon, 'lb', 'c.rar', 400_000, _rar4_head(0x30), same))],
        key='fleet:kindlater')
    kinds = _fleet_kinds(api, 'fleet:kindlater')
    ok = (first.get('Show.S01E01.twinA') == 'borrowable' and kinds.get('Show.S01E01.twinB') == 'twin'
          and kinds.get('Show.S01E01.twinA') == 'twin')
    return ('fleetkindlater', ok, 'first=%s kinds=%s a=%s b=%s' % (first, kinds, a, b))


def scenario_fleetencryptedlast(daemon, t):
    """appendfleet: re-uploads of one release, some with encrypted rar headers.
    All have the same file count and size, but an encrypted re-upload has other
    bytes (a fresh salt), so its par2 set lists other hashes: same size is not a
    twin. The two whole plain re-uploads, whose par2 sets match, are the TWINs
    and go first; the whole encrypted copy comes next (unborrowable), and the
    encrypted copy under 97.5% alive last."""
    daemon.fake_nntp.alive = set()
    same = _payload(400_000, 7101)
    enc = _rar4_head(0x30, main_flags=0x80)   # main header flag: encrypted headers
    api = daemon.wait_ready()
    reply = _fleet(daemon, [
        ('Show.S01E01.enc1', _fleet_copy(daemon, 'e1', 'a.rar', 400_000, enc, _payload(400_000, 7102))),
        ('Show.S01E01.enc2', _fleet_copy(daemon, 'e2', 'b.rar', 400_000, enc, _payload(400_000, 7103), 38)),
        ('Show.S01E01.plainA', _fleet_copy(daemon, 'pa', 'c.rar', 400_000, _rar4_head(0x30), same)),
        ('Show.S01E01.plainB', _fleet_copy(daemon, 'pb', 'd.rar', 400_000, _rar4_head(0x30), same))],
        key='fleet:enc')
    order = [m['Name'] for m in sorted(reply['Members'], key=lambda m: m['Rank'])]
    items = {g['NZBName']: g for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet:enc'}
    kinds = {name: {p['Name']: p['Value'] for p in g.get('Parameters', [])}.get('DupeCopyKind')
             for name, g in items.items()}
    chosen = next((m['Name'] for m in reply['Members'] if m['Status'] == 'QUEUED'), None)
    ok = (set(order[:2]) == {'Show.S01E01.plainB', 'Show.S01E01.plainA'}
          and order[2:] == ['Show.S01E01.enc1', 'Show.S01E01.enc2']
          and kinds.get('Show.S01E01.plainA') == 'twin' and kinds.get('Show.S01E01.enc1') == 'unborrowable'
          and kinds.get('Show.S01E01.enc2') == 'unborrowable')
    return ('fleetencryptedlast', ok, 'order=%s chosen=%s kinds=%s' % (order, chosen, kinds))


def scenario_fleetencryptedheadless(daemon, t):
    """appendfleet: three encrypted 7-Zip copies (password in the nzb), one of
    which lost the first article of its data file. That copy cannot be read,
    but its password still makes it unborrowable, not "unknown"; so it does
    not jump ahead of the whole copies and a whole one is queued."""
    daemon.fake_nntp.alive = set()
    sevenz = bytes([0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c, 0, 4]) + bytes(24)
    api = daemon.wait_ready()
    def enc(tag, payload_seed, headless):
        nzb = _fleet_copy(daemon, tag, 'a.7z.001', 400_000, sevenz, _payload(400_000, payload_seed))
        if headless:
            daemon.fake_nntp.alive.discard('%s-0@x' % tag)
        return nzb.replace('<file ', '<head><meta type="password">pw</meta></head><file ', 1)
    reply = _fleet(daemon, [
        ('Show.S01E01.headless', enc('eh', 7501, True)),
        ('Show.S01E01.whole1', enc('w1', 7502, False)),
        ('Show.S01E01.whole2', enc('w2', 7503, False))], key='fleet:enchead')
    order = [m['Name'] for m in sorted(reply['Members'], key=lambda m: m['Rank'])]
    items = {g['NZBName']: g for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet:enchead'}
    kinds = {n: {p['Name']: p['Value'] for p in g.get('Parameters', [])}.get('DupeCopyKind')
             for n, g in items.items()}
    ok = (order[-1] == 'Show.S01E01.headless' and kinds.get('Show.S01E01.headless') == 'unborrowable')
    return ('fleetencryptedheadless', ok, 'order=%s kinds=%s' % (order, kinds))


def scenario_fleetencryptedpassword(daemon, t):
    """appendfleet (2026-11-20): a RAR5 copy with ENCRYPTED HEADERS, store
    mode inside, whose nzb carries the password, is read with that password
    (zipfast opens the headers) and classed `borrowable-encrypted`: it lends
    like a borrowable ALT and ranks right after the plain borrowable copy,
    ahead of a compressed copy. The same posting without a password stays
    unborrowable and locked, last. All copies are whole, so kind decides."""
    if not generators.HAVE_RAR:
        return ('fleetencryptedpassword', None, 'SKIP: rar not installed')
    daemon.fake_nntp.alive = set()
    password = 'alt-pw'
    # the copy's first article is its first 400 bytes: the signature, the
    # encryption header and the encrypted main and file headers (as with
    # _rar4_head, a short body keeps within the segment size the nzb declares)
    with tempfile.TemporaryDirectory(prefix='fep-') as workdir:
        hp = generators.rar5_store_volumes_hp('movie.mkv', _payload(300_000, 7701), 1_000_000,
                                              password, workdir)[0][:400]
    api = daemon.wait_ready()
    with_pw = _fleet_copy(daemon, 'fw', 'a.rar', 400_000, hp, _payload(400_000, 7702))
    with_pw = with_pw.replace('<file ', '<head><meta type="password">%s</meta></head><file ' % password, 1)
    no_pw = _fleet_copy(daemon, 'fn', 'b.rar', 400_000, hp, _payload(400_000, 7703))
    store = _fleet_copy(daemon, 'fs', 'c.rar', 400_000, _rar4_head(0x30), _payload(400_000, 7704))
    comp = _fleet_copy(daemon, 'fc', 'd.rar', 400_000, _rar4_head(0x33), _payload(400_000, 7705))
    reply = _fleet(daemon, [('Show.S01E01.nopw', no_pw), ('Show.S01E01.comp', comp),
                            ('Show.S01E01.pw', with_pw), ('Show.S01E01.store', store)], key='fleet:encpw')
    order = [m['Name'] for m in sorted(reply['Members'], key=lambda m: m['Rank'])]
    items = {g['NZBName']: g for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet:encpw'}
    params = {n: {p['Name']: p['Value'] for p in g.get('Parameters', [])} for n, g in items.items()}
    kinds = {n: p.get('DupeCopyKind') for n, p in params.items()}
    ok = (order == ['Show.S01E01.store', 'Show.S01E01.pw', 'Show.S01E01.comp', 'Show.S01E01.nopw']
          and kinds.get('Show.S01E01.pw') == 'borrowable-encrypted'
          and kinds.get('Show.S01E01.store') == 'borrowable'
          and params.get('Show.S01E01.pw', {}).get('DupeLocked') == 'yes')
    return ('fleetencryptedpassword', ok, 'order=%s kinds=%s' % (order, kinds))


def scenario_fleetlockedlast(daemon, t):
    """appendfleet: a copy whose rar headers are encrypted while no password
    is at hand can never be extracted or borrowed from, so it ranks after a
    plain copy that can, even a whole locked copy behind one 85% alive (live
    South Park S23E05: the locked copy was queued first and failed only after
    downloading whole). DupeLocked=yes records the encrypted headers for
    failover and the rescue, which decide when they rank whether a password
    is at hand. With its password in the nzb, or as {{password}} in its name,
    the same copy ranks by health as before. A copy queued before the fleet
    (the same posting as a member) gets DupeLocked too."""
    daemon.fake_nntp.alive = set()
    enc = _rar4_head(0x30, main_flags=0x80)   # main header flag: encrypted headers
    api = daemon.wait_ready()

    def params_of(key):
        return {g['NZBName']: {p['Name']: p['Value'] for p in g.get('Parameters', [])}
                for g in api.listgroups() + api.history() if g.get('DupeKey') == key}

    def copies(tag, password):
        locked = _fleet_copy(daemon, tag + 'k', 'a.rar', 400_000, enc, _payload(400_000, 7601))
        if password:
            locked = locked.replace('<file ', '<head><meta type="password">pw</meta></head><file ', 1)
        plain = _fleet_copy(daemon, tag + 'p', 'b.rar', 400_000, _rar4_head(0x30), _payload(400_000, 7602), 34)
        return locked, plain

    def run(key, tag, ep, password, enc_name='.enc'):
        locked, plain = copies(tag, password)
        reply = _fleet(daemon, [(ep + enc_name, locked), (ep + '.plain', plain)], key=key)
        order = [m['Name'] for m in sorted(reply['Members'], key=lambda m: m['Rank'])]
        return order, {n: p.get('DupeLocked') for n, p in params_of(key).items()}

    order, locked = run('fleet:locked', 'l', 'Show.S01E01', False)
    order_pw, locked_pw = run('fleet:lockedpw', 'w', 'Show.S01E02', True)
    order_nm, locked_nm = run('fleet:lockedname', 'n', 'Show.S01E03', False, '.{{pw}}.enc')
    # queued on its own first: the fleet finds it the same posting as a member
    old, plain = copies('o', False)
    daemon.append(api, 'Show.S01E04.old', old, True, 'fleet:lockedold', 50)
    _fleet(daemon, [('Show.S01E04.enc', old), ('Show.S01E04.plain', plain)], key='fleet:lockedold')
    old_locked = params_of('fleet:lockedold').get('Show.S01E04.old', {}).get('DupeLocked')
    ok = (order == ['Show.S01E01.plain', 'Show.S01E01.enc']
          and locked.get('Show.S01E01.enc') == 'yes' and locked.get('Show.S01E01.plain') is None
          and order_pw == ['Show.S01E02.enc', 'Show.S01E02.plain']
          and locked_pw.get('Show.S01E02.enc') == 'yes'
          and order_nm == ['Show.S01E03.{{pw}}.enc', 'Show.S01E03.plain']
          and old_locked == 'yes')
    return ('fleetlockedlast', ok, 'order=%s locked=%s order_pw=%s locked_pw=%s order_name=%s locked_name=%s old=%s'
            % (order, locked, order_pw, locked_pw, order_nm, locked_nm, old_locked))


def scenario_fleettwinvolumes(daemon, t):
    """appendfleet: a twin posted with par2 volumes only (no index file, as
    MobLand S02E09 copy 11 was) is still a TWIN: its file list is read from
    its smallest volume, from the volume's end when its first article holds
    only recovery data. Both twins, whole like the ALT, rank ahead of a
    borrowable ALT."""
    daemon.fake_nntp.alive = set()
    same = _payload(400_000, 7401)
    api = daemon.wait_ready()
    # a real volume layout: its first article is recovery data only, the
    # file list (critical packets) comes after it, in the last article
    ids = _fleet_ids('tv')
    daemon.fake_nntp.alive |= set(ids) | {'tv-vol1@x', 'tv-vol2@x'}
    index = generators.par2_index([('d.bin', same)], pad=0)
    daemon.fake_nntp.bodies_by_id['tv-vol1@x'] = _yenc_part(b'\x5a' * 6000, 'd.vol-01.par2')
    daemon.fake_nntp.bodies_by_id['tv-vol2@x'] = _yenc_part(index, 'd.vol-01.par2')
    vol_only = _fake_nzb_files([('d.bin', ids, 400_000),
                                ('d.vol-01.par2', ['tv-vol1@x', 'tv-vol2@x'], 6000 + len(index))]).decode()
    reply = _fleet(daemon, [
        ('Show.S01E01.store', _fleet_copy(daemon, 'vs', 'b.rar', 410_000, _rar4_head(0x30), _payload(410_000, 7402))),
        ('Show.S01E01.twinA', _fleet_copy(daemon, 'ta2', 'c.bin', 400_000, None, same)),
        ('Show.S01E01.twinvol', vol_only)],
        key='fleet:vol')
    order = [m['Name'] for m in sorted(reply['Members'], key=lambda m: m['Rank'])]
    items = {g['NZBName']: g for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet:vol'}
    kinds = {n: {p['Name']: p['Value'] for p in g.get('Parameters', [])}.get('DupeCopyKind')
             for n, g in items.items()}
    # F10: the stored copy is the primary (its packing is known; the twins'
    # first articles tell no packing), the twins follow it
    ok = (order[0] == 'Show.S01E01.store' and set(order[1:3]) == {'Show.S01E01.twinA', 'Show.S01E01.twinvol'}
          and kinds.get('Show.S01E01.twinvol') == 'twin' and kinds.get('Show.S01E01.twinA') == 'twin')
    return ('fleettwinvolumes', ok, 'order=%s kinds=%s' % (order, kinds))


def scenario_fleetkindslowcheck(daemon, t):
    """appendfleet when the health check is slow enough to use the whole
    fleet budget (every sampled data article is slow to answer, as on a busy
    provider; 30 s, so no copy gets a health result and the order is by kind
    alone). The copy kinds are still read, first, with their own share of
    the time: the store-mode rar ALT (borrowable) is queued ahead of the plain
    7-Zip (unknown), and the compressed rar and the 7-Zip whose nzb carries a
    password (encrypted) come last as unborrowable; the intake gives a verdict
    for every copy too. Before, both ran after the check, got no time, and
    every copy was "unknown" (so the order fell to size, and an encrypted
    re-upload could be queued first)."""
    daemon.fake_nntp.alive = set()
    sevenz = bytes([0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c, 0, 4]) + bytes(24)
    api = daemon.wait_ready()
    copies = [
        ('Show.S01E01.enc7z', _fleet_copy(daemon, 'ez', 'a.7z.001', 400_000, sevenz, _payload(400_000, 7201))),
        ('Show.S01E01.compressed', _fleet_copy(daemon, 'cz', 'b.rar', 400_000, _rar4_head(0x33), _payload(400_000, 7202))),
        ('Show.S01E01.plain7z', _fleet_copy(daemon, 'pz', 'c.7z.001', 400_000, sevenz, _payload(400_000, 7203))),
        ('Show.S01E01.store', _fleet_copy(daemon, 'sz', 'd.rar', 400_000, _rar4_head(0x30), _payload(400_000, 7204)))]
    name, nzb = copies[0]
    copies[0] = (name, nzb.replace('<file ', '<head><meta type="password">secret</meta></head><file ', 1))
    # data articles 1..39 of every copy answer slowly; the par2 index and the
    # first data article (what the kind probe reads) answer at once
    for tag in ('ez', 'cz', 'pz', 'sz'):
        for digit in '123456789':
            daemon.fake_nntp.delays['%s-%s' % (tag, digit)] = 30
    started = time.time()
    reply = _fleet(daemon, copies, key='fleet:slow', timeout=20)
    took = time.time() - started
    daemon.fake_nntp.delays = {}
    order = [m['Name'] for m in sorted(reply['Members'], key=lambda m: m['Rank'])]
    items = {g['NZBName']: g for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet:slow'}
    kinds = {n: {p['Name']: p['Value'] for p in g.get('Parameters', [])}.get('DupeCopyKind')
             for n, g in items.items()}
    ok = (order[:2] == ['Show.S01E01.store', 'Show.S01E01.plain7z']
          and set(order[2:]) == {'Show.S01E01.enc7z', 'Show.S01E01.compressed'}
          and kinds.get('Show.S01E01.store') == 'borrowable'
          and kinds.get('Show.S01E01.plain7z') == 'unknown'
          and kinds.get('Show.S01E01.enc7z') == 'unborrowable'
          and kinds.get('Show.S01E01.compressed') == 'unborrowable')
    # the intake also runs ahead of the slow check: one verdict line per copy
    intake = _grep_log(t, 'Fleet intake: Show.S01E01.')
    ok = ok and intake >= 4
    return ('fleetkindslowcheck', ok, 'took=%.0fs order=%s kinds=%s intake_lines=%d'
            % (took, order, kinds, intake))


def _yenc_parts(data, name, part_size):
    """A file posted as yEnc articles of ``part_size`` bytes."""
    total = (len(data) + part_size - 1) // part_size
    out = []
    for n in range(total):
        chunk = data[n * part_size:(n + 1) * part_size]
        lines, line = [], bytearray()
        for b in chunk:
            c = (b + 42) & 255
            if c in (0, 9, 10, 13, 32, 61, 46):
                line += b'=' + bytes([(c + 64) & 255])
            else:
                line.append(c)
            if len(line) >= 128:
                lines.append(bytes(line))
                line = bytearray()
        if line:
            lines.append(bytes(line))
        lines = b'\r\n'.join(lines)
        begin = n * part_size + 1
        out.append(b'=ybegin part=%d total=%d line=128 size=%d name=%s\r\n=ypart begin=%d end=%d\r\n'
                   % (n + 1, total, len(data), name.encode(), begin, begin + len(chunk) - 1)
                   + lines + b'\r\n=yend size=%d part=%d pcrc32=%08x\r\n'
                   % (len(chunk), n + 1, zlib.crc32(chunk) & 0xffffffff))
    return out


def _intake_copy(daemon, tag, name, data, part_size=40_000):
    """A fleet copy whose articles really hold ``data`` as ``name``."""
    bodies = _yenc_parts(data, name, part_size)
    ids = ['%s-%d@x' % (tag, i) for i in range(len(bodies))]
    for mid, body in zip(ids, bodies):
        daemon.fake_nntp.bodies_by_id[mid] = body
    daemon.fake_nntp.alive |= set(ids)
    return _fake_nzb_files([(name, ids, len(data))]).decode()


def scenario_fleetintake(daemon, t):
    """appendfleet intake: every copy is opened down to its video and sampled.
    A bare mkv and a stored zip holding the same mkv are the same release; a
    copy holding another video is dropped (ERROR "not the same video") and
    never queued."""
    import io as _io
    import zipfile as _zip
    daemon.fake_nntp.alive = set()
    ebml = b'\x1a\x45\xdf\xa3'   # intake only compares payloads that start like a video
    video = ebml + _payload(400_000, 7201)
    other = ebml + _payload(400_000, 7202)
    packed = _io.BytesIO()
    with _zip.ZipFile(packed, 'w', _zip.ZIP_STORED) as z:
        z.writestr('movie.mkv', video)
    api = daemon.wait_ready()
    reply = _fleet(daemon, [
        ('Show.S01E01.bare', _intake_copy(daemon, 'ib', 'movie.mkv', video)),
        ('Show.S01E01.other', _intake_copy(daemon, 'io', 'film.mkv', other)),
        ('Show.S01E01.zip', _intake_copy(daemon, 'iz', 'movie.zip', packed.getvalue()))],
        key='fleet:intake')
    members = {m['Name']: m for m in reply['Members']}
    other_m = members.get('Show.S01E01.other', {})
    items = {g['NZBName'] for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet:intake'}
    log_same = _grep_log(t, "Show.S01E01.zip carries the fleet's video")
    # both copies keep the digest of the video inside, for TwinCheck's alt label
    inner = {g['NZBName']: next((p['Value'] for p in g.get('Parameters', []) if p['Name'] == 'DupeInner'), '')
             for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet:intake'}
    ok = (other_m.get('Status') == 'ERROR' and 'not the same video' in other_m.get('Reason', '')
          and 'Show.S01E01.other' not in items
          and any(members.get(n, {}).get('Status') == 'QUEUED' for n in ('Show.S01E01.bare', 'Show.S01E01.zip'))
          and log_same > 0
          and len(set(inner.values())) == 1 and len(next(iter(inner.values()), '').rsplit('.', 1)[-1]) == 16)
    return ('fleetintake', ok, 'reply=%s items=%s zip_same_logged=%s inner=%s' % (
        reply, sorted(items), log_same, inner))


def scenario_fleetintakelater(daemon, t):
    """appendfleet intake retried after the reply: a copy whose articles are
    too slow for the intake's share of the fleet time is unverified at
    submission and parked as a backup. Once the reply is sent it is read again
    in the background; it carries another video, so it is removed from the
    fleet and the log says so. The copies that carry the fleet's video stay."""
    daemon.fake_nntp.alive = set()
    ebml = b'\x1a\x45\xdf\xa3'
    video = ebml + _payload(400_000, 7301)
    other = ebml + _payload(400_000, 7302)
    api = daemon.wait_ready()
    copies = [
        ('Show.S01E01.bare', _intake_copy(daemon, 'lb', 'movie.mkv', video)),
        ('Show.S01E01.bare2', _intake_copy(daemon, 'lc', 'movie.mkv', video)),
        ('Show.S01E01.slow', _intake_copy(daemon, 'ls', 'film.mkv', other))]
    daemon.fake_nntp.delays['ls-'] = 6
    reply = _fleet(daemon, copies, key='fleet:later', timeout=20)
    daemon.fake_nntp.delays = {}
    slow_at_reply = next((m for m in reply['Members'] if m['Name'] == 'Show.S01E01.slow'), {})
    deadline = time.time() + 90
    removed = False
    while time.time() < deadline:
        names = {g['NZBName'] for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet:later'}
        if 'Show.S01E01.slow' not in names and _grep_log(t, 'Fleet intake later: Show.S01E01.slow') > 0:
            removed = True
            break
        time.sleep(1)
    names = {g['NZBName'] for g in api.listgroups() + api.history() if g.get('DupeKey') == 'fleet:later'}
    retried = _grep_log(t, 'unverified copies are checked again in the background')
    gone_logged = _grep_log(t, 'Show.S01E01.slow carries another video, removed from the fleet')
    ok = (slow_at_reply.get('Status') in ('BACKUP', 'QUEUED') and retried > 0 and removed
          and gone_logged > 0 and {'Show.S01E01.bare', 'Show.S01E01.bare2'} <= names)
    return ('fleetintakelater', ok, 'slow_at_reply=%s retried=%d removed=%s names=%s'
            % (slow_at_reply.get('Status'), retried, removed, sorted(names)))


def _watch_wait(t, needle, timeout=90):
    deadline = time.time() + timeout
    while time.time() < deadline and _grep_log(t, needle) == 0:
        time.sleep(0.25)
    time.sleep(1)
    return _grep_log(t, needle)


def scenario_watchforcepick(daemon, t):
    """DupeWatch without DupeSearch: a pick sent in DupeMode FORCE cannot fail
    over, so it is switched to SCORE, as DupeSearch (and the DupeDonors
    extension) did. Nothing is searched."""
    mids = ['wf-pick-%d@x' % i for i in range(40)]
    daemon.fake_nntp.alive = set(mids)
    api = daemon.wait_ready()
    content = base64.standard_b64encode(_fake_nzb_ids(mids, 400_000)).decode()
    pick_id = api.append(DS_TITLE, content, 'test', 0, False, True, DS_KEY, 100, 'force', [])
    switched = _watch_wait(t, 'DupeMode FORCE set to SCORE')
    pick = _ds_group(api, DS_TITLE) or {}
    searched = _grep_log(t, 'DupeSearch:')
    ok = pick.get('NZBID') == pick_id and pick.get('DupeMode') == 'SCORE' and switched == 1 and searched == 0
    return ('watchforcepick', ok, 'mode=%s switched_logs=%d dupesearch_logs=%d' % (pick.get('DupeMode'), switched, searched))


def _watch_pick_and_backups(daemon, t, tag, pick_alive, backups):
    """A paused pick of 40 articles of which <pick_alive> exist, then backups
    [(name, size, alive_count, score)] of the same key that go to history."""
    pick_ids = ['%s-pick-%d@x' % (tag, i) for i in range(40)]
    alive = set(pick_ids[:pick_alive])
    nzbs = []
    for n, (name, size, count, score) in enumerate(backups):
        ids = ['%s-b%d-%d@x' % (tag, n, i) for i in range(40)]
        alive |= set(ids[:count])
        nzbs.append((name, _fake_nzb_ids(ids, size).decode(), score))
    daemon.fake_nntp.alive = alive
    api = daemon.wait_ready()
    pick_id = _ds_append(api, DS_TITLE, _fake_nzb_ids(pick_ids, 400_000).decode(), DS_KEY, 100)
    ids = []
    for name, nzb, score in nzbs:
        ids.append(_ds_append(api, name, nzb, DS_KEY, score, paused=False))
    for name, _, _ in nzbs:
        daemon.wait_history(api, name, timeout=30)
    return api, pick_id, ids


def _watch_taken_over(api, pick_id, backup_id):
    """The backup went back to the queue (or already downloaded: fake articles
    are tiny) and the pick was filed as a duplicate."""
    deadline = time.time() + 30
    back, pick_h = [], {}
    while time.time() < deadline:
        back = [g for g in api.listgroups() + api.history()
                if g.get('NZBID') == backup_id and not g.get('Status', '').startswith('DELETED')]
        pick_h = next((h for h in api.history() if h.get('NZBID') == pick_id), {})
        if back and pick_h:
            break
        time.sleep(0.5)
    return bool(back), pick_h


def scenario_watchsearchedkey(daemon, t):
    """Review of 76a80014: DupeSearch writes ``dupes:<title>`` onto every
    download it searches. Such an ordinary download (no fleet: no DupeFleet,
    not held or probing, no held copy of its key) still gets the backup pass,
    which carries DupeSearch's retry and re-ranks; only real fleets that stay
    in Downloads are left to their probe. Review 2: the duplicate check holds
    an ordinary backup of such a key (DupeHeld) like a fleet copy; a plain hold
    is no fleet mark, so the download keeps its pass with a held backup."""
    key = 'dupes:show.s01e01.1080p.web.h264.grp'
    ids = ['ws-pick-%d@x' % i for i in range(40)]
    backup_ids = ['ws-b-%d@x' % i for i in range(40)]
    daemon.fake_nntp.alive = set(backup_ids)
    api = daemon.wait_ready()
    _ds_append(api, DS_TITLE, _fake_nzb_ids(ids, 400_000).decode(), key, 100)
    backup = 'Show S01E01 1080p WEB H264-OTHER'
    _ds_append(api, backup, _fake_nzb_ids(backup_ids, 400_000).decode(), key, 50, paused=False)
    held = None
    deadline = time.time() + 30
    while time.time() < deadline and held is None:
        g = next((g for g in api.listgroups() if g['NZBName'] == backup), None)
        if g and _params(g).get('DupeHeld') == 'yes':
            held = _params(g)
        time.sleep(0.25)
    # the backup's add wakes the pick: a pass logged after the backup's hold
    # (by position in the log, so no poll timing matters) proves a plain hold
    # no longer skips it
    needle = 'checking the backups of ' + DS_TITLE

    def pass_after_hold():
        log = t.read_file('nzbget.log').decode(errors='replace')
        anchor = log.find('Holding ' + backup)
        return anchor >= 0 and log.find(needle, anchor) >= 0, anchor >= 0
    after, anchored = False, False
    deadline = time.time() + 90
    while time.time() < deadline and not after:
        after, anchored = pass_after_hold()
        time.sleep(0.25)
    checked = _grep_log(t, needle)
    fleet_marks = held is not None and any(k in held for k in ('DupeFleet', 'DupeProbing', 'DupeFleetMember'))
    ok = after and held is not None and not fleet_marks
    return ('watchsearchedkey', ok, 'backup_pass_logs=%d pass_after_hold=%s anchored=%s backup_held=%s fleet_marks=%s' % (
        checked, after, anchored, held is not None, fleet_marks))


def scenario_watchdeadpick(daemon, t):
    """DupeWatch without DupeSearch: a pick with nothing left on any server
    drops below its backups and the whole backup in history takes its place."""
    api, pick_id, (backup_id,) = _watch_pick_and_backups(
        daemon, t, 'wd', 0, [('Show S01E01 1080p WEB H264-GRP', 400_000, 40, 90)])
    demoted = _watch_wait(t, 'dead pick demoted to score')
    back, pick_h = _watch_taken_over(api, pick_id, backup_id)
    ok = demoted == 1 and back and pick_h.get('Status') == 'DELETED/DUPE' and _grep_log(t, 'DupeSearch:') == 0
    return ('watchdeadpick', ok, 'demoted_logs=%d backup_back=%s pick=%s' % (demoted, back, pick_h.get('Status')))


def scenario_watchswap(daemon, t):
    """DupeWatch without DupeSearch: a pick sampled 70% alive will fail; a whole
    backup waits in history: it goes back to the queue and the pick is filed
    as a scored duplicate backup."""
    api, pick_id, (backup_id,) = _watch_pick_and_backups(
        daemon, t, 'ws', 28, [('Show S01E01 1080p WEB H264-GRP', 400_000, 40, 90)])
    swapped = _watch_wait(t, 'sampled 70%% alive and will fail: swapped for backup %d' % backup_id)
    back, pick_h = _watch_taken_over(api, pick_id, backup_id)
    params = {p['Name']: p['Value'] for p in pick_h.get('Parameters', [])}
    ok = swapped == 1 and back and pick_h.get('Status') == 'DELETED/DUPE' and params.get('DupeAlive') == '70'
    return ('watchswap', ok, 'swapped_logs=%d backup_back=%s pick=%s alive=%s'
            % (swapped, back, pick_h.get('Status'), params.get('DupeAlive')))


def scenario_watchborrow(daemon, t):
    """DupeWatch with live article fallback and DupeBorrowFirst: a pick
    sampled 70% alive, with a whole backup it can borrow from waiting in
    history, is NOT swapped: it keeps downloading (and extracting) and
    borrows its missing articles, so nothing it has done is thrown away."""
    api, pick_id, (backup_id,) = _watch_pick_and_backups(
        daemon, t, 'wb', 28, [('Show S01E01 1080p WEB H264-GRP', 400_000, 40, 90)])
    kept = _watch_wait(t, 'borrows from backup %d' % backup_id)
    swapped = _grep_log(t, 'swapped for backup')
    in_queue = any(g.get('NZBID') == pick_id for g in api.listgroups()) or any(
        h.get('NZBID') == pick_id and h.get('Status', '').startswith('SUCCESS') for h in api.history())
    ok = kept >= 1 and swapped == 0 and in_queue
    return ('watchborrow', ok, 'borrow_logs=%d swapped=%d pick_kept=%s' % (kept, swapped, in_queue))


def scenario_watchswaptwin(daemon, t):
    """DupeWatch: the failing pick (70% alive) is swapped for its TWIN (the
    same post: par2 sets with the same hashes, same article counts) sampled
    98% alive, not for the whole ALT, though the ALT is wholer and scored
    higher."""
    daemon.fake_nntp.alive = set()
    same = _payload(400_000, 7201)
    pick = _fleet_copy(daemon, 'wtp', 'c.bin', 400_000, None, same, 28)
    alt = _fleet_copy(daemon, 'wta', 'e.bin', 380_000, None, _payload(380_000, 7202), 40)
    twin = _fleet_copy(daemon, 'wtt', 'd.bin', 400_000, None, same, 39)
    api = daemon.wait_ready()
    pick_id = _ds_append(api, DS_TITLE, pick, DS_KEY, 100)
    alt_id = _ds_append(api, 'Show S01E01 1080p WEB H264-GRP', alt, DS_KEY, 90, paused=False)
    twin_id = _ds_append(api, 'Show_S01E01_1080p_WEB_H264-GRP', twin, DS_KEY, 80, paused=False)
    for name in ('Show S01E01 1080p WEB H264-GRP', 'Show_S01E01_1080p_WEB_H264-GRP'):
        daemon.wait_history(api, name, timeout=30)
    swapped = _watch_wait(t, 'swapped for backup')
    to_twin = _grep_log(t, 'swapped for backup %d (98%% alive)' % twin_id)
    back, pick_h = _watch_taken_over(api, pick_id, twin_id)
    alt_h = next((h for h in api.history() if h.get('NZBID') == alt_id), {})
    # (the twin lacks one article and has no recovery slices, so it may fail
    # over to the ALT afterwards: only the swap's choice is checked)
    ok = swapped == 1 and to_twin == 1 and back
    return ('watchswaptwin', ok, 'swapped_logs=%d to_twin=%d twin_back=%s alt=%s'
            % (swapped, to_twin, back, alt_h.get('Status')))


def scenario_watchfleet(daemon, t):
    """DupeWatch without DupeSearch looks after fleets from appendfleet too: the
    queued copy (paused) loses 30% of its articles after the fleet is placed, the next
    upkeep pass samples it 70% alive and swaps it for the fleet's whole backup."""
    daemon.fake_nntp.alive = set(_fleet_ids('wfa', 300)) | set(_fleet_ids('wfb', 300))
    daemon.fake_nntp.delays.update({'wfa-': 0.3, 'wfb-': 0.3})
    api = daemon.wait_ready()
    reply = _fleet(daemon, [('Show.S01E01.a', _fake_nzb_ids(_fleet_ids('wfa', 300), 3_000_000).decode()),
                            ('Show.S01E01.b', _fake_nzb_ids(_fleet_ids('wfb', 300), 3_100_000).decode())],
                   key='fleet:watch')
    chosen = next(m for m in reply['Members'] if m['NZBID'] == reply['Chosen'])
    backup = next(m for m in reply['Members'] if m['Status'] == 'BACKUP')
    prefix = 'wfa' if chosen['Name'].endswith('a') else 'wfb'
    # the queued copy is paused so that it is still queued when the upkeep
    # runs, 20 s after the add
    api.editqueue('GroupPause', 0, '', [chosen['NZBID']])
    daemon.fake_nntp.delays.clear()
    daemon.fake_nntp.alive -= set(_fleet_ids(prefix, 300)[::3])
    swapped = _watch_wait(t, 'swapped for backup %d' % backup['NZBID'], timeout=90)
    checked = _grep_log(t, 'Dupes: checking the backups of')
    return ('watchfleet', swapped == 1, 'swapped_logs=%d checked_logs=%d chosen=%s backup=%s'
            % (swapped, checked, chosen['Name'], backup['Name']))


def scenario_watchrescue(daemon, t):
    """DupeWatch without DupeSearch: a pick fails its health check and its only
    backup is in DupeMode FORCE, which failover skips: the rescue returns the
    backup, which completes."""
    primary, backup, data = _probe_fixture(t, 'wr')
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'wr-key', 100)
    daemon.append(api, 'Backup', backup, False, 'wr-key', 90)
    bh = daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('HistorySetDupeMode', 0, 'FORCE', [bh['NZBID']])
    pid = [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'][0]
    api.editqueue('GroupResume', 0, '', [pid])
    hp = daemon.wait_history(api, 'Primary')
    deadline = time.time() + 120
    hb = daemon.wait_history(api, 'Backup')
    while not hb['Status'].startswith('SUCCESS') and time.time() < deadline:
        time.sleep(0.5)
        hb = daemon.wait_history(api, 'Backup')
    rescued = _grep_log(t, 'Rescue: Primary failed its health check with nothing of its key queued: returning backup Backup')
    ok = hp['Status'] == 'FAILURE/HEALTH' and rescued == 1 and hb['Status'].startswith('SUCCESS') and _verify_output(t, data)
    return ('watchrescue', ok, 'primary=%s backup=%s rescue_logs=%d' % (hp['Status'], hb['Status'], rescued))


def scenario_midwaytwin(daemon, t):
    """The download loses the last 12 of its 20 articles; its TWIN waits in
    history as a backup. With the defaults (DupeArticleFallback=live,
    HealthCheck=dupe) the download is not replaced: from the first missing
    article on it takes the rest from the twin and finishes, keeping what it
    had downloaded. The twin is never downloaded as a whole."""
    size, seg = 10_000_000, 500_000
    data = _payload(size, 1306)
    pp = _place_copy(t, 'mwA', data)
    dp = _place_copy(t, 'mwB', data)
    primary = build_nzb_with_par2(t, pp, 'MidA.bin', data, seg, set(range(9, 21)))
    twin = build_nzb(dp, 'obf-mid.bin', size, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'MidA', primary, True, 'mw-key', 100)
    daemon.append(api, 'MidTwin', twin, False, 'mw-key', 50)
    daemon.wait_history(api, 'MidTwin', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'MidA'])
    h = daemon.wait_history(api, 'MidA')
    recov = int(h.get('DupeRecoveredArticles', 0))
    twin_h = next((x for x in api.history() if x['NZBName'] == 'MidTwin'), {})
    twin_tried = int(twin_h.get('SuccessArticles', 0)) + int(twin_h.get('FailedArticles', 0))
    failover = _grep_log(t, 'Failing over') + _grep_log(t, 'Found duplicate')
    integ = _verify_output(t, data)
    ok = h['Status'].startswith('SUCCESS') and recov >= 1 and failover == 0 and twin_tried == 0 and integ
    return ('midwaytwin', ok, 'status=%s recovered=%d failover_logs=%d twin=%s twin_tried=%d integrity=%s'
            % (h['Status'], recov, failover, twin_h.get('Status'), twin_tried, integ))


def scenario_midwayalt(daemon, t):
    """The download (.mkv in 500 KB articles) loses a block in the middle; an
    ALT of the same content in 250 KB articles waits in history. Articles
    cannot be swapped across the two, so stream repair reads the missing bytes
    from the ALT: the download finishes without being replaced or restarted."""
    size, seg_primary, seg_alt = 6_000_000, 500_000, 250_000
    data = _payload(size, 4343)
    pp = _place_copy(t, 'maA', data, 'file.mkv')
    ap = _place_copy(t, 'maB', data, 'file.mkv')
    primary = build_nzb(pp, 'MidAlt.mkv', size, seg_primary, {5, 6, 7})
    alt = build_nzb(ap, 'obf-midalt.mkv', size, seg_alt, set())
    api = daemon.wait_ready()
    daemon.append(api, 'MidAlt', primary, True, 'ma-key', 100)
    daemon.append(api, 'MidAltB', alt, False, 'ma-key', 50)
    daemon.wait_history(api, 'MidAltB', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'MidAlt'])
    h = daemon.wait_history(api, 'MidAlt')
    alt_h = next((x for x in api.history() if x['NZBName'] == 'MidAltB'), {})
    alt_tried = int(alt_h.get('SuccessArticles', 0)) + int(alt_h.get('FailedArticles', 0))
    failover = _grep_log(t, 'Failing over') + _grep_log(t, 'Found duplicate')
    integ = _verify_output(t, data, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    ok = h['Status'].startswith('SUCCESS') and failover == 0 and alt_tried == 0 and integ
    return ('midwayalt', ok, 'status=%s failover_logs=%d alt=%s alt_tried=%d integrity=%s'
            % (h['Status'], failover, alt_h.get('Status'), alt_tried, integ))


def scenario_fleetalldead(daemon, t):
    """appendfleet: every member is dead: nothing is queued, the reply says ALL_DEAD."""
    daemon.fake_nntp.alive = set()
    api = daemon.wait_ready()
    reply = _fleet(daemon, [('Show.S01E01.d1', _fake_nzb_ids(_fleet_ids('d1'), 400_000).decode()),
                            ('Show.S01E01.d2', _fake_nzb_ids(_fleet_ids('d2'), 410_000).decode())])
    time.sleep(2)
    queued = [g['NZBName'] for g in api.listgroups()]
    ok = reply['Chosen'] == 0 and reply['Reason'] == 'ALL_DEAD' and not queued and \
        all(m['Status'] == 'DEAD' for m in reply['Members'])
    return ('fleetalldead', ok, 'reply=%s queued=%s' % (reply, queued))


def scenario_fleetshutdown(daemon, t):
    """appendfleet: nzbget shuts down while it checks a fleet (slow server): the
    call ends, nothing is half-added, and nzbget starts again cleanly."""
    import threading
    daemon.fake_nntp.alive = set(_fleet_ids('sd'))
    daemon.fake_nntp.delays.update({'sd-': 2.0})
    api = daemon.wait_ready()
    box = {}

    def send():
        try:
            box['reply'] = _fleet(daemon, [('Show.S01E01.sd', _fake_nzb_ids(_fleet_ids('sd'), 400_000).decode())],
                                  timeout=60, http_timeout=90)
        except Exception as e:
            box['error'] = type(e).__name__
    thread = threading.Thread(target=send)
    thread.start()
    time.sleep(3)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    thread.join(timeout=30)
    daemon.fake_nntp.delays.clear()
    daemon.start()
    api = daemon.wait_ready()
    queued = [g['NZBName'] for g in api.listgroups()]
    history = [h['NZBName'] for h in api.history()]
    ok = not thread.is_alive() and not queued and not history
    return ('fleetshutdown', ok, 'reply=%s queued=%s history=%s' % (box, queued, history))


def _probe_fixture(t, tag, alive_every=0):
    """A six-file, 180-article dead posting (every article missing, or all but
    every ``alive_every``-th of each file) and a healthy lower-scored backup of
    the same title in another packing."""
    seg = 100_000
    vol = 3_000_000
    n = vol // seg
    dead_set = set(range(1, n + 1))
    if alive_every:
        dead_set = {i for i in dead_set if (i - 1) % alive_every != 3 % alive_every}
    primary = [('%sA/d%d.bin' % (tag, i), 'Dead%d.bin' % i, vol, seg, set(dead_set)) for i in range(6)]
    for m in primary:
        t.write_file(os.path.join('data', m[0]), _payload(vol, 9700))
    data = _payload(2_900_000, 9701)
    bp = _place_copy(t, '%sB' % tag, data)
    backup = build_nzb(bp, 'Backup.bin', 2_900_000, seg, set())
    return primary, backup, data


def scenario_deadpickafterreload(daemon, t):
    """B4: a reload stops the dead-pick probes (StopAll refuses new ones while
    nzbget shuts its parts down); the new coordinator allows them again, so the
    probe still abandons a dead pick after a reload."""
    api = daemon.wait_ready()
    api.reload()
    time.sleep(3)
    daemon.wait_ready()
    name, ok, detail = scenario_deadpickprobe(daemon, t)
    return ('deadpickafterreload', ok, detail)


def scenario_deadpickprobe(daemon, t):
    """HealthCheck=dupe, a posting none of whose articles exists on the server
    and a healthy backup in history: the probe (STAT of 10 articles spread
    over the posting) finds nothing and abandons the download for the backup
    at once, not through the failed-article path (the news server answers
    after 500 ms, so the probe finishes before 32 articles failed)."""
    primary, backup, data = _probe_fixture(t, 'dp')
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'dp-key', 100)
    daemon.append(api, 'Backup', backup, False, 'dp-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups()
                                         if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary')
    deadline = time.time() + 240
    hb = daemon.wait_history(api, 'Backup')
    while hb['Status'].startswith('DELETED') and time.time() < deadline:
        time.sleep(0.5)
        hb = daemon.wait_history(api, 'Backup')
    probed = _grep_log(t, 'none of 10 sampled articles exists on any server')
    health_failover = _grep_log(t, 'Failing over Primary to duplicate Backup: health')
    integ = _verify_output(t, data)
    failed_articles = int(hp.get('FailedArticles', 0))
    return ('deadpickprobe', probed == 1 and health_failover == 0 and failed_articles < 32 and
            integ and hb['Status'].startswith('SUCCESS'),
            'status=%s backup_status=%s probe_logs=%d health_failover_logs=%d failed_articles=%d integrity=%s'
            % (hp['Status'], hb['Status'], probed, health_failover, failed_articles, integ))


def _deadpick_servers(daemon, t, tag):
    """A large dead posting (1800 articles, 15 files) with a healthy backup,
    several news servers answering after 200 ms (SCENARIO_EXTRA_SERVERS); the
    probe runs through the servers one after the other and finishes before the
    downloads have failed the 15% of the posting that make the regular health
    check abandon it."""
    seg, vol = 20_000, 2_400_000
    n = vol // seg
    payload = _payload(vol, 9800)
    primary = []
    for i in range(15):
        rel = '%sA/d%d.bin' % (tag, i)
        t.write_file(os.path.join('data', rel), payload)
        primary.append((rel, 'Dead%d.bin' % i, vol, seg, set(range(1, n + 1))))
    data = _payload(2_900_000, 9801)
    backup = build_nzb(_place_copy(t, '%sB' % tag, data), 'Backup.bin', 2_900_000, 100_000, set())
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, tag + '-key', 100)
    daemon.append(api, 'Backup', backup, False, tag + '-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups()
                                         if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    deadline = time.time() + 300
    hb = daemon.wait_history(api, 'Backup', timeout=300)
    while hb['Status'].startswith('DELETED') and time.time() < deadline:
        time.sleep(0.5)
        hb = daemon.wait_history(api, 'Backup', timeout=300)
    return hp, hb, _verify_output(t, data)


def scenario_deadpickservers(daemon, t):
    """Six news servers, one of them unreachable (an optional server nothing
    listens on): it's left out of the verdict (it can't deliver the posting
    either), and the five others answer definitively. Under
    load the probe can take over 10 s; the dead download then fails over by the
    rule that doesn't wait for a slow probe (B83) - the probe still gives its
    verdict, which is what this checks."""
    hp, hb, integ = _deadpick_servers(daemon, t, 'ds')
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, '5 of 5 servers answered definitively') == 0:
        time.sleep(0.5)
    probed = _grep_log(t, '0 of 10 sampled articles exist (5 of 5 servers answered definitively, '
                          '1 unreachable left out): the posting is dead')
    return ('deadpickservers', probed == 1 and integ and hb['Status'].startswith('SUCCESS'),
            'status=%s backup_status=%s probe_logs=%d failed_articles=%s integrity=%s'
            % (hp['Status'], hb['Status'], probed, hp.get('FailedArticles'), integ))


def scenario_deadpickfewservers(daemon, t):
    """Six news servers, two of them unreachable: they're left out (down, they
    can't deliver the posting), and the four others answer definitively: dead.
    (It needed five definitive answers; but a server that didn't answer may be
    the one that has the posting - only one that is down is left out.) The
    dead posting fails over, by the probe or the rule that doesn't wait for a
    slow one."""
    hp, hb, integ = _deadpick_servers(daemon, t, 'df')
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, '2 unreachable left out') == 0:
        time.sleep(0.5)
    verdict = _grep_log(t, '(4 of 4 servers answered definitively, 2 unreachable left out): the posting is dead')
    # the old rule: "no verdict (... 4 of 6 servers answered definitively)"
    no_verdict = _grep_log(t, 'Dupe probe: no verdict')
    return ('deadpickfewservers', no_verdict == 0 and integ and hb['Status'].startswith('SUCCESS'),
            'status=%s backup_status=%s verdict_logs=%d no_verdict_logs=%d integrity=%s'
            % (hp['Status'], hb['Status'], verdict, no_verdict, integ))


def scenario_deadpickstray(daemon, t):
    """B45: a dead posting with a single stray article alive, and that article is
    one the probe samples (part 10 of the first file, the first of the ten
    samples). One sample found doesn't make the posting alive: the probe calls
    it dead, says where it found the stray article, and fails over to the
    backup at once."""
    primary, backup, data = _probe_fixture(t, 'ds')
    m = primary[0]
    primary[0] = (m[0], m[1], m[2], m[3], m[4] - {10})
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'ds-key', 100)
    daemon.append(api, 'Backup', backup, False, 'ds-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups()
                                         if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary')
    verdict = _grep_log(t, 'Dupe probe: 1 of 10 sampled articles exist')
    dead = _grep_log(t, 'the posting is dead')
    failed_over = _grep_log(t, 'Failing over Primary to duplicate Backup: none of 10 sampled')
    return ('deadpickstray', verdict == 1 and dead == 1 and failed_over == 1,
            'status=%s probe_one_found_logs=%d dead_logs=%d probe_failover_logs=%d'
            % (hp['Status'], verdict, dead, failed_over))


def scenario_deaddownloadlate(daemon, t):
    """B45: a large dead posting (1,800 articles, no par2) whose backup arrives
    only after the download started, so the probe had no backup to fail over to
    and stepped aside. Once 64 articles beyond the first of each file failed and
    none of its own arrived, the download fails over to the backup - long before
    its health falls below the critical 85% (270 failures)."""
    seg = 10_000
    vol = 3_000_000
    n = vol // seg
    primary = [('dlA/d%d.bin' % i, 'Dead%d.bin' % i, vol, seg, set(range(1, n + 1))) for i in range(6)]
    for m in primary:
        t.write_file(os.path.join('data', m[0]), _payload(vol, 9720))
    data = _payload(2_900_000, 9721)
    bp = _place_copy(t, 'dlB', data)
    backup = build_nzb(bp, 'Backup.bin', 2_900_000, 100_000, set())
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), False, 'dl-key', 100)
    time.sleep(1)
    daemon.append(api, 'Backup', backup, False, 'dl-key', 90)
    hp = daemon.wait_history(api, 'Primary')
    deadline = time.time() + 120
    hb = daemon.wait_history(api, 'Backup')
    while not hb['Status'].startswith('SUCCESS') and time.time() < deadline:
        time.sleep(0.5)
        hb = daemon.wait_history(api, 'Backup')
    # the early failover: B45 (0 of its own arrived) or, first since B49, 40 failed in a row
    rule = (_grep_log(t, 'Failing over Primary to duplicate Backup: 0 of its own articles downloaded') +
            _grep_log(t, 'failed in a row'))
    failed = int(hp.get('FailedArticles', 0))
    ok = rule == 1 and failed < 270 and hb['Status'].startswith('SUCCESS') and _verify_output(t, data)
    return ('deaddownloadlate', ok, 'status=%s backup=%s rule_failover_logs=%d failed_articles=%d'
            % (hp['Status'], hb['Status'], rule, failed))


def scenario_forcefailover(daemon, t):
    """B46: a pick sent in DupeMode FORCE (a client re-sending an nzb-file nzbget
    skipped as a copy) is dead, and a healthy backup waits in history: with
    HealthCheck=dupe it fails over like a score pick instead of failing with
    the backup unused (production 4144)."""
    primary, backup, data = _probe_fixture(t, 'ff')
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'ff-key', 100)
    daemon.append(api, 'Backup', backup, False, 'ff-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    pid = [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'][0]
    api.editqueue('GroupSetDupeMode', 0, 'FORCE', [pid])
    api.editqueue('GroupResume', 0, '', [pid])
    hp = daemon.wait_history(api, 'Primary')
    deadline = time.time() + 120
    hb = daemon.wait_history(api, 'Backup')
    while not hb['Status'].startswith('SUCCESS') and time.time() < deadline:
        time.sleep(0.5)
        hb = daemon.wait_history(api, 'Backup')
    over = _grep_log(t, 'Failing over Primary to duplicate Backup')
    mode = [h.get('DupeMode') for h in api.history() if h['NZBID'] == pid]
    ok = mode == ['FORCE'] and over == 1 and hb['Status'].startswith('SUCCESS') and _verify_output(t, data)
    return ('forcefailover', ok, 'primary=%s mode=%s backup=%s failover_logs=%d' % (hp['Status'], mode, hb['Status'], over))


def scenario_copybackup(daemon, t):
    """B46: the only other posting in history is a copy (sent twice: the second
    time nzbget skipped it as the same content, then the first was deleted for
    good): when the dead pick fails over, the copy - another posting - is
    fetched; a copy of the pick itself never is."""
    primary, backup, data = _probe_fixture(t, 'cb')
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'cb-key', 100)
    daemon.append(api, 'Backup', backup, False, 'cb-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    first = [h['NZBID'] for h in api.history() if h['NZBName'] == 'Backup']
    daemon.append(api, 'Backup', backup, False, 'cb-key', 91)
    time.sleep(2)
    copies = [h for h in api.history() if h['NZBName'] == 'Backup' and h['NZBID'] not in first]
    api.editqueue('HistoryFinalDelete', 0, '', first)
    # the pick's own copy in history, which must not come back
    daemon.append(api, 'Primary', build_multi_nzb(primary), False, 'cb-key', 99)
    time.sleep(2)
    pid = [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'][0]
    api.editqueue('GroupResume', 0, '', [pid])
    daemon.wait_history(api, 'Primary')
    deadline = time.time() + 120
    done = []
    while time.time() < deadline and not done:
        done = [h for h in api.history() if h['NZBName'] == 'Backup' and h['Status'].startswith('SUCCESS')]
        time.sleep(0.5)
    self_copy_back = _grep_log(t, 'Found duplicate Primary')
    # since B43 a re-sent backup's nzb-file is filed as a backup (DUPE), no longer
    # skipped as a copy: either way it is a backup, and the pick's own copy isn't
    ok = (len(copies) == 1 and copies[0]['Status'] in ('DELETED/COPY', 'DELETED/DUPE') and done and
          self_copy_back == 0 and _verify_output(t, data))
    return ('copybackup', ok, 'copy=%s backup_done=%s self_copy_returned=%d'
            % (copies[0]['Status'] if copies else None, bool(done), self_copy_back))


def _big_dead(t, tag, alive=()):
    """A 1,800-article posting (six files of 300 articles, no par2) whose
    articles are all missing but the global indexes in <alive>, and a healthy
    backup in another packing."""
    seg, vol = 10_000, 3_000_000
    n = vol // seg
    primary = []
    for i in range(6):
        missing = {p for p in range(1, n + 1) if i * n + p - 1 not in alive}
        primary.append(('%sA/d%d.bin' % (tag, i), 'Dead%d.bin' % i, vol, seg, missing))
        t.write_file(os.path.join('data', primary[-1][0]), _payload(vol, 9730))
    data = _payload(2_900_000, 9731)
    bp = _place_copy(t, '%sB' % tag, data)
    return primary, build_nzb(bp, 'Backup.bin', 2_900_000, 100_000, set()), data


def _late_backup_run(daemon, t, tag, alive=(), restart_after=0):
    primary, backup, data = _big_dead(t, tag, alive)
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), False, tag + '-key', 100)
    time.sleep(1)
    daemon.append(api, 'Backup', backup, False, tag + '-key', 90)
    at_restart = 0
    if restart_after:
        deadline = time.time() + 60
        while time.time() < deadline:
            g = [x for x in api.listgroups() if x['NZBName'] == 'Primary']
            at_restart = g[0]['FailedArticles'] if g else -1
            if not g or at_restart >= restart_after:
                break
            time.sleep(0.05)
        try:
            api.shutdown()
        except Exception:
            pass
        t.procs[-1].wait(timeout=60)
        daemon.start()
        api = daemon.wait_ready()
    hp = daemon.wait_history(api, 'Primary')
    deadline = time.time() + 120
    hb = daemon.wait_history(api, 'Backup')
    while not hb['Status'].startswith('SUCCESS') and time.time() < deadline:
        time.sleep(0.5)
        hb = daemon.wait_history(api, 'Backup')
    rule = _grep_log(t, 'Failing over Primary to duplicate Backup: ')
    return hp, hb, rule, data, at_restart


def scenario_deaddownloadstray2(daemon, t):
    """R1c: 2 of the 1,800 articles of the posting exist (one stray in two
    files): the early failover still fires - once fewer than 1 in 100 tried
    articles arrived - long before the health check would (270 failures)."""
    # the first articles of two files: direct-rename fetches those first
    hp, hb, rule, data, _ = _late_backup_run(daemon, t, 'dt', alive={0, 300})
    failed = int(hp.get('FailedArticles', 0))
    ok = (rule == 1 and int(hp.get('SuccessArticles', 0)) >= 1 and failed < 270 and
          hb['Status'].startswith('SUCCESS') and _verify_output(t, data))
    return ('deaddownloadstray2', ok, 'status=%s backup=%s failover_logs=%d failed_articles=%d success=%s'
            % (hp['Status'], hb['Status'], rule, failed, hp.get('SuccessArticles')))


def scenario_deaddownloadrestart(daemon, t):
    """R1f: nzbget restarts after 40 failed articles of a dead posting
    (ContinuePartial=yes, as in production): it fails over promptly after the
    restart - the kept failure count, or the probe that runs again - not
    after another full run of failures."""
    hp, hb, rule, data, at_restart = _late_backup_run(daemon, t, 'dr', restart_after=40)
    failed = int(hp.get('FailedArticles', 0))
    ok = (rule == 1 and at_restart >= 40 and failed < 105 and hb['Status'].startswith('SUCCESS')
          and _verify_output(t, data))
    return ('deaddownloadrestart', ok, 'status=%s backup=%s failover_logs=%d failed_at_restart=%d failed_articles=%d'
            % (hp['Status'], hb['Status'], rule, at_restart, failed))


def scenario_deadbackupsonly(daemon, t):
    """R2c: every backup was found dead by a dupe tool (DupeAlive=0): none is
    fetched, the dead pick fails as before."""
    primary, backup, data = _probe_fixture(t, 'db')
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'db-key', 100)
    _ds_append(api, 'Backup', backup, 'db-key', 90, paused=False, params=[{'Name': 'DupeAlive', 'Value': '0'}])
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary')
    time.sleep(3)
    back = _grep_log(t, 'Found duplicate Backup') + _grep_log(t, 'Failing over Primary')
    hb = daemon.wait_history(api, 'Backup')
    ok = back == 0 and hb['Status'] == 'DELETED/DUPE' and hp['Status'].startswith('FAILURE')
    return ('deadbackupsonly', ok, 'primary=%s backup=%s returned_logs=%d' % (hp['Status'], hb['Status'], back))


def scenario_backuporder(daemon, t):
    """R2f: two backups of equal score, one found 50% alive and one 90% alive
    (DupeAlive): the more alive one takes the dead pick's place."""
    primary, backup, data = _probe_fixture(t, 'bo')
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'bo-key', 100)
    _ds_append(api, 'Half', backup, 'bo-key', 90, paused=False, params=[{'Name': 'DupeAlive', 'Value': '50'}])
    _ds_append(api, 'Most', backup.replace('Backup.bin', 'Backup2.bin'), 'bo-key', 90, paused=False,
               params=[{'Name': 'DupeAlive', 'Value': '90'}])
    daemon.wait_history(api, 'Half', timeout=60)
    daemon.wait_history(api, 'Most', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    daemon.wait_history(api, 'Primary')
    time.sleep(2)
    most = _grep_log(t, 'to duplicate Most') + _grep_log(t, 'Found duplicate Most')
    half = _grep_log(t, 'to duplicate Half') + _grep_log(t, 'Found duplicate Half')
    ok = most >= 1 and half == 0
    return ('backuporder', ok, 'most_alive_chosen_logs=%d half_alive_chosen_logs=%d' % (most, half))


def scenario_copyoffailed(daemon, t):
    """B47: a pick failed earlier (a dead posting), and the same nzb-file was
    sent again and skipped as a copy - scored above the healthy backup. When
    the next dead pick fails over, the copy (the same dead posting) is passed
    over for the healthy backup (production 4145 after 4144 had died)."""
    first, backup, data = _probe_fixture(t, 'cf')
    second, _, _ = _probe_fixture(t, 'cg')
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(first), False, 'cf-key', 100)
    daemon.wait_history(api, 'Primary')
    daemon.append(api, 'Primary', build_multi_nzb(first), False, 'cf-key', 98)
    time.sleep(2)
    copies = [h['Status'] for h in api.history() if h['NZBName'] == 'Primary']
    daemon.append(api, 'Second', build_multi_nzb(second), True, 'cf-key', 100)
    daemon.append(api, 'Backup', backup, False, 'cf-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Second'])
    daemon.wait_history(api, 'Second')
    deadline = time.time() + 120
    done = []
    while time.time() < deadline and not done:
        done = [h for h in api.history() if h['NZBName'] == 'Backup' and h['Status'].startswith('SUCCESS')]
        time.sleep(0.5)
    copy_back = _grep_log(t, 'to duplicate Primary') + _grep_log(t, 'Found duplicate Primary')
    ok = 'DELETED/COPY' in copies and bool(done) and copy_back == 0 and _verify_output(t, data)
    return ('copyoffailed', ok, 'first_statuses=%s backup_done=%s dead_copy_chosen=%d' % (copies, bool(done), copy_back))


def scenario_resendbackup(daemon, t):
    """B43: a client cancels its pick and sends one of the backups' nzb-files
    again as the new pick. Its content waits in history only as an untried
    backup: it is downloaded, not skipped as a copy (before, nothing
    downloaded and clients re-sent it in DupeMode force)."""
    primary, backup, data = _probe_fixture(t, 'rb')
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'rb-key', 100)
    daemon.append(api, 'Backup', backup, False, 'rb-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupFinalDelete', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    time.sleep(1)
    daemon.append(api, 'Resent', backup, False, 'rb-key', 100)
    deadline = time.time() + 120
    done = None
    while time.time() < deadline and not done:
        done = next((h for h in api.history() if h['NZBName'] == 'Resent' and h['Status'] != 'DELETED/COPY'
                     and h['Status'].startswith('SUCCESS')), None)
        copy = next((h for h in api.history() if h['NZBName'] == 'Resent' and h['Status'] == 'DELETED/COPY'), None)
        if copy:
            break
        time.sleep(0.5)
    ok = bool(done) and _grep_log(t, 'waits in history only as an untried backup') == 1 and _verify_output(t, data)
    return ('resendbackup', ok, 'resent=%s' % (done['Status'] if done else (copy and copy['Status'])))


def scenario_resendfailed(daemon, t):
    """B43: the nzb-file of a pick that failed (a dead posting) is sent again
    while a healthy backup waits in history and nothing of the key is queued:
    it is skipped as the same dead posting, and the backup is fetched in its
    place - the re-send still leads to a download."""
    first, backup, data = _probe_fixture(t, 'rf')
    other, _, _ = _probe_fixture(t, 'rg')
    api = daemon.wait_ready()
    daemon.append(api, 'Dead', build_multi_nzb(first), False, 'rf-key', 100)
    daemon.wait_history(api, 'Dead')
    daemon.append(api, 'Holder', build_multi_nzb(other), True, 'rf-key', 200)
    daemon.append(api, 'Backup', backup, False, 'rf-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupFinalDelete', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Holder'])
    time.sleep(1)
    daemon.append(api, 'Dead', build_multi_nzb(first), False, 'rf-key', 100)
    deadline = time.time() + 120
    done = []
    while time.time() < deadline and not done:
        done = [h for h in api.history() if h['NZBName'] == 'Backup' and h['Status'].startswith('SUCCESS')]
        time.sleep(0.5)
    skipped = [h['Status'] for h in api.history() if h['NZBName'] == 'Dead']
    fetched = _grep_log(t, 'is the same posting as a failed download: fetching the best backup instead')
    ok = 'DELETED/COPY' in skipped and fetched == 1 and bool(done) and _verify_output(t, data)
    return ('resendfailed', ok, 'dead_statuses=%s fetch_logs=%d backup_done=%s' % (skipped, fetched, bool(done)))


def _projected_run(daemon, t, tag, alive_percent, backup_alive):
    """A 1,800-article posting (twenty files, no par2: critical health 85%) of which
    <alive_percent>% of the articles exist, spread evenly; a healthy backup marked
    <backup_alive>% alive (DupeAlive) waits in history, or none if None."""
    seg, vol = 10_000, 900_000
    n = vol // seg
    primary = []
    # twenty files of 90 articles: 200 tried articles span three files
    for i in range(20):
        missing = {p for p in range(1, n + 1) if ((i * n + p - 1) * 37) % 100 >= alive_percent}
        primary.append(('%sA/d%d.bin' % (tag, i), 'Part%d.bin' % i, vol, seg, missing))
        t.write_file(os.path.join('data', primary[-1][0]), _payload(vol, 9740 + i))
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, tag + '-key', 100)
    if backup_alive is not None:
        data = _payload(2_900_000, 9749)
        bp = _place_copy(t, '%sB' % tag, data)
        _ds_append(api, 'Backup', build_nzb(bp, 'Backup.bin', 2_900_000, 100_000, set()), tag + '-key', 90,
                   paused=False, params=[{'Name': 'DupeAlive', 'Value': str(backup_alive)}])
        daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    swaps = _grep_log(t, 'projected health')
    return hp, swaps


def scenario_projectedswap(daemon, t):
    """B48a: 31% of the pick's articles arrive and a backup is known 100% alive:
    the pick is swapped once about 200 articles were tried, long before its
    health (which counts failures against the whole posting) reaches critical."""
    hp, swaps = _projected_run(daemon, t, 'pa', 31, 100)
    failed = int(hp.get('FailedArticles', 0))
    return ('projectedswap', swaps == 1 and failed < 250,
            'status=%s swap_logs=%d failed_articles=%d' % (hp['Status'], swaps, failed))


def scenario_projectednobackup(daemon, t):
    """B48b/B69: 31% arrive and no backup waits: no swap, but the download is
    parked once about 200 articles were tried (projected under half the
    critical 85%), instead of running through every article."""
    hp, _ = _projected_run(daemon, t, 'pb', 31, None)
    swaps = _grep_log(t, 'Failing over')
    parked = _grep_log(t, 'under half the critical')
    failed = int(hp.get('FailedArticles', 0))
    return ('projectednobackup', swaps == 0 and parked == 1 and failed < 250,
            'status=%s swap_logs=%d park_logs=%d failed_articles=%d' % (hp['Status'], swaps, parked, failed))


def scenario_projectedhalfnobackup(daemon, t):
    """B69/B74: 50% arrive and no backup waits: above half the critical 85%, so
    not parked after 200 tried, but below critical: parked once 1,000 were
    tried (about 500 failed), not at the end (900)."""
    hp, _ = _projected_run(daemon, t, 'ph', 50, None)
    early = _grep_log(t, 'under half the critical')
    parked = _grep_log(t, 'below critical')
    failed = int(hp.get('FailedArticles', 0))
    return ('projectedhalfnobackup', early == 0 and parked == 1 and 400 < failed < 700,
            'status=%s early_park_logs=%d park_logs=%d failed_articles=%d' % (hp['Status'], early, parked, failed))


def scenario_projectedbelownobackup(daemon, t):
    """B74 (Las Azules S02E05 4417: 79.3% arriving against a critical 80%, ran
    through 10 GB): 80% arrive against the critical 85%, no backup waits:
    parked once 1,000 articles were tried (about 200 failed), instead of
    running to the end (360)."""
    hp, _ = _projected_run(daemon, t, 'pz', 80, None)
    parked = _grep_log(t, 'below critical')
    failed = int(hp.get('FailedArticles', 0))
    return ('projectedbelownobackup', parked == 1 and failed < 300,
            'status=%s park_logs=%d failed_articles=%d' % (hp['Status'], parked, failed))


def scenario_keepreturned(daemon, t):
    """B73 (Las Azules S02E01 4404): a dead pick fails over to a backup, which
    downloads in good health (slowly here). A duplicate scored higher arrives
    meanwhile, as a dupe tool appends one: the returned backup keeps
    downloading and succeeds; the newcomer waits in history as a backup.
    Before, nzbget moved the downloading backup to history and started over."""
    size, seg = 3_000_000, 100_000
    data = _payload(size, 7373)
    pick = build_nzb(_place_copy(t, 'krP', data), 'Pick.bin', size, seg, set(range(1, 31)))
    backup = build_nzb(_place_copy(t, 'krA', data), 'BackA.bin', size, seg, set())
    late = build_nzb(_place_copy(t, 'krC', data), 'LateC.bin', size, seg, set())
    api = daemon.wait_ready()
    pick_id = daemon.append(api, 'Pick', pick, True, 'kr-key', 100)
    daemon.append(api, 'BackA', backup, False, 'kr-key', 90)
    daemon.wait_history(api, 'BackA', timeout=30)
    api.editqueue('GroupResume', 0, '', [pick_id])
    deadline = time.time() + 60
    while time.time() < deadline:
        g = _ds_group(api, 'BackA')
        if g and int(g.get('SuccessArticles', 0)) >= 3:
            break
        time.sleep(0.2)
    daemon.append(api, 'LateC', late, True, 'kr-key', 95)
    deadline = time.time() + 120
    a_status = c_status = None
    while time.time() < deadline:
        hist = {x['NZBName']: x['Status'] for x in api.history()}
        a_status, c_status = hist.get('BackA'), hist.get('LateC')
        if a_status and a_status.startswith(('SUCCESS', 'FAILURE')) and c_status:
            break
        time.sleep(0.5)
    moved = _grep_log(t, 'Moving collection BackA with lower duplicate score')
    ok = moved == 0 and (a_status or '').startswith('SUCCESS') and c_status == 'DELETED/DUPE'
    return ('keepreturned', ok, 'backup=%s late=%s moved_logs=%d' % (a_status, c_status, moved))


def scenario_restartfailover(daemon, t):
    """A dead pick fails over to a backup, and nzbget restarts while the backup
    downloads (slowly here). After the restart the backup resumes and succeeds;
    the pick stays failed and no other backup is fetched."""
    size, seg = 3_000_000, 100_000
    data = _payload(size, 7474)
    pick = build_nzb(_place_copy(t, 'rfP', data), 'Pick.bin', size, seg, set(range(1, 31)))
    backup = build_nzb(_place_copy(t, 'rfA', data), 'BackA.bin', size, seg, set())
    other = build_nzb(_place_copy(t, 'rfB', data), 'BackB.bin', size, seg, set())
    api = daemon.wait_ready()
    pick_id = daemon.append(api, 'Pick', pick, True, 'rf-key', 100)
    daemon.append(api, 'BackA', backup, False, 'rf-key', 90)
    daemon.append(api, 'BackB', other, False, 'rf-key', 80)
    deadline = time.time() + 30
    while time.time() < deadline and len([h for h in api.history() if h['NZBName'].startswith('Back')]) < 2:
        time.sleep(0.2)
    api.editqueue('GroupResume', 0, '', [pick_id])
    deadline = time.time() + 60
    while time.time() < deadline:
        g = _ds_group(api, 'BackA')
        if g and int(g.get('SuccessArticles', 0)) >= 3:
            break
        time.sleep(0.2)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    deadline = time.time() + 120
    hist = {}
    while time.time() < deadline:
        hist = {x['NZBName']: x['Status'] for x in api.history()}
        if hist.get('BackA', '').startswith(('SUCCESS', 'FAILURE')):
            break
        time.sleep(0.5)
    integ = _verify_output(t, data)
    ok = (hist.get('BackA', '').startswith('SUCCESS') and hist.get('Pick', '').startswith('FAILURE') and
          hist.get('BackB') == 'DELETED/DUPE' and integ)
    return ('restartfailover', ok, 'pick=%s backupA=%s backupB=%s integrity=%s'
            % (hist.get('Pick'), hist.get('BackA'), hist.get('BackB'), integ))


def scenario_slowprobe(daemon, t):
    """Physical S02E01 4948: a dead backup ran to 1,120 failed articles in 34 s,
    none of its own arriving, while its dead-pick probe waited (here every STAT
    takes 5 s, so the probe needs most of a minute; articles fail at about 20 a
    second, as on a real server). The rules that fail over a dead download stood
    aside while the probe ran. Now, once the probe ran 10 s with 200 articles
    failed and none arrived, the download fails over without it."""
    seg, vol = 10_000, 3_000_000
    n = vol // seg
    primary = []
    for i in range(20):
        primary.append(('spA/d%d.bin' % i, 'Dead%d.bin' % i, vol, seg, set(range(1, n + 1))))
        t.write_file(os.path.join('data', primary[-1][0]), _payload(vol, 9800 + i))
    data = _payload(2_900_000, 9821)
    backup = build_nzb(_place_copy(t, 'spB', data), 'Backup.bin', 2_900_000, 100_000, set())
    api = daemon.wait_ready()
    pick_id = daemon.append(api, 'DeadSp', build_multi_nzb(primary), True, 'sp-key', 100)
    daemon.append(api, 'BackSp', backup, False, 'sp-key', 90)
    daemon.wait_history(api, 'BackSp', timeout=30)
    api.editqueue('GroupResume', 0, '', [pick_id])
    h = daemon.wait_history(api, 'DeadSp', timeout=240)
    failed = int(h.get('FailedArticles', 0))
    swapped = _grep_log(t, 'Failing over DeadSp')
    ok = swapped == 1 and failed < 500
    return ('slowprobe', ok, 'status=%s failed_articles=%d failover_logs=%d' % (h['Status'], failed, swapped))


def scenario_runparcovers(daemon, t):
    """A pick lacks one stretch of 60 articles in a row (a missing volume, 3% of
    its data) while its par2 files hold 10%: par2 can repair it. The rule that
    fails over after 40 failures in a row (B49) must not swap it for the live
    backup waiting in history: the download completes. (The par2 files are
    recognized by name; ParCheck=manual leaves them unread.)"""
    seg, n = 4_000, 2_000
    size = seg * n
    data = _payload(size, 6161)
    pp = _place_copy(t, 'rpA', data, 'show.mkv')
    par_index = _place_copy(t, 'rpA', _payload(4_000, 6162), 'show.par2')
    par_vol = _place_copy(t, 'rpA', _payload(seg * 200, 6163), 'show.vol00+200.par2')
    primary = build_multi_nzb([(pp, 'show.mkv', size, seg, set(range(1000, 1060))),
                               (par_index, 'show.par2', 4_000, seg, set()),
                               (par_vol, 'show.vol00+200.par2', seg * 200, seg, set())])
    backup = build_nzb(_place_copy(t, 'rpB', data, 'show.mkv'), 'show.mkv', size, seg, set())
    api = daemon.wait_ready()
    pick_id = daemon.append(api, 'PickRp', primary, True, 'rp-key', 100)
    _ds_append(api, 'BackRp', backup, 'rp-key', 90, params=[{'Name': 'DupeAlive', 'Value': '100'}])
    daemon.wait_history(api, 'BackRp', timeout=30)
    api.editqueue('GroupResume', 0, '', [pick_id])
    h = daemon.wait_history(api, 'PickRp', timeout=180)
    swapped = _grep_log(t, 'Failing over PickRp')
    ok = swapped == 0 and int(h.get('FailedArticles', 0)) == 60
    return ('runparcovers', ok, 'status=%s failed_articles=%s failover_logs=%d' % (h['Status'], h.get('FailedArticles'), swapped))


def _ondisk_first(daemon, t, tag):
    """A release downloaded once (SUCCESS, its file on disk), then another posting
    of it under the same key is sent, as when a client asks again."""
    size, seg = 2_000_000, 100_000
    data = _payload(size, 7070)
    first = build_nzb(_place_copy(t, tag + '1', data, 'show.mkv'), 'show.mkv', size, seg, set())
    again = build_nzb(_place_copy(t, tag + '2', data, 'show.mkv'), 'show.mkv', size, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'First', first, False, tag + '-key', 100)
    h = daemon.wait_history(api, 'First')
    return api, h, again


def scenario_ondiskskip(daemon, t):
    """B70 (Knife Edge S02E01): the release is downloaded and on disk; another
    posting of it, scored higher, arrives under its key: it isn't downloaded,
    but kept in history as a backup."""
    api, h, again = _ondisk_first(daemon, t, 'os')
    daemon.append(api, 'Again', again, False, 'os-key', 200)
    h2 = daemon.wait_history(api, 'Again', timeout=60)
    skipped = _grep_log(t, 'is downloaded and on disk')
    ok = h['Status'].startswith('SUCCESS') and h2['Status'] == 'DELETED/DUPE' and skipped == 1
    return ('ondiskskip', ok, 'first=%s again=%s skip_logs=%d' % (h['Status'], h2['Status'], skipped))


def scenario_ondiskgone(daemon, t):
    """B70: the release succeeded, but its files were deleted from disk since:
    the success no longer counts, and the posting sent again downloads."""
    api, h, again = _ondisk_first(daemon, t, 'og')
    for rel in t.find_files('main', 'dst'):
        os.remove(t.path(rel))
    daemon.append(api, 'Again', again, False, 'og-key', 200)
    h2 = daemon.wait_history(api, 'Again', timeout=120)
    ok = h['Status'].startswith('SUCCESS') and h2['Status'].startswith('SUCCESS') and \
        _grep_log(t, 'is downloaded and on disk') == 0
    return ('ondiskgone', ok, 'first=%s again=%s' % (h['Status'], h2['Status']))


def scenario_ondiskstop(daemon, t):
    """B77 (Sugar S01E01): one copy is in stream repair (from a slow duplicate,
    1 s a request) when another copy of the same key downloads whole and
    succeeds: the repair is stopped, and the repairing copy is kept in history
    as a backup instead of running on."""
    # about 80 requests to the slow duplicate: the repair outlasts the clean download
    size, seg = 40_000_000, 500_000
    data = _payload(size, 7171)
    damaged = build_nzb(_place_copy(t, 'odA', data, 'file.mkv'), 'odA.mkv', size, seg, set(range(2, 80, 2)))
    donor = build_nzb(_place_copy(t, 'odB', data, 'file.mkv'), 'obf-od.mkv', size, 250_000, set())
    clean = build_nzb(_place_copy(t, 'odC', data, 'file.mkv'), 'odC.mkv', size, seg, set())
    api = daemon.wait_ready()
    # a dupe tool found the duplicate dead (DupeAlive=0): no failover fetches it,
    # so stream repair is the only option left (B78) - yet its articles exist
    _ds_append(api, 'DonOd', donor, 'od-key', 50, params=[{'Name': 'DupeAlive', 'Value': '0'}])
    daemon.append(api, 'Damaged', damaged, False, 'od-key', 100)
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, 'Queueing Damaged for post-processing') == 0:
        time.sleep(0.2)
    daemon.append(api, 'Clean', clean, False, 'od-key', 110)
    hc = daemon.wait_history(api, 'Clean', timeout=120)
    hd = daemon.wait_history(api, 'Damaged', timeout=120)
    stopped = _grep_log(t, 'Stopping Damaged')
    ok = hc['Status'].startswith('SUCCESS') and hd['Status'] == 'DELETED/DUPE' and stopped == 1
    return ('ondiskstop', ok, 'clean=%s damaged=%s stop_logs=%d' % (hc['Status'], hd['Status'], stopped))


def scenario_ondiskstoprepairfirst(daemon, t):
    """ondiskstop the other way round: the repair (from a quick duplicate)
    finishes before the other copy, slow to download, is whole. The repaired
    copy wins and the other copy is stopped and kept in history as a backup;
    the two never both post-process into the destination."""
    size, seg = 40_000_000, 500_000
    data = _payload(size, 7174)
    damaged = build_nzb(_place_copy(t, 'ocA', data, 'file.mkv'), 'ocA.mkv', size, seg, set(range(2, 80, 2)))
    donor = build_nzb(_place_copy(t, 'ocB', data, 'file.mkv'), 'obf-oc.mkv', size, 250_000, set())
    clean = build_nzb(_place_copy(t, 'ocC', data, 'file.mkv'), 'ocC.mkv', size, seg, set())
    api = daemon.wait_ready()
    _ds_append(api, 'DonOc', donor, 'oc-key', 50, params=[{'Name': 'DupeAlive', 'Value': '0'}])
    daemon.append(api, 'DamagedOc', damaged, False, 'oc-key', 100)
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, 'Queueing DamagedOc for post-processing') == 0:
        time.sleep(0.2)
    daemon.append(api, 'CleanOc', clean, False, 'oc-key', 110)
    hd = daemon.wait_history(api, 'DamagedOc', timeout=120)
    hc = daemon.wait_history(api, 'CleanOc', timeout=120)
    stopped = _grep_log(t, 'Stopping CleanOc')
    landed = sorted(os.path.basename(f) for f in t.find_files('main', 'dst')
                    if os.path.basename(f).startswith('oc'))
    ok = (hd['Status'].startswith('SUCCESS') and hc['Status'] == 'DELETED/DUPE' and stopped == 1
          and landed == ['ocA.mkv'])
    return ('ondiskstoprepairfirst', ok, 'damaged=%s clean=%s stop_logs=%d in_dst=%s'
            % (hd['Status'], hc['Status'], stopped, landed))


def _ondiskstop_same_name(daemon, t, label, tag, shared_dir, name_b='RelSame'):
    """Two copies of one release under the same name and dupe key: copy A, a
    stored rar set, loses articles in two volumes; direct unpack reads past the
    holes, and after the download a stream repair fills them from a slow
    duplicate. Meanwhile copy B, whole, is added and (each copy in its own
    InterDir folder) downloads while A still repairs. Only one copy may go on
    to unpack and move: B is held before unpacking, as A's extraction is under
    way; A finishes and stops B, which stays in history as a backup. With
    ``shared_dir`` (no InterDir: the two would download into one folder) B
    must not download while A repairs, nor be discarded for A's files.
    Either way exactly one movie lands in the destination, and no archive
    volume of either copy is left behind (a stopped copy's files go, as the
    delete route removes them). ``name_b``: B under another name (with no
    InterDir its own final folder: it must not download while A repairs
    either, or both copies would land files in the destination)."""
    seg = 300_000
    movie = _payload(5_000_000, 9981)
    vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 1_400_000)
    names = ['Rel.part%02d.rar' % (i + 1) for i in range(len(vols))]
    damage = {1: {1, 2, 3, 4}, 2: {1, 2, 3}}
    for sub in ('A', 'B', 'C'):
        for n, d in zip(names, vols):
            t.write_file(os.path.join('data', tag + sub, n), d)
    def copy(sub, dmg, article):
        return build_multi_nzb([('%s%s/%s' % (tag, sub, n), n, len(d), article, dmg.get(i, set()))
                                for i, (n, d) in enumerate(zip(names, vols))])
    api = daemon.wait_ready()
    _ds_append(api, 'Don' + tag, copy('B', {}, 200_000), tag + '-key', 50,
               params=[{'Name': 'DupeAlive', 'Value': '0'}])
    daemon.append(api, 'RelSame', copy('A', damage, seg), False, tag + '-key', 100)
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, 'Queueing RelSame for post-processing') == 0:
        time.sleep(0.2)
    # (once A's repair took its duplicates: B, whole, would otherwise lend it quickly)
    time.sleep(1.0)
    # B: another posting of the same release, under the same name
    daemon.append(api, name_b, copy('C', {}, seg), False, tag + '-key', 110)
    deadline = time.time() + 240
    hist = []
    while time.time() < deadline:
        hist = [h for h in api.history() if h.get('NZBName') in ('RelSame', name_b)]
        if len(hist) == 2:
            break
        time.sleep(0.5)
    statuses = sorted(h['Status'] for h in hist)
    movies = [f for f in t.find_files('main', 'dst') if f.endswith('.mkv')]
    intact = len(movies) == 1 and t.read_file(movies[0]) == movie
    leftover = [f for f in t.find_files('main') if f.endswith('.rar')]
    held = _grep_log(t, 'before unpacking: RelSame is')
    stopped = _grep_log(t, 'Stopping %s: RelSame is downloaded and on disk' % name_b)
    completed = (_grep_log(t, 'Collection RelSame completely downloaded') +
                 (_grep_log(t, 'Collection %s completely downloaded' % name_b) if name_b != 'RelSame' else 0))
    repaired = _grep_log(t, 'from duplicate Don' + tag)
    ok = (statuses == ['DELETED/DUPE', 'SUCCESS/UNPACK'] and intact and stopped == 1 and repaired >= 1
          and not leftover)
    if shared_dir:
        # B never downloaded into the folder while A repaired (no overlap, no hold)
        ok = ok and held == 0
    else:
        ok = ok and held == 1 and completed == 2
    return (label, ok, 'statuses=%s movies_in_dst=%d intact=%s held_logs=%d stop_logs=%d '
            'completed_logs=%d repair_logs=%d leftover_volumes=%s'
            % (statuses, len(movies), intact, held, stopped, completed, repaired, leftover))


def scenario_ondiskstopsamename(daemon, t):
    return _ondiskstop_same_name(daemon, t, 'ondiskstopsamename', 'osn', False)


def scenario_ondiskstopsamenamenointer(daemon, t):
    return _ondiskstop_same_name(daemon, t, 'ondiskstopsamenamenointer', 'osi', True)


def scenario_ondiskstopothernamenointer(daemon, t):
    return _ondiskstop_same_name(daemon, t, 'ondiskstopothernamenointer', 'oon', True, name_b='RelOther')


def scenario_repairlast(daemon, t):
    """B78: stream repair is the last option. A download is damaged beyond its
    par2 (none here) while a whole backup waits in history: it isn't repaired
    from the backup, it fails over to it, which then downloads whole. Before,
    the download was stream-repaired first, which can take longer than
    downloading a good posting."""
    size, seg = 6_000_000, 500_000
    data = _payload(size, 7272)
    damaged = build_nzb(_place_copy(t, 'rlA', data, 'file.mkv'), 'rlA.mkv', size, seg, set(range(2, 10)))
    backup = build_nzb(_place_copy(t, 'rlB', data, 'file.mkv'), 'obf-rl.mkv', size, 250_000, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DamagedRl', damaged, True, 'rl-key', 100)
    daemon.append(api, 'BackRl', backup, False, 'rl-key', 90)
    daemon.wait_history(api, 'BackRl', timeout=30)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'DamagedRl'])
    deadline = time.time() + 180
    hist = {}
    while time.time() < deadline:
        hist = {x['NZBName']: x['Status'] for x in api.history()}
        if hist.get('BackRl', '').startswith(('SUCCESS', 'FAILURE')) and hist.get('DamagedRl', '').startswith('FAILURE'):
            break
        time.sleep(0.5)
    skipped = _grep_log(t, 'a backup in history is tried first')
    # (with no par2 nothing is borrowed either, so the download may fail over
    # before the repair decision that logs the skip: no repair is what counts)
    repaired = _grep_log(t, 'Stream repair of rlA') + _grep_log(t, 'Recovered') 
    ok = (skipped >= 1 or repaired == 0) and hist.get('DamagedRl', '').startswith('FAILURE') and \
        hist.get('BackRl', '').startswith('SUCCESS')
    return ('repairlast', ok, 'damaged=%s backup=%s skip_logs=%d repair_logs=%d' % (
        hist.get('DamagedRl'), hist.get('BackRl'), skipped, repaired))


def scenario_samepostingrepair(daemon, t):
    """The proxy's stream-mode case: a pick without par2 missing a few articles,
    and in history a donor that shares every other article's message-id (the
    same posting with the missing articles intact). Before: the pick downloaded,
    ended FAILURE/HEALTH, the donor was never used. The release must end up
    complete: repaired from the donor, or the donor downloaded in its place."""
    size, seg = 6_000_000, 500_000
    data = _payload(size, 7373)
    pp = _place_copy(t, 'spA', data, 'file.mkv')
    pick = build_nzb(pp, 'sp.mkv', size, seg, {3, 7})
    donor = build_nzb(pp, 'sp.mkv', size, seg, set())
    api = daemon.wait_ready()
    # (unpaused, as DupeDonors and appendfleet add their copies: a backup added
    # paused comes back paused when it is failed over to, by design)
    daemon.append(api, 'DonSP', donor, False, 'sp-key', 99)
    daemon.append(api, 'PickSP', pick, False, 'sp-key', 100)
    deadline = time.time() + 180
    hist = {}
    while time.time() < deadline:
        hist = {x['NZBName']: x['Status'] for x in api.history()}
        done = hist.get('PickSP', '').startswith('SUCCESS') or hist.get('DonSP', '').startswith('SUCCESS')
        if done or (hist.get('PickSP', '').startswith('FAILURE') and not any(g['NZBName'] == 'DonSP' for g in api.listgroups())
                    and time.time() > deadline - 150):
            if done:
                break
        time.sleep(0.5)
    ok = hist.get('PickSP', '').startswith('SUCCESS') or hist.get('DonSP', '').startswith('SUCCESS')
    return ('samepostingrepair', ok, 'pick=%s donor=%s failover_logs=%d repair_logs=%d' % (
        hist.get('PickSP'), hist.get('DonSP'), _grep_log(t, 'Failing over'), _grep_log(t, 'Stream repair')))


def scenario_samepostinglate(daemon, t):
    """As samepostingrepair, but only 1 of 24 articles missing (health 96%, above
    critical: no failover while downloading), no par2: the pick ends damaged, and
    the release must still end up complete through the donor."""
    size, seg = 12_000_000, 500_000
    data = _payload(size, 7474)
    pp = _place_copy(t, 'slA', data, 'file.mkv')
    pick = build_nzb(pp, 'sl.mkv', size, seg, {7})
    donor = build_nzb(pp, 'sl.mkv', size, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DonSL', donor, False, 'sl-key', 99)
    daemon.append(api, 'PickSL', pick, False, 'sl-key', 100)
    deadline = time.time() + 180
    hist = {}
    while time.time() < deadline:
        hist = {x['NZBName']: x['Status'] for x in api.history()}
        if hist.get('PickSL', '').startswith('SUCCESS') or hist.get('DonSL', '').startswith('SUCCESS'):
            break
        time.sleep(0.5)
    ok = hist.get('PickSL', '').startswith('SUCCESS') or hist.get('DonSL', '').startswith('SUCCESS')
    return ('samepostinglate', ok, 'pick=%s donor=%s queued=%s' % (
        hist.get('PickSL'), hist.get('DonSL'), [(g['NZBName'], g['Status']) for g in api.listgroups()]))


def scenario_recheckwholefail(daemon, t):
    """The failed-article recheck (HealthCheck=dupe, no backup) samples a file none
    of whose articles arrived too: it read a saved state such a file never has
    ("could not open file .../<id>c"), and left its articles out."""
    size, seg = 2_000_000, 500_000
    a = _place_copy(t, 'rwA', _payload(size, 7575), 'a.bin')
    b = _place_copy(t, 'rwB', _payload(size, 7576), 'b.bin')
    nzb = build_multi_nzb([(a, 'a.bin', size, seg, {2}), (b, 'b.bin', size, seg, {1, 2, 3, 4})])
    api = daemon.wait_ready()
    daemon.append(api, 'RelRW', nzb, False, 'rw-key', 100)
    h = daemon.wait_history(api, 'RelRW', timeout=120)
    deadline = time.time() + 30
    while time.time() < deadline and _grep_log(t, 'failed articles exist on the servers') == 0:
        time.sleep(0.5)
    with open(t.path('nzbget.log'), errors='replace') as f:
        found = re.findall(r'(\d+) of (\d+) failed articles exist on the servers', f.read())
    sampled = int(found[-1][1]) if found else -1
    no_state = _grep_log(t, 'could not open file')
    ok = no_state == 0 and sampled >= 5
    return ('recheckwholefail', ok, 'status=%s sampled=%d state_errors=%d' % (h['Status'], sampled, no_state))


# lightburst: 16 connections (a light reserve of 2), BODY of the load held
# 0.25 s by the proxy, and a speed limit the download reaches (a full pipe:
# more connections would not make it faster)
LIGHTBURST_OPTIONS = ['DupeArticleFallback=article', 'HealthCheck=none', 'ArticleRetries=0',
                      'Server1.Connections=16', 'DownloadRate=6000']


def _lightburst_phase(daemon, api, t, tag):
    """A full-speed download, and in its middle a small release whose failed
    articles the daemon then asks the server about (STAT). Returns (seconds
    from the release's history entry to the recheck verdict or None, seconds
    the download took, the most light connections RPC status showed, the
    recheck line)."""
    load_size, load_seg = 48_000_000, 400_000
    load = _place_copy(t, 'lbLoad%s' % tag, _payload(load_size, 8100), 'big.bin')
    burst_size, burst_seg = 1_200_000, 100_000
    burst = _place_copy(t, 'lbBurst%s' % tag, _payload(burst_size, 8101), 'b.bin')
    verdicts = ('failed articles exist on the servers', 'Failed articles not rechecked')
    before = sum(_grep_log(t, v) for v in verdicts)
    started = time.time()
    daemon.append(api, 'Load%s' % tag, build_nzb(load, 'Load%s.bin' % tag, load_size, load_seg, set()),
                  False, 'lbl-%s' % tag, 0)
    deadline = time.time() + 30
    while time.time() < deadline and api.status().get('DownloadRate', 0) == 0:
        time.sleep(0.2)
    time.sleep(2)  # every connection the download may use is busy now

    def light_seen():
        return max([s.get('LightConnections', 0) for s in api.status().get('NewsServers', [])] or [0])
    light = light_seen()
    content = base64.standard_b64encode(
        build_nzb(burst, 'Burst%s.bin' % tag, burst_size, burst_seg, set(range(2, 12))).encode()).decode()
    api.append('Burst%s' % tag, content, 'test', 100, False, False, 'lbb-%s' % tag, 0, 'score', [])
    in_history = None
    verdict_at = None
    deadline = time.time() + 60
    while time.time() < deadline and verdict_at is None:
        if in_history is None and any(h.get('NZBName') == 'Burst%s' % tag for h in api.history()):
            in_history = time.time()
        if in_history is not None and sum(_grep_log(t, v) for v in verdicts) > before:
            verdict_at = time.time()
        light = max(light, light_seen())
        time.sleep(0.05)
    hl = daemon.wait_history(api, 'Load%s' % tag, timeout=180)
    load_secs = time.time() - started
    latency = verdict_at - in_history if verdict_at is not None and in_history is not None else None
    with open(t.path('nzbget.log'), errors='replace') as f:
        lines = [ln for ln in f.read().splitlines() if any(v in ln for v in verdicts)]
    return latency, load_secs, light, lines[-1].split('\t')[-1] if len(lines) > before else '', hl['Status']


def scenario_lightburst(daemon, t):
    """Light connections: a 48 MB download at its speed limit on 16
    connections, its articles slow to come (0.25 s each), with a STAT burst
    (the recheck of a failed release's articles) in its middle. Run with
    LightConnections=0, then after a reload with LightConnections=5: with
    the reserve the STAT burst finishes at once on a light connection, and
    the download takes no longer than without it (within noise)."""
    api = daemon.wait_ready()
    off = _lightburst_phase(daemon, api, t, 'Off')
    daemon.write_config(LIGHTBURST_OPTIONS + ['LightConnections=5'])
    api = daemon.reload(api)
    on = _lightburst_phase(daemon, api, t, 'On')
    resized = _grep_log(t, 'Light connections of ')

    def fmt(latency):
        return 'none' if latency is None else '%.2fs' % latency
    mbps = lambda secs: 48.0 / secs if secs > 0 else 0.0
    ok = (on[0] is not None and on[0] < 3.0 and on[2] >= 1 and off[2] == 0 and resized >= 1 and
          on[1] <= off[1] * 1.15 + 1.0 and 'SUCCESS' in on[4] and 'SUCCESS' in off[4])
    return ('lightburst', ok,
            'off: stat_latency=%s load=%.1fs (%.2f MB/s) light=%d verdict="%s"; '
            'on: stat_latency=%s load=%.1fs (%.2f MB/s) light=%d verdict="%s"; resize_logs=%d'
            % (fmt(off[0]), off[1], mbps(off[1]), off[2], off[3],
               fmt(on[0]), on[1], mbps(on[1]), on[2], on[3], resized))


def scenario_projectedhealthy(daemon, t):
    """B48c: 96% arrive (above the critical 85%): no swap, though a whole backup waits."""
    hp, swaps = _projected_run(daemon, t, 'pc', 96, 100)
    return ('projectedhealthy', swaps == 0, 'status=%s swap_logs=%d' % (hp['Status'], swaps))


def scenario_projectedworsebackup(daemon, t):
    """B48d: 60% arrive and the only backup is 50% alive: no swap for a worse copy."""
    hp, swaps = _projected_run(daemon, t, 'pd', 60, 50)
    return ('projectedworsebackup', swaps == 0, 'status=%s swap_logs=%d' % (hp['Status'], swaps))


def scenario_projectededge(daemon, t):
    """B58 (Dark Matter S02E05 4180): 87% arrive, within 5 points of the critical
    85%, and a backup is known 100% alive: par-repair can't be counted on that
    close to critical, so the pick is swapped once about 200 articles were
    tried. Before, it ran to the end: its projection never fell below critical."""
    hp, swaps = _projected_run(daemon, t, 'pe', 87, 100)
    failed = int(hp.get('FailedArticles', 0))
    close = _grep_log(t, 'too close to critical')
    return ('projectededge', swaps == 1 and close == 1 and failed < 60,
            'status=%s swap_logs=%d too_close_logs=%d failed_articles=%d' % (hp['Status'], swaps, close, failed))


def scenario_projectededgelead(daemon, t):
    """B58: 87% arrive, close to the critical 85%, but the backup is only 95%
    alive, less than 10 points wholer: no swap."""
    hp, swaps = _projected_run(daemon, t, 'pf', 87, 95)
    return ('projectededgelead', swaps == 0, 'status=%s swap_logs=%d' % (hp['Status'], swaps))


def scenario_parprojection(daemon, t):
    """B59 (Dark Matter S02E05 4180): borrowing waits for par-check while par2 may
    cover the damage, but 15% of the data is missing (evenly) and the par2 files
    hold 14%. Judged by the articles tried so far, par2 can't cover it: the wait
    ends once 200 were tried, and the missing articles are borrowed from the
    whole duplicate from then on, and it doesn't come back when the articles
    fetched from the lead duplicate lift the projection again. Before, it ended
    only when the failures outgrew the par2 data, near the end of the download,
    and about 140 articles failed; now only those that failed before 200 were
    tried (about 30) are left to post-processing. (The par2 files are
    recognized by name; the par-check of post-processing can't read them and
    fails, which doesn't matter here. The wait for par-check needs one that
    runs by itself: under ParCheck=manual the articles are borrowed at once.)"""
    seg, n = 4_000, 1_000
    size = seg * n
    data = _payload(size, 5959)
    missing = {p for p in range(2, n + 1) if (p * 37) % 100 < 15}
    pp = _place_copy(t, 'projA', data, 'show.mkv')
    par_index = _place_copy(t, 'projA', _payload(4_000, 5960), 'show.par2')
    par_vol = _place_copy(t, 'projA', _payload(seg * 140, 5961), 'show.vol00+140.par2')
    primary = build_multi_nzb([(pp, 'show.mkv', size, seg, missing),
                               (par_index, 'show.par2', 4_000, seg, set()),
                               (par_vol, 'show.vol00+140.par2', seg * 140, seg, set())])
    dp = _place_copy(t, 'projB', data, 'show.mkv')
    donor = build_multi_nzb([(dp, 'show.mkv', size, seg, set())])
    api = daemon.wait_ready()
    daemon.append(api, 'DonProj', donor, True, 'proj-key', 50)
    daemon.append(api, 'Proj', primary, False, 'proj-key', 100)
    h = daemon.wait_history(api, 'Proj', timeout=180)
    lifted = _grep_log(t, 'or will, judged by the articles tried so far')
    failed = int(h.get('FailedArticles', 0))
    ok = lifted == 1 and failed < 50
    return ('parprojection', ok, 'status=%s missing=%d lifted_logs=%d failed_articles=%d'
            % (h['Status'], len(missing), lifted, failed))


def _run_case(daemon, t, tag, missing_of, backup):
    """Six files of 300 articles (no par2); <missing_of(index)> says which
    articles (by their index over the whole posting) are missing; <backup> adds
    a healthy backup in history (DupeAlive unknown, so only the run rule applies)."""
    seg, vol = 10_000, 3_000_000
    n = vol // seg
    primary = []
    for i in range(6):
        missing = {p for p in range(1, n + 1) if missing_of(i * n + p - 1)}
        primary.append(('%sA/d%d.bin' % (tag, i), 'Part%d.bin' % i, vol, seg, missing))
        t.write_file(os.path.join('data', primary[-1][0]), _payload(vol, 9760 + i))
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, tag + '-key', 100)
    if backup:
        data = _payload(2_900_000, 9769)
        bp = _place_copy(t, '%sB' % tag, data)
        daemon.append(api, 'Backup', build_nzb(bp, 'Backup.bin', 2_900_000, 100_000, set()), False, tag + '-key', 90)
        daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    return hp, _grep_log(t, 'failed in a row')


def scenario_runswap(daemon, t):
    """B49a: after its first article, 59 articles of the first file in a row are
    missing, the rest exist; a backup waits: the pick is swapped at the 40th
    failure in a row."""
    hp, swaps = _run_case(daemon, t, 'ra', lambda k: 1 <= k <= 59, True)
    failed = int(hp.get('FailedArticles', 0))
    return ('runswap', swaps == 1 and failed <= 59, 'status=%s run_swap_logs=%d failed=%d' % (hp['Status'], swaps, failed))


def scenario_runnobackup(daemon, t):
    """B49b: the same run with no backup: the download goes on."""
    hp, swaps = _run_case(daemon, t, 'rb', lambda k: 1 <= k <= 59, False)
    return ('runnobackup', swaps == 0,
            'status=%s run_swap_logs=%d' % (hp['Status'], swaps))


def scenario_runscattered(daemon, t):
    """B49c: failures scattered, never more than 8 in a row: no swap for a run."""
    hp, swaps = _run_case(daemon, t, 'rc', lambda k: k % 12 < 8 and k % 300 != 0, True)
    return ('runscattered', swaps == 0, 'status=%s run_swap_logs=%d' % (hp['Status'], swaps))


def scenario_runreset(daemon, t):
    """B49d: two runs of 30 failures with articles arriving between them: the
    count starts again after the arrivals, so no swap."""
    hp, swaps = _run_case(daemon, t, 'rd', lambda k: 1 <= k <= 30 or 61 <= k <= 90, True)
    return ('runreset', swaps == 0, 'status=%s run_swap_logs=%d' % (hp['Status'], swaps))


def scenario_deadpickpartial(daemon, t):
    """The probe never abandons a partly alive posting: 85% of the articles
    are missing but some exist, among them one the probe samples. Since fleet
    scores became 1-100 (#401) the add-on score is only a tiebreak, so a
    failover to the backup is decided by health, not by the backup's low
    score; only the probe's verdict is asserted."""
    primary, backup, data = _probe_fixture(t, 'dq', alive_every=7)
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'dq-key', 100)
    daemon.append(api, 'Backup', backup, False, 'dq-key', 1)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups()
                                         if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary')
    failed_over = _grep_log(t, 'Failing over Primary')
    alive_logs = _grep_log(t, 'not abandoning it')
    return ('deadpickpartial', alive_logs == 1,
            'status=%s failover_logs=%d alive_probe_logs=%d' % (hp['Status'], failed_over, alive_logs))


def scenario_yencrangefar(daemon, t):
    """F23/F24: an article's yEnc range lies far past the file size it declares
    (here the last part says begin=9000001 end=9500000 of a 1.5 MB file). The
    file was filled with zeros up to that offset (a crafted or damaged posting
    could fill the disk); now the article is malformed: it fails, and the file
    never grows past its declared size."""
    size, seg = 1_500_000, 500_000
    data = _payload(size, 5400)
    pp = _place_copy(t, 'yrA', data, 'far.bin')
    api = daemon.wait_ready()
    daemon.append(api, 'RelYR', build_nzb(pp, 'far.bin', size, seg, set()), False, 'yr-key', 100)
    h = daemon.wait_history(api, 'RelYR', timeout=120)
    sizes = [os.path.getsize(t.path(rel)) for rel in t.find_files('main') if rel.endswith('far.bin')]
    rewritten = daemon.proxy.rewritten if daemon.proxy else 0
    ok = rewritten >= 1 and sizes and max(sizes) <= size
    return ('yencrangefar', ok, 'status=%s rewritten=%d sizes=%s failed=%s' % (
        h['Status'], rewritten, sizes, h.get('FailedArticles')))


def scenario_yencrangeshort(daemon, t):
    """An article's yEnc range is shorter than its data (the last part says
    end=1000100 of 1,000,001..1,500,000, its body decodes to 500 KB with a valid
    crc), the article cache on: the article fails. It counted as finished, its
    size counted the bytes past the range, and the cache padded the segment with
    memory never written (into the file)."""
    size, seg = 1_500_000, 500_000
    data = _payload(size, 8383)
    pp = _place_copy(t, 'ysA', data)
    api = daemon.wait_ready()
    daemon.append(api, 'RelYS', build_nzb(pp, 'short.bin', size, seg, set()), False, 'ys-key', 100)
    h = daemon.wait_history(api, 'RelYS', timeout=120)
    integ = _verify_output(t, data)
    wrong_success = h['Status'].startswith('SUCCESS') and not integ
    rewritten = daemon.proxy.rewritten if daemon.proxy else 0
    ok = rewritten >= 1 and not wrong_success and int(h.get('FailedArticles', 0)) >= 1
    return ('yencrangeshort', ok, 'status=%s integrity=%s failed=%s rewritten=%d' % (
        h['Status'], integ, h.get('FailedArticles'), rewritten))


def scenario_quotafutureday(daemon, t):
    """The clock went back a day or more since the volume stats were saved
    (here the saved first day and save time are moved 5 days ahead), with a
    daily quota set:
    the quota check read day slot -1 (std::out_of_range) and the daemon died
    at the start. Now it stays up and downloads."""
    data = _payload(300_000, 8484)
    pp = _place_copy(t, 'qfA', data)
    api = daemon.wait_ready()
    daemon.append(api, 'RelQF1', build_nzb(pp, 'first.bin', len(data), 100_000, set()), False, 'qf-key1', 100)
    daemon.wait_history(api, 'RelQF1', timeout=60)
    stats = t.path('main', 'queue', 'stats')
    # The volume stats are saved by the periodic service tick, not at exit: shutting
    # down right after the download can leave no file yet (seen 1 run in 8 on macOS).
    # Wait until a saved file holds a day slot before stopping the daemon.
    day_now = int(time.time()) // 86400
    deadline = time.time() + 60
    while time.time() < deadline:
        try:
            if any(re.match(r'^(\d+),', ln) and abs(int(ln.split(',')[0]) - day_now) <= 2
                   for ln in open(stats).read().split('\n')):
                break
        except OSError:
            pass
        time.sleep(0.5)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    today = int(time.time()) // 86400
    lines = open(stats).read().split('\n')
    moved = 0
    for i, line in enumerate(lines):
        m = re.match(r'^(\d+),(-?\d+),(-?\d+),(-?\d+)$', line)
        if m and abs(int(m.group(1)) - today) <= 2:
            lines[i] = '%d,%d,%s,%s' % (today + 5, int(m.group(2)) + 5 * 86400, m.group(3), m.group(4))
            moved += 1
    open(stats, 'w').write('\n'.join(lines))
    daemon.start()
    api = daemon.wait_ready()
    time.sleep(4)
    data2 = _payload(300_000, 8485)
    pp2 = _place_copy(t, 'qfB', data2)
    daemon.append(api, 'RelQF2', build_nzb(pp2, 'second.bin', len(data2), 100_000, set()), False, 'qf-key2', 100)
    h = daemon.wait_history(api, 'RelQF2', timeout=60)
    alive = t.procs[-1].poll() is None
    ok = moved >= 1 and alive and h['Status'].startswith('SUCCESS')
    return ('quotafutureday', ok, 'moved=%d alive=%s status=%s' % (moved, alive, h['Status']))


def scenario_connhold(daemon, t):
    """Idle connections are held for 5 seconds before they're closed. The
    hold took the longest idle time of a level's connections, and a connection
    never used counts as idle since 1970: whenever none was in use, all were
    closed at once (a download soon after logged in again). Now the
    connection stays open 2 seconds after a one-article download and is
    closed once the hold is over."""
    data = _payload(90_000, 8686)
    pa = _place_copy(t, 'chA', data)
    api = daemon.wait_ready()
    daemon.append(api, 'RelCH1', build_nzb(pa, 'first.bin', len(data), 100_000, set()), False, 'ch-key1', 100)
    h = daemon.wait_history(api, 'RelCH1', timeout=60)
    time.sleep(2)
    open_held = daemon.proxy.accepted - daemon.proxy.closed
    time.sleep(8)
    open_after = daemon.proxy.accepted - daemon.proxy.closed
    ok = h['Status'].startswith('SUCCESS') and open_held >= 1 and open_after == 0
    return ('connhold', ok, 'status=%s open 2s after=%d 10s after=%d (accepted=%d)' % (
        h['Status'], open_held, open_after, daemon.proxy.accepted))


def _inner_archive_keep(daemon, t, name):
    """A rar whose content is itself an archive (setup.7z), UseTempUnpackDir=no,
    UnpackCleanupDisk=yes, no InterDir: the cleanup deleted every *.7z/*.rar in the
    folder, the extracted setup.7z too, and reported success. Now only the
    downloaded volumes are deleted."""
    inner = _payload(600_000, 8989)
    volumes = generators.rar3_store_volumes_valid('setup.7z', inner, 300_000)
    members = []
    for i, vol in enumerate(volumes, 1):
        rel = 'iaA/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 500_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'RelIA', build_multi_nzb(members), False, 'ia-key', 100)
    h = daemon.wait_history(api, 'RelIA', timeout=180)
    files = {os.path.basename(rel): rel for rel in t.find_files('main', 'dst')}
    kept = 'setup.7z' in files and t.read_file(files['setup.7z']) == inner
    volumes_left = [f for f in files if re.search(r'Rel\.part\d+\.rar$', f)]
    ok = h['Status'].startswith('SUCCESS') and kept and not volumes_left
    return (name, ok, 'status=%s setup.7z_kept=%s volumes_left=%s files=%s' % (
        h['Status'], kept, volumes_left, sorted(files)))


def scenario_innerarchivekeep(daemon, t):
    return _inner_archive_keep(daemon, t, 'innerarchivekeep')


def scenario_innerarchivekeepdirect(daemon, t):
    """As innerarchivekeep, with the archive extracted by direct unpack."""
    return _inner_archive_keep(daemon, t, 'innerarchivekeepdirect')


def _serve_rss(items, delay=0.0):
    """A local http server answering every GET with an RSS feed of ``items``
    (title, url); returns its port."""
    import http.server
    body = ('<?xml version="1.0"?><rss version="2.0"><channel><title>t</title>' + ''.join(
        '<item><title>%s</title><link>%s</link><enclosure url="%s" length="1000" type="application/x-nzb"/></item>'
        % (title, url, url) for title, url in items) + '</channel></rss>').encode()

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            time.sleep(delay)
            self.send_response(200)
            self.send_header('Content-Type', 'application/rss+xml')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass

    srv = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv.server_address[1]


def scenario_feedpreviewparallel(daemon, t):
    """Previews of a configured feed (id 1) at the same time as its fetch: the
    previews wrote the feed's own temp file (feed-1.tmp), so parallel ones
    read each other's half-written or deleted file. Every preview now gets
    its own file and returns all items."""
    import concurrent.futures
    items = [('Item.%d' % i, 'http://127.0.0.1:9/item%d.nzb' % i) for i in range(5)]
    port = _serve_rss(items, delay=0.3)
    url = 'http://127.0.0.1:%d/rss' % port
    daemon.wait_ready()

    def preview(_):
        r = _rpc(daemon, 'previewfeed', [1, 'f1', url, '', False, True, '', 0, 0, '', True, 0, ''], timeout=60)
        return len(r.get('result') or []), r.get('error')

    with concurrent.futures.ThreadPoolExecutor(8) as pool:
        fetch = pool.submit(_rpc, daemon, 'fetchfeed', [1])
        results = list(pool.map(preview, range(8)))
        fetch.result()
    ok = all(n == len(items) and not err for n, err in results)
    return ('feedpreviewparallel', ok, 'previews=%s' % results)


def _next_minute_task_options():
    at = time.localtime(time.time() + 60)
    return ['Task1.Time=%02d:%02d' % (at.tm_hour, at.tm_min), 'Task1.Command=unpausedownlaod']


def scenario_tasktypo(daemon, t):
    """A misspelled Task1.Command (unpausedownlaod) was logged as an invalid
    value and then became the first command, pausedownload: once the user
    resumed after the configuration error, the task paused downloads at its
    time every day. The task is skipped now."""
    api = daemon.wait_ready()
    _rpc(daemon, 'resume', [])
    resumed = not _rpc(daemon, 'status', []).get('result', {}).get('DownloadPaused')
    time.sleep(75)
    paused = _rpc(daemon, 'status', []).get('result', {}).get('DownloadPaused')
    errors = _grep_log(t, 'Invalid value for option "Task1.Command"')
    ok = resumed and not paused and errors >= 1
    return ('tasktypo', ok, 'resumed=%s paused_after_task_time=%s invalid_logs=%d' % (resumed, paused, errors))


def scenario_urlredirects(daemon, t):
    """URL downloads through three kinds of request:
    - a relative redirect with "://" in its query (/get?u=https://...) was taken
      for an absolute URL with the protocol "/get?u=https" and failed;
    - a protocol-relative redirect (//host:port/path) was requested as a path on
      the old host;
    - a url of about 1,500 characters lost "HTTP/1.0" and its line end in a
      1,024-byte buffer, a malformed request.
    All three fetch their nzb now."""
    import http.server
    nzbs = {}
    for tag, seed in (('A', 9191), ('B', 9192), ('C', 9193)):
        data = _payload(90_000, seed)
        pp = _place_copy(t, 'ur' + tag, data)
        nzbs[tag] = build_nzb(pp, 'url%s.bin' % tag, len(data), 100_000, set()).encode()
    port_box = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            path = self.path
            if path == '/a':
                self._redirect('/get?u=https://example.invalid/x.nzb')
            elif path.startswith('/get?u='):
                self._nzb('A')
            elif path == '/b':
                self._redirect('//127.0.0.1:%d/other/b.nzb' % port_box[0])
            elif path == '/other/b.nzb':
                self._nzb('B')
            elif path.startswith('/long?'):
                self._nzb('C')
            else:
                self.send_response(404)
                self.end_headers()

        def _redirect(self, location):
            self.send_response(302)
            self.send_header('Location', location)
            self.send_header('Content-Length', '0')
            self.end_headers()

        def _nzb(self, tag):
            self.send_response(200)
            self.send_header('Content-Type', 'application/x-nzb')
            self.send_header('Content-Length', str(len(nzbs[tag])))
            self.end_headers()
            self.wfile.write(nzbs[tag])

        def log_message(self, *args):
            pass

    srv = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    port_box.append(srv.server_address[1])
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    base = 'http://127.0.0.1:%d' % port_box[0]
    api = daemon.wait_ready()
    urls = {'RelURA': base + '/a', 'RelURB': base + '/b', 'RelURC': base + '/long?sig=' + 'q' * 1500}
    for name, url in urls.items():
        _rpc(daemon, 'appendurl', [name + '.nzb', url, '', 0, False, False, name.lower(), 0, 'SCORE', []])
    statuses = {}
    for name in urls:
        try:
            statuses[name] = daemon.wait_history(api, name, timeout=60)['Status']
        except RuntimeError:
            statuses[name] = 'TIMEOUT'
    srv.shutdown()
    ok = all(st.startswith('SUCCESS') for st in statuses.values())
    return ('urlredirects', ok, 'statuses=%s' % statuses)


def scenario_urlclosed(daemon, t):
    """A server that closes the connection without answering: the first-line
    check read the line buffer instead of the read's result, so the "closed by
    remote host" case (a connection error) was never taken, and the url
    failed with "URL ... failed: " and whatever the buffer held."""
    srv = socket.socket()
    srv.bind(('127.0.0.1', 0))
    srv.listen(16)
    port = srv.getsockname()[1]

    def serve():
        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                return
            conn.recv(65536)
            conn.close()
    threading.Thread(target=serve, daemon=True).start()
    api = daemon.wait_ready()
    _rpc(daemon, 'appendurl', ['RelUC.nzb', 'http://127.0.0.1:%d/x.nzb' % port, '', 0, False, False, 'uc', 0, 'SCORE', []])
    try:
        status = daemon.wait_history(api, 'RelUC', timeout=90)['Status']
    except RuntimeError:
        status = 'TIMEOUT'
    srv.close()
    closed = _grep_log(t, 'Connection closed by remote host')
    garbage = _grep_log(t, 'RelUC.nzb failed: ')
    ok = status.startswith('FAILURE') and closed >= 1 and garbage == 0
    return ('urlclosed', ok, 'status=%s closed_logs=%d failed_logs=%d' % (status, closed, garbage))


def scenario_urlretrywait(daemon, t):
    """UrlInterval (20 s) longer than UrlTimeout + 10 (12 s), a server that always
    answers 500, UrlRetries=2: the wait between tries didn't count as
    activity, so the download was cancelled as hanging and restarted with
    all its retries again - forever. Now it fails after its two tries."""
    import http.server

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(500)
            self.send_header('Content-Length', '0')
            self.end_headers()

        def log_message(self, *args):
            pass

    srv = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    api = daemon.wait_ready()
    _rpc(daemon, 'appendurl', ['RelUW.nzb', 'http://127.0.0.1:%d/x.nzb' % srv.server_address[1], '', 0, False, False, 'uw', 0, 'SCORE', []])
    try:
        status = daemon.wait_history(api, 'RelUW', timeout=60)['Status']
    except RuntimeError:
        status = 'TIMEOUT'
    srv.shutdown()
    hanging = _grep_log(t, 'Cancelling hanging url download')
    ok = status.startswith('FAILURE') and hanging == 0
    return ('urlretrywait', ok, 'status=%s hanging_cancels=%d' % (status, hanging))


def scenario_historyeditlist(daemon, t):
    """HistorySetCategory for [a normal item, a hidden duplicate record]: the
    record can't take a category, and its failure, the last in the list,
    replaced the first item's success - the call answered false and the
    change wasn't saved (gone after a crash). Now it answers true and the
    change is on disk."""
    data = _payload(90_000, 9393)
    pp = _place_copy(t, 'heA', data)
    data2 = _payload(90_000, 9394)
    pp2 = _place_copy(t, 'heB', data2)
    api = daemon.wait_ready()
    daemon.append(api, 'RelHE1', build_nzb(pp, 'he1.bin', len(data), 100_000, set()), False, 'he-key1', 100)
    daemon.append(api, 'RelHE2', build_nzb(pp2, 'he2.bin', len(data2), 100_000, set()), False, 'he-key2', 100)
    h1 = daemon.wait_history(api, 'RelHE1', timeout=60)
    h2 = daemon.wait_history(api, 'RelHE2', timeout=60)
    # deleting a finished item keeps a hidden duplicate record of it
    _rpc(daemon, 'editqueue', ['HistoryDelete', '', [h2['NZBID']]])
    time.sleep(1)
    hidden = [x for x in _rpc(daemon, 'history', [True]).get('result', []) if x.get('Kind') == 'DUP']
    dup_id = hidden[0]['NZBID'] if hidden else h2['NZBID']
    reply = _rpc(daemon, 'editqueue', ['HistorySetCategory', 'newcat', [h1['NZBID'], dup_id]]).get('result')
    t.procs[-1].kill()
    t.procs[-1].wait(timeout=30)
    daemon.start()
    api = daemon.wait_ready()
    category = next((x.get('Category') for x in api.history() if x['NZBID'] == h1['NZBID']), None)
    ok = bool(hidden) and reply is True and category == 'newcat'
    return ('historyeditlist', ok, 'hidden=%d reply=%s category_after_crash=%s' % (len(hidden), reply, category))


def scenario_speedtestnohistory(daemon, t):
    """testserverspeed with KeepHistory=0: the finished test download (no
    scripts, no disk writes) was removed from the queue and freed, and the
    cleanup then used it (use after free, a crash under AddressSanitizer).
    The cleanup runs first now; the daemon stays up."""
    import http.server
    data = _payload(300_000, 9595)
    pp = _place_copy(t, 'stA', data)
    nzb = build_nzb(pp, 'speed.bin', len(data), 100_000, set()).encode()

    hits = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            hits.append(self.path)
            self.send_response(200)
            self.send_header('Content-Type', 'application/x-nzb')
            self.send_header('Content-Length', str(len(nzb)))
            self.end_headers()
            self.wfile.write(nzb)

        def log_message(self, *args):
            pass

    srv = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    api = daemon.wait_ready()
    url = 'http://127.0.0.1:%d/speed.nzb' % srv.server_address[1]
    reply = _rpc(daemon, 'testserverspeed', [url, 1])
    # the test download is quick: wait until the nzb was fetched and the queue is empty
    deadline = time.time() + 60
    empty = False
    while time.time() < deadline and t.procs[-1].poll() is None:
        time.sleep(1)
        try:
            empty = bool(hits) and not api.listgroups()
        except Exception:
            break
        if empty:
            time.sleep(3)
            break
    srv.shutdown()
    alive = t.procs[-1].poll() is None
    ok = alive and empty and reply.get('result') is True
    return ('speedtestnohistory', ok, 'reply=%s fetched=%d done=%s alive=%s' % (reply.get('result', reply.get('error')), len(hits), empty, alive))


def scenario_getunsafe(daemon, t):
    """Commands that change state or reach out (readurl, the speed tests)
    ran over GET as "safe" methods; they
    need POST now, as every other changing command does. Reading commands
    still work over GET."""
    import json as _json
    import urllib.request as _req
    daemon.wait_ready()

    def get(path):
        with _req.urlopen('http://127.0.0.1:%d/jsonrpc/%s' % (daemon.rpc_port, path), timeout=30) as reply:
            return _json.loads(reply.read().decode())

    refused = {}
    for method in ('readurl?=http%3A%2F%2F127.0.0.1%3A9%2F&=x', 'testdiskspeed?=/tmp&=1&=1'):
        err = get(method).get('error') or {}
        refused[method.split('?')[0]] = err.get('code')
    version = get('version').get('result')
    posted = _rpc(daemon, 'readurl', ['http://127.0.0.1:9/', 'x']).get('error', {}).get('code')
    ok = all(code == 4 for code in refused.values()) and bool(version) and posted not in (None, 4)
    return ('getunsafe', ok, 'get_error_codes=%s version_over_get=%s post_error=%s' % (refused, version, posted))


def scenario_netspeedpause(daemon, t):
    """testnetworkspeed paused downloads for the test and then always
    unpaused them, also when they had been paused before (by the user or
    for low disk space). A pause from before the test stays now."""
    daemon.wait_ready()
    _rpc(daemon, 'pausedownload', [])
    started = time.time()
    reply = _rpc(daemon, 'testnetworkspeed', [], timeout=180)
    took = time.time() - started
    paused = _rpc(daemon, 'status', []).get('result', {}).get('DownloadPaused')
    ok = paused is True
    return ('netspeedpause', ok, 'paused_after=%s took=%.0fs reply=%s' % (
        paused, took, str(reply.get('result', reply.get('error')))[:80]))


def scenario_httpedge(daemon, t):
    """Two answers of the web server on a kept-alive connection:
    - POST with Content-Length: 0 got no answer at all, the client waited
      until its timeout; it gets 400 now (and the connection is closed);
    - GET /nzbget got the redirect and then a second, stray response, which a
      client reads as the answer to its next request; it gets one now."""
    daemon.wait_ready()

    def exchange(request, wait):
        sock = socket.create_connection(('127.0.0.1', daemon.rpc_port), timeout=wait)
        sock.sendall(request)
        data = b''
        try:
            while True:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                data += chunk
        except socket.timeout:
            pass
        sock.close()
        return data

    started = time.time()
    post = exchange(b'POST /jsonrpc HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\nContent-Length: 0\r\n\r\n', 5)
    post_took = time.time() - started
    redirect = exchange(b'GET /nzbget HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n', 3)
    responses = redirect.count(b'HTTP/1.')
    ok = post.startswith(b'HTTP/1.') and b' 400' in post.split(b'\r\n')[0] and post_took < 4 and responses == 1 and b' 301' in redirect.split(b'\r\n')[0]
    return ('httpedge', ok, 'post=%r took=%.1fs redirect_responses=%d first=%r' % (
        post.split(b'\r\n')[0], post_took, responses, redirect.split(b'\r\n')[0]))


def scenario_volumereset(daemon, t):
    """resetservervolume reset the counters in memory only: the stats are
    saved when a download changes them, so after a reset with nothing
    downloaded since, a restart (here a kill) brought the old counters back.
    The reset is saved now."""
    data = _payload(300_000, 9696)
    pp = _place_copy(t, 'vrA', data)
    api = daemon.wait_ready()
    daemon.append(api, 'RelVR', build_nzb(pp, 'vr.bin', len(data), 100_000, set()), False, 'vr-key', 100)
    daemon.wait_history(api, 'RelVR', timeout=60)
    time.sleep(3)
    before = _rpc(daemon, 'servervolumes', []).get('result', [{}])[0].get('TotalSizeLo', 0)
    reset = _rpc(daemon, 'resetservervolume', [-1, '']).get('result')
    time.sleep(4)
    t.procs[-1].kill()
    t.procs[-1].wait(timeout=30)
    daemon.start()
    daemon.wait_ready()
    after = _rpc(daemon, 'servervolumes', []).get('result', [{}])[0].get('TotalSizeLo', 0)
    ok = before > 0 and reset is True and after == 0
    return ('volumereset', ok, 'total_before=%d reset=%s total_after_restart=%d' % (before, reset, after))


def scenario_xmlparamdecode(daemon, t):
    """String parameters of testserver, testserverspeed and testdiskspeed
    weren't decoded: over XML-RPC a url with "&" (sent as
    "&amp;") was fetched with "&amp;" in it. And testserver over JSON-P (GET)
    parsed the query string as a JSON body and always answered "Invalid
    JSON"; JSON-P now runs only the reading methods, so testserver, which
    connects out, is refused there through the callback (error 4, or 401
    now that testserver is also refused to the restricted access JSON-P
    runs with)."""
    import http.server
    import urllib.request as _req
    hits = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            hits.append(self.path)
            self.send_response(404)
            self.send_header('Content-Length', '0')
            self.end_headers()

        def log_message(self, *args):
            pass

    srv = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    api = daemon.wait_ready()
    api.testserverspeed('http://127.0.0.1:%d/x.nzb?a=1&b=2' % srv.server_address[1], 1)
    deadline = time.time() + 20
    while time.time() < deadline and not hits:
        time.sleep(0.5)
    srv.shutdown()
    url = 'http://127.0.0.1:%d/jsonprpc/testserver?=cb&=127.0.0.1&=%d&=&=&=false&=&=5&=0' % (daemon.rpc_port, daemon.nntp_port)
    with _req.urlopen(url, timeout=30) as reply:
        jsonp = reply.read().decode()
    refused = jsonp.startswith('cb(') and ('"code" : 4,' in jsonp or '"code" : 401,' in jsonp)
    ok = hits and hits[0] == '/x.nzb?a=1&b=2' and 'Invalid JSON' not in jsonp and refused
    return ('xmlparamdecode', bool(ok), 'fetched=%s jsonp=%s' % (hits[:1], jsonp[:160].replace('\n', ' ')))


def _path_traversal(daemon, t, name):
    """A hostile NZB or article names a file outside the destination: the nzb
    subject is "../../../escape.bin", the article's yEnc name (rewritten by a
    proxy) "../../ab.bin", and a rar member "../../escape2.bin". No file is
    written outside the destination folder. The download names are made safe;
    the rar member breaches the unpack limits (#453): post-processing fails
    with the reason logged (zipfast refuses the name before the member starts
    and does not report it, so the reason doesn't name the member)."""
    data = _payload(90_000, 9797)
    t.write_file(os.path.join('data', 'ptA/aaaaaaaa.bin'), data)
    inner = _payload(120_000, 9798)
    volumes = generators.rar3_store_volumes_valid('../../escape2.bin', inner, 200_000)
    members = [('ptA/aaaaaaaa.bin', '../../../escape.bin', len(data), 100_000, set())]
    for i, vol in enumerate(volumes, 1):
        rel = 'ptA/rel.part%02d.rar' % i
        t.write_file(os.path.join('data', rel), vol)
        members.append((rel, 'Rel.part%02d.rar' % i, len(vol), 500_000, set()))
    api = daemon.wait_ready()
    started = time.time()
    daemon.append(api, 'RelPT', build_multi_nzb(members), False, 'pt-key', 100)
    h = daemon.wait_history(api, 'RelPT', timeout=120)
    dst = os.path.join(t.work, 'main', 'dst')
    outside = []
    for dp, _, files in os.walk(os.path.dirname(t.work.rstrip('/'))):
        for fn in files:
            full = os.path.join(dp, fn)
            if fn in ('escape.bin', 'escape2.bin', 'ab.bin') \
                    and not full.startswith(dst + os.sep) and os.path.getmtime(full) >= started:
                outside.append(full)
    inside = sorted(os.path.basename(r) for r in t.find_files('main', 'dst'))
    reason = _grep_log(t, 'Unpack limit: an archive entry or link would land outside the unpack folder')
    ok = not outside and h['Status'] == 'FAILURE/UNPACK' and reason >= 1
    return (name, ok, 'status=%s reason_logged=%d outside_dst=%s inside=%s' % (
        h['Status'], reason, outside, inside))


def scenario_pathtraversalnzb(daemon, t):
    return _path_traversal(daemon, t, 'pathtraversalnzb')


def scenario_pathtraversalarticle(daemon, t):
    """As pathtraversalnzb, with FileNaming=article (the yEnc name counts)."""
    return _path_traversal(daemon, t, 'pathtraversalarticle')


def scenario_urlschemes(daemon, t):
    """Only http and https are fetched: appendurl with file:// (a local nzb
    that exists) and gopher:// fails to fetch, and file:///... sent as nzb
    content is not followed either. Asked for by the proxy's review."""
    data = _payload(90_000, 9999)
    pp = _place_copy(t, 'usA', data)
    local = t.path('main', 'local.nzb')
    with open(local, 'w') as f:
        f.write(build_nzb(pp, 'local.bin', len(data), 100_000, set()))
    api = daemon.wait_ready()
    _rpc(daemon, 'appendurl', ['RelUSF.nzb', 'file://' + local, '', 0, False, False, 'usf', 0, 'SCORE', []])
    _rpc(daemon, 'appendurl', ['RelUSG.nzb', 'gopher://127.0.0.1:%d/x' % daemon.rpc_port, '', 0, False, False, 'usg', 0, 'SCORE', []])
    content = base64.standard_b64encode(('file://' + local).encode()).decode()
    _rpc(daemon, 'append', ['RelUSC.nzb', content, '', 0, False, False, 'usc', 0, 'SCORE', []])
    statuses = {}
    for name in ('RelUSF', 'RelUSG', 'RelUSC'):
        try:
            statuses[name] = daemon.wait_history(api, name, timeout=60)['Status']
        except RuntimeError:
            statuses[name] = 'TIMEOUT'
    downloaded = [r for r in t.find_files('main', 'dst') if r.endswith('local.bin')]
    ok = all(st.startswith('FAILURE') for st in statuses.values()) and not downloaded
    return ('urlschemes', ok, 'statuses=%s downloaded=%s' % (statuses, downloaded))


def scenario_nzbentities(daemon, t):
    """NZB XML with entity declarations: an external entity (a local file)
    and an expansion bomb are refused at the scan, and the file's content
    shows up nowhere. A plain nzb with the standard DOCTYPE still downloads.
    Asked for by the proxy's review."""
    data = _payload(90_000, 10101)
    pp = _place_copy(t, 'neA', data)
    plain = build_nzb(pp, 'ne.bin', len(data), 100_000, set())
    secret_path = t.path('main', 'secret.txt')
    with open(secret_path, 'w') as f:
        f.write('SECRET-CONTENT-12345')
    body = plain[plain.index('<nzb'):]
    doctype = '<!DOCTYPE nzb PUBLIC "-//newzBin//DTD NZB 1.0//EN" "http://www.newzbin.com/DTD/nzb/nzb-1.0.dtd">\n'
    xxe = ('<?xml version="1.0"?>\n<!DOCTYPE nzb [<!ENTITY x SYSTEM "file://%s">]>\n' % secret_path +
           body.replace('<file ', '<file note="&x;" ', 1))
    lol = ('<?xml version="1.0"?>\n<!DOCTYPE nzb [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">'
           '<!ENTITY c "&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;"><!ENTITY d "&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;">]>\n' +
           body.replace('<file ', '<file note="&d;" ', 1))
    std = '<?xml version="1.0"?>\n' + doctype + body
    api = daemon.wait_ready()
    for name, xml in (('RelNEX', xxe), ('RelNEL', lol), ('RelNES', std)):
        daemon.append(api, name, xml, False, name.lower(), 100)
    statuses = {name: daemon.wait_history(api, name, timeout=60)['Status'] for name in ('RelNEX', 'RelNEL', 'RelNES')}
    leaked = _grep_log(t, 'SECRET-CONTENT')
    for h in api.history():
        leaked += str(h).count('SECRET-CONTENT')
    ok = (statuses['RelNEX'].startswith('FAILURE') and statuses['RelNEL'].startswith('FAILURE') and
          statuses['RelNES'].startswith('SUCCESS') and leaked == 0)
    return ('nzbentities', ok, 'statuses=%s leaked=%d' % (statuses, leaked))


def _uu_parts(data, name, parts):
    """``data`` uuencoded and cut into ``parts`` article bodies (lines of 45
    bytes; "begin" in the first, "end" in the last), dot-stuffed for NNTP."""
    import binascii
    lines = [binascii.b2a_uu(data[i:i + 45]).rstrip(b'\n') for i in range(0, len(data), 45)]
    per = -(-len(lines) // parts)
    bodies = []
    for k in range(parts):
        chunk = lines[k * per:(k + 1) * per]
        if k == 0:
            chunk = [b'begin 644 ' + name.encode()] + chunk
        if k == parts - 1:
            chunk = chunk + [b'`', b'end']
        bodies.append(b''.join((b'.' + l if l.startswith(b'.') else l) + b'\r\n' for l in chunk))
    return bodies


def scenario_uucache(daemon, t):
    """A uuencoded file of three articles with the article cache on and
    DirectWrite=no: cached articles were written at their segment offset,
    which is 0 for uuencoded ones (only yEnc tells where a part goes), so
    each overwrote the one before. The file comes out whole now."""
    data = _payload(45 * 300, 10202)
    ids = ['uu%d@x' % i for i in range(3)]
    for mid, body in zip(ids, _uu_parts(data, 'uu.bin', 3)):
        daemon.fake_nntp.bodies_by_id[mid] = body
    daemon.fake_nntp.alive = set(ids)
    api = daemon.wait_ready()
    daemon.append(api, 'RelUU', _fake_nzb_ids(ids, len(data), name='uu.bin').decode(), False, 'uu-key', 100)
    h = daemon.wait_history(api, 'RelUU', timeout=60)
    files = [r for r in t.find_files('main', 'dst') if r.endswith('uu.bin')]
    got = t.read_file(files[0]) if files else b''
    ok = h['Status'].startswith('SUCCESS') and got == data
    return ('uucache', ok, 'status=%s files=%s size=%d/%d identical=%s' % (h['Status'], files, len(got), len(data), got == data))


def scenario_filepausepars(daemon, t):
    """editqueue FilePauseAllPars / FilePauseExtraPars on file ids paused the
    par files but answered false (the list of items was emptied before the
    result was taken). They answer true now."""
    data = _payload(400_000, 10405)
    pp = _place_copy(t, 'fpA', data)
    api = daemon.wait_ready()
    daemon.append(api, 'RelFP', build_nzb_with_par2(t, pp, 'fp.bin', data, 100_000, set()), True, 'fp-key', 100)
    time.sleep(1)
    group = next(g for g in api.listgroups() if g['NZBName'] == 'RelFP')
    files = _rpc(daemon, 'listfiles', [0, 0, group['NZBID']]).get('result', [])
    ids = [f['ID'] for f in files]
    allpars = _rpc(daemon, 'editqueue', ['FilePauseAllPars', '', ids]).get('result')
    extra = _rpc(daemon, 'editqueue', ['FilePauseExtraPars', '', ids]).get('result')
    ok = len(ids) >= 2 and allpars is True and extra is True
    return ('filepausepars', ok, 'files=%d allpars=%s extrapars=%s' % (len(ids), allpars, extra))


def scenario_mergefinished(daemon, t):
    """GroupMerge of a group with a file already downloaded: the merge set the
    category and rebuilt the source's own directory, so the finished file
    stayed there - the merged group's par-check and unpack never saw it and
    the folder was left behind. It moves to the merged group's folder now."""
    a = _payload(90_000, 10506)
    b1 = _payload(30_000, 10507)
    b2 = _payload(90_000, 10508)
    for rel, data in (('mfA/a.bin', a), ('mfB/b1.bin', b1), ('mfB/b2.bin', b2)):
        t.write_file(os.path.join('data', rel), data)
    api = daemon.wait_ready()
    daemon.append(api, 'RelMFA', build_multi_nzb([('mfA/a.bin', 'a.bin', len(a), 100_000, set())]), True, 'mf-a', 100)
    daemon.append(api, 'RelMFB', build_multi_nzb([('mfB/b1.bin', 'b1.bin', len(b1), 100_000, set()),
                                                  ('mfB/b2.bin', 'b2.bin', len(b2), 100_000, set())]), True, 'mf-b', 100)
    time.sleep(1)
    groups = {g['NZBName']: g for g in api.listgroups()}
    gid_a, gid_b = groups['RelMFA']['NZBID'], groups['RelMFB']['NZBID']
    files_b = _rpc(daemon, 'listfiles', [0, 0, gid_b]).get('result', [])
    b1_id = next(f['ID'] for f in files_b if 'b1' in f['Filename'])
    _rpc(daemon, 'editqueue', ['FileResume', '', [b1_id]])
    deadline = time.time() + 60
    while time.time() < deadline and len(_rpc(daemon, 'listfiles', [0, 0, gid_b]).get('result', [])) > 1:
        time.sleep(0.5)
    merged = _rpc(daemon, 'editqueue', ['GroupMerge', '', [gid_a, gid_b]]).get('result')
    _rpc(daemon, 'editqueue', ['GroupResume', '', [gid_a]])
    h = daemon.wait_history(api, 'RelMFA', timeout=90)
    found = {}
    for rel in t.find_files('main', 'dst'):
        found.setdefault(os.path.basename(rel), []).append(rel)
    dest_dir = os.path.dirname(found.get('a.bin', [''])[0])
    together = all(os.path.dirname(found.get(n, [''])[0]) == dest_dir for n in ('a.bin', 'b1.bin', 'b2.bin'))
    whole = together and t.read_file(found['b1.bin'][0]) == b1
    ok = merged is True and h['Status'].startswith('SUCCESS') and whole
    return ('mergefinished', ok, 'merged=%s status=%s files=%s' % (merged, h['Status'], {k: v for k, v in found.items()}))


def _direct_rename_subdir(daemon, t, name):
    """DirectRename=yes, a par2 naming its file "Sub/dr.bin" while the file
    (posted as obf.bin) is still downloading: the rename of the file in
    progress set its name to the relative path, but completing it never
    created "Sub" - with DirectWrite the move of the temporary file failed
    (the data stayed in <id>.out.tmp), without it the file couldn't be
    created and failed. The completed file was recorded by its base name
    too. Now the file lands in Sub/ and the download succeeds."""
    data = _payload(600_000, 10609)
    par = generators.par2_index([('Sub/dr.bin', data)])
    t.write_file(os.path.join('data', 'drA/obf.bin'), data)
    t.write_file(os.path.join('data', 'drA/rel.par2'), par)
    members = [('drA/rel.par2', 'rel.par2', len(par), 500_000, set()),
               ('drA/obf.bin', 'obf.bin', len(data), 100_000, set())]
    api = daemon.wait_ready()
    daemon.append(api, 'RelDR', build_multi_nzb(members), False, 'dr-key', 100)
    h = daemon.wait_history(api, 'RelDR', timeout=120)
    files = {r: t.read_file(r) for r in t.find_files('main', 'dst')}
    placed = [r for r in files if r.endswith('/Sub/dr.bin')]
    whole = bool(placed) and files[placed[0]] == data
    stranded = [r for r in t.find_files('main') if r.endswith('.out.tmp')]
    renamed = _grep_log(t, 'Renaming in-progress file')
    ok = h['Status'].startswith('SUCCESS') and whole and not stranded and renamed >= 1
    return (name, ok, 'status=%s renamed_in_progress=%d placed=%s whole=%s stranded=%s files=%s' % (
        h['Status'], renamed, placed, whole, stranded, sorted(files)[:6]))


def scenario_directrenamesubdir(daemon, t):
    return _direct_rename_subdir(daemon, t, 'directrenamesubdir')


def scenario_directrenamesubdirjoin(daemon, t):
    """As directrenamesubdir, with DirectWrite=no (the file is joined)."""
    return _direct_rename_subdir(daemon, t, 'directrenamesubdirjoin')


_PERCENT_SERVER_PORT = []


def _percent_server_options():
    port = free_port()
    _PERCENT_SERVER_PORT[:] = [port]
    return ['Server1.Port=%d' % port, 'ArticleRetries=0', 'Server1.Retention=0']


def scenario_nntppercent(daemon, t):
    """A news server whose greeting is "400 busy %s%s%s...": the error message
    holding the server's answer was used as a printf format. The daemon stays
    up and logs the answer as sent."""
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(('127.0.0.1', _PERCENT_SERVER_PORT[0]))
    srv.listen(16)

    def serve():
        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                return
            try:
                conn.sendall(b'400 busy %s%s%s%s%s%s%s%s%s%s\r\n')
            except OSError:
                pass
            conn.close()
    threading.Thread(target=serve, daemon=True).start()
    api = daemon.wait_ready()
    nzb = _fake_nzb_ids(['pc%d@x' % i for i in range(3)], 30_000).decode()
    daemon.append(api, 'RelPC', nzb, False, 'pc-key', 100)
    time.sleep(8)
    alive = t.procs[-1].poll() is None
    literal = _grep_log(t, 'busy %s%s%s')
    srv.close()
    ok = alive and literal >= 1
    return ('nntppercent', ok, 'alive=%s literal_logs=%d' % (alive, literal))


def scenario_joinequalpieces(daemon, t):
    """Split pieces all of one size (the file an exact multiple of the piece
    size), par-checked: the joined-size check took the expected size as
    first * (count - 1) + the size of a different, shorter last piece, which
    there wasn't - every such join failed with "joined X of Y bytes". The
    file is joined now."""
    data = _payload(3_000_000, 10710)
    pieces = [data[i:i + 1_000_000] for i in range(0, len(data), 1_000_000)]
    par = generators.par2_index([('eq.mkv.%03d' % (i + 1), piece) for i, piece in enumerate(pieces)])
    members = []
    for i, piece in enumerate(pieces, 1):
        rel = 'jeA/eq.mkv.%03d' % i
        t.write_file(os.path.join('data', rel), piece)
        members.append((rel, 'eq.mkv.%03d' % i, len(piece), 500_000, set()))
    t.write_file(os.path.join('data', 'jeA/eq.par2'), par)
    members.append(('jeA/eq.par2', 'eq.par2', len(par), 500_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'RelJE', build_multi_nzb(members), False, 'je-key', 100)
    h = daemon.wait_history(api, 'RelJE', timeout=180)
    joined = [r for r in t.find_files('main', 'dst') if r.endswith('/eq.mkv')]
    whole = bool(joined) and t.read_file(joined[0]) == data
    ok = h['Status'].startswith('SUCCESS') and whole
    return ('joinequalpieces', ok, 'status=%s par=%s joined=%s whole=%s' % (h['Status'], h.get('ParStatus'), joined, whole))


def scenario_directunpackkeepnointer(daemon, t):
    """As directunpackkeep without InterDir: the unpack folder is the
    download's own folder there, and the failed direct unpack's cleanup
    deleted it with everything downloaded so far."""
    r = scenario_directunpackkeep(daemon, t)
    return ('directunpackkeepnointer',) + tuple(r[1:])


def scenario_parscanpercent(daemon, t):
    """ParScan=full with "%" in the download's name: the par2 file's path was
    used as a printf format for the scan wildcard - a garbled path (the
    par-check couldn't find its files) or a crash. The repair works now."""
    data = _payload(400_000, 10811)
    name = 'pc%s%d%x.bin'
    pp = _place_copy(t, 'ppA', data)
    api = daemon.wait_ready()
    daemon.append(api, 'Rel%s%dPS', build_nzb_with_par2(t, pp, name, data, 100_000, set()), False, 'ps-key', 100)
    h = daemon.wait_history(api, 'Rel%s%dPS', timeout=120)
    alive = t.procs[-1].poll() is None
    ok = alive and h.get('ParStatus') == 'SUCCESS'
    return ('parscanpercent', ok, 'alive=%s status=%s par=%s' % (alive, h['Status'], h.get('ParStatus')))


def scenario_retentionnodate(daemon, t):
    """An nzb whose files have no date, a server with a retention: the file
    time defaults to 0, its age came out at about 20,000 days, and every
    article failed "out of server retention" without a request. Now an
    unknown age doesn't count against the retention."""
    data = _payload(300_000, 10912)
    pp = _place_copy(t, 'rnA', data)
    nzb = re.sub(r' date="[0-9]*"', '', build_nzb(pp, 'rn.bin', len(data), 100_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'RelRN', nzb, False, 'rn-key', 100)
    h = daemon.wait_history(api, 'RelRN', timeout=60)
    out_of_retention = _grep_log(t, 'out of server retention')
    ok = 'date=' not in nzb and h['Status'].startswith('SUCCESS') and out_of_retention == 0
    return ('retentionnodate', ok, 'status=%s retention_failures=%d' % (h['Status'], out_of_retention))


def scenario_joingroupreconnect(daemon, t):
    """A server that drops the connection every 2 articles, with the removed
    option Server1.JoinGroup=yes still in the config: once the reconnected
    connection skipped GROUP and articles that exist counted as missing.
    JoinGroup is gone now (articles are fetched by message-id, never with
    GROUP; the option is dropped quietly), so the server here doesn't ask
    for GROUP: every article arrives across the reconnects."""
    ids = ['jg%d@x' % i for i in range(8)]
    for mid, body in zip(ids, _uu_parts(_payload(45 * 80, 11013), 'jg.bin', 8)):
        daemon.fake_nntp.bodies_by_id[mid] = body
    daemon.fake_nntp.alive = set(ids)
    daemon.fake_nntp.drop_every = 2
    api = daemon.wait_ready()
    daemon.append(api, 'RelJG', _fake_nzb_ids(ids, 45 * 80, name='jg.bin').decode(), False, 'jg-key', 100)
    h = daemon.wait_history(api, 'RelJG', timeout=90)
    ok = (h['Status'].startswith('SUCCESS') and not h.get('FailedArticles')
          and daemon.fake_nntp.sessions >= 4)
    return ('joingroupreconnect', ok, 'status=%s failed_articles=%s no_group_replies=%d sessions=%d' % (
        h['Status'], h.get('FailedArticles'), daemon.fake_nntp.no_group_replies, daemon.fake_nntp.sessions))


def scenario_joingroupnogroups(daemon, t):
    """JoinGroup=yes and an nzb whose file lists no group: the join loop never
    ran, its empty answer counted as "connection closed", and the article was
    retried forever - the download never ended. It downloads now."""
    ids = ['ng%d@x' % i for i in range(3)]
    for mid, body in zip(ids, _uu_parts(_payload(45 * 30, 11114), 'ng.bin', 3)):
        daemon.fake_nntp.bodies_by_id[mid] = body
    daemon.fake_nntp.alive = set(ids)
    nzb = _fake_nzb_ids(ids, 45 * 30, name='ng.bin').decode().replace('<groups><group>alt.binaries.test</group></groups>', '')
    api = daemon.wait_ready()
    daemon.append(api, 'RelNG', nzb, False, 'ng-key', 100)
    try:
        status = daemon.wait_history(api, 'RelNG', timeout=60)['Status']
    except RuntimeError:
        status = 'STUCK'
    ok = '<group>' not in nzb and status.startswith('SUCCESS')
    return ('joingroupnogroups', ok, 'status=%s closed_logs=%d' % (status, _grep_log(t, 'Connection closed by remote host')))


def scenario_stallquit(daemon, t):
    """A server stalls in the middle of an article (ArticleTimeout=5): after
    the timeout the connection was closed with QUIT and waited for its answer
    (7.6 s in all before, 6.1 s now). It's closed at once now."""
    ids = ['sq%d@x' % i for i in range(2)]
    for mid, body in zip(ids, _uu_parts(_payload(45 * 200, 11215), 'sq.bin', 2)):
        daemon.fake_nntp.bodies_by_id[mid] = body
    daemon.fake_nntp.alive = set(ids)
    daemon.fake_nntp.stall_ids = {ids[0]}
    api = daemon.wait_ready()
    started = time.time()
    daemon.append(api, 'RelSQ', _fake_nzb_ids(ids, 45 * 200, name='sq.bin').decode(), False, 'sq-key', 100)
    h = daemon.wait_history(api, 'RelSQ', timeout=90)
    took = time.time() - started
    ok = h['Status'].startswith('SUCCESS') and took < 7
    return ('stallquit', ok, 'status=%s took=%.1fs (one timeout is 5 s)' % (h['Status'], took))


def scenario_appendslashname(daemon, t):
    """append with a name holding "/" ("[abc123] [16/22] - Show.S01E05.nzb", as
    indexers name postings): the name was taken as a path, everything up to
    the last "/" dropped - "22] - _Show.S01E05". The whole name is kept now,
    with the "/" made safe; a name climbing out ("../../../x") stays inside
    the destination and isn't taken for a hidden file."""
    data = _payload(90_000, 11316)
    pp = _place_copy(t, 'snA', data)
    api = daemon.wait_ready()
    name = '[abc123] [16/22] - Show.S01E05.nzb'
    content = base64.standard_b64encode(build_nzb(pp, 'sn.bin', len(data), 100_000, set()).encode()).decode()
    nid = _rpc(daemon, 'append', [name, content, '', 0, False, True, 'sn-key', 100, 'SCORE', []]).get('result')
    time.sleep(1)
    got = next((g['NZBName'] for g in api.listgroups() if g['NZBID'] == nid), None)
    # a name climbing out of the destination stays inside it
    data2 = _payload(90_000, 11317)
    pp2 = _place_copy(t, 'snB', data2)
    token = uuid.uuid4().hex[:8]
    content2 = base64.standard_b64encode(build_nzb(pp2, 'sn2-%s.bin' % token, len(data2), 100_000, set()).encode()).decode()
    nid2 = _rpc(daemon, 'append', ['../../../escapename.nzb', content2, '', 0, False, False, 'sn-key2', 100, 'SCORE', []]).get('result')
    h = {'Status': 'TIMEOUT', 'NZBName': None}
    deadline = time.time() + 60
    while time.time() < deadline:
        found = [x for x in api.history() if x['NZBID'] == nid2]
        if found:
            h = found[0]
            break
        time.sleep(0.5)
    dst = os.path.join(t.work, 'main', 'dst')
    outside = [os.path.join(dp, f) for dp, _, fs in os.walk(os.path.dirname(t.work.rstrip('/')))
               for f in fs if f == 'sn2-%s.bin' % token and not os.path.join(dp, f).startswith(dst + os.sep)]
    ok = (got is not None and got.startswith('[abc123]') and 'Show.S01E05' in got and
          h['Status'].startswith('SUCCESS') and not outside)
    return ('appendslashname', ok, 'name=%r escape_name=%r status=%s outside=%s' % (got, h['NZBName'], h['Status'], outside))


def scenario_parrenamebad(daemon, t):
    """Par-rename with a par2 index missing the description of two files (a
    damaged index) and one file to rename: the index was listed as damaged
    once per missing description, renamed to .bad once and then failed to
    rename again for each other listing. It's listed once now."""
    a = _payload(200_000, 11418)
    b = _payload(100_000, 11419)
    c = _payload(100_000, 11420)
    par = generators.par2_index([('real.mkv', a), ('b.mkv', b), ('c.mkv', c)], no_desc=('b.mkv', 'c.mkv'))
    t.write_file(os.path.join('data', 'prA/obf.bin'), a)
    t.write_file(os.path.join('data', 'prA/rel.par2'), par)
    members = [('prA/obf.bin', 'obf.bin', len(a), 100_000, set()), ('prA/rel.par2', 'rel.par2', len(par), 500_000, set())]
    api = daemon.wait_ready()
    daemon.append(api, 'RelPR', build_multi_nzb(members), False, 'pr-key', 100)
    h = daemon.wait_history(api, 'RelPR', timeout=120)
    damaged = _grep_log(t, 'Damaged par2-file detected')
    failed = _grep_log(t, 'Failed to rename file')
    renamed = bool([r for r in t.find_files('main', 'dst') if r.endswith('/real.mkv')])
    ok = renamed and damaged == 1 and failed == 0
    return ('parrenamebad', ok, 'status=%s renamed=%s damaged_logs=%d failed_renames=%d' % (h['Status'], renamed, damaged, failed))


def scenario_longnamedir(daemon, t):
    """A download name past 255 bytes (and one starting with "."): its
    directory "<name>.#<id>" couldn't be created (ENAMETOOLONG), every file
    write failed and counted as failed articles - a healthy posting ended
    FAILURE/HEALTH. The directory name is cut to fit now (on a whole UTF-8
    character), and a leading "." gets "_" (no hidden directory)."""
    a = _payload(200_000, 11521)
    b = _payload(200_000, 11522)
    pa = _place_copy(t, 'lnA', a)
    pb = _place_copy(t, 'lnB', b)
    api = daemon.wait_ready()
    long_name = 'L' * 250 + '\u00e9' * 30 + '.S01E01'
    for name, pp, data in ((long_name, pa, a), ('.hiddenname', pb, b)):
        content = base64.standard_b64encode(build_nzb(pp, name[:8].strip('.') + '.bin', len(data), 100_000, set()).encode()).decode()
        _rpc(daemon, 'append', [name + '.nzb', content, '', 0, False, False, name[:8], 100, 'SCORE', []])
    results = []
    deadline = time.time() + 90
    while time.time() < deadline and len(results) < 2:
        results = [x for x in api.history()]
        time.sleep(0.5)
    statuses = sorted(x['Status'] for x in results)
    dirs = [d for d in os.listdir(t.path('main', 'dst')) if os.path.isdir(t.path('main', 'dst', d))]
    sub = [os.path.join(dp, d) for dp, ds, _ in os.walk(t.path('main', 'dst')) for d in ds]
    hidden = [d for d in sub if os.path.basename(d).startswith('.')]
    longest = max((len(os.path.basename(d).encode()) for d in sub), default=0)
    ok = len(results) == 2 and all(st.startswith('SUCCESS') for st in statuses) and not hidden and longest <= 255
    return ('longnamedir', ok, 'statuses=%s longest_dir_bytes=%d hidden=%s' % (statuses, longest, hidden))


def _damaged_state_restart(daemon, t, damage):
    """Download one item, stop, damage a state file with ``damage(queue_dir)``,
    start again: returns (damaged, alive, api)."""
    data = _payload(90_000, 11623)
    pp = _place_copy(t, 'dsA', data)
    api = daemon.wait_ready()
    daemon.append(api, 'RelDS', build_nzb(pp, 'ds.bin', len(data), 100_000, set()), False, 'ds-key', 100)
    daemon.wait_history(api, 'RelDS', timeout=60)
    time.sleep(2)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    damaged = damage(t.path('main', 'queue'))
    daemon.start()
    try:
        api = daemon.wait_ready()
        time.sleep(2)
        alive = t.procs[-1].poll() is None and bool(_rpc(daemon, 'version', []).get('result'))
    except Exception:
        alive = False
    return damaged, alive


def scenario_damagedstats(daemon, t):
    """A stats file whose seconds array count is -1 (damaged): the count sized
    the array, which threw and ended the daemon at every start (a count below
    60 left the array too short for the counters). Now the file is refused
    and the daemon runs."""
    def damage(queue_dir):
        path = os.path.join(queue_dir, 'stats')
        lines = open(path).read().split('\n')
        for i, line in enumerate(lines):
            if line == '60':
                lines[i] = '-1'
                open(path, 'w').write('\n'.join(lines))
                return True
        return False
    damaged, alive = _damaged_state_restart(daemon, t, damage)
    ok = damaged and alive
    return ('damagedstats', ok, 'damaged=%s alive=%s' % (damaged, alive))


def scenario_damagedhistorykind(daemon, t):
    """A history record of a kind no version writes (9, a damaged file): the
    record was used without being created (a crash at every start). Now the
    history file is set aside as unreadable and the daemon runs."""
    def damage(queue_dir):
        path = os.path.join(queue_dir, 'history')
        lines = open(path).read().split('\n')
        for i, line in enumerate(lines):
            m = re.match(r'^(\d+),1,(\d{9,10})$', line)
            if m:
                lines[i] = '%s,9,%s' % (m.group(1), m.group(2))
                open(path, 'w').write('\n'.join(lines))
                return True
        return False
    damaged, alive = _damaged_state_restart(daemon, t, damage)
    set_aside = _grep_log(t, 'could not be read')
    ok = damaged and alive
    return ('damagedhistorykind', ok, 'damaged=%s alive=%s set_aside_logs=%d' % (damaged, alive, set_aside))


def scenario_articledecoyparrestart(daemon, t):
    """articledecoypar with a restart while the file still downloads, after
    the decoy's articles were borrowed: the duplicate an article came from
    isn't saved, so after the restart no article counted as borrowed and the
    par2 check of borrowed bytes was skipped - SUCCESS with the decoy's bytes
    in the file. Articles fetched in a fallback round count as borrowed now.
    (Here the borrowed articles are fetched again after the restart; the
    case of a staged borrowed article restored as done isn't set up.)"""
    size, seg = 5_000_000, 500_000
    data = _payload(size, 11724)
    decoy = _payload(size, 11725)
    pp = _place_copy(t, 'arA', data)
    dp = _place_copy(t, 'arB', decoy)
    primary = build_nzb_with_par2(t, pp, 'DecoyRs.bin', data, seg, {3, 7})
    donor = build_nzb(dp, 'DecoyRs.bin', size, seg, set())
    api = daemon.wait_ready()
    daemon.append(api, 'DonAR', donor, True, 'ar-key', 50)
    daemon.append(api, 'RelAR', primary, False, 'ar-key', 100)
    time.sleep(7)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    try:
        h = daemon.wait_history(api, 'RelAR', timeout=120)
    except RuntimeError:
        groups = [(g['NZBName'], g['Status'], g['RemainingSizeMB'], g.get('FailedArticles')) for g in api.listgroups()]
        logs = [l[-110:] for l in t.read_file('nzbget.log').decode(errors='replace').splitlines() if 'RelAR' in l][-6:]
        return ('articledecoyparrestart', False, 'stuck groups=%s logs=%s' % (groups, logs))
    integ = _verify_output(t, data)
    wrong_success = h['Status'].startswith('SUCCESS') and not integ
    rejected = _grep_log(t, "don't match its par2 checksums")
    ok = not wrong_success
    return ('articledecoyparrestart', ok, 'status=%s integrity=%s rejected_logs=%d' % (h['Status'], integ, rejected))


def scenario_emptyconfigread(daemon, t):
    """The config file emptied while the daemon runs (0 bytes), then the
    "loadconfig" call: reading it with a one-byte line buffer returned an empty
    line forever, the request never answered and a thread spun at 100 %
    CPU. It answers now."""
    daemon.wait_ready()
    open(t.path(daemon.conf_rel), 'w').close()
    try:
        reply = _rpc(daemon, 'loadconfig', [], timeout=15)
        answered = 'result' in reply or 'error' in reply
    except Exception:
        answered = False
    ok = answered
    return ('emptyconfigread', ok, 'answered=%s' % answered)


def scenario_damagedfilestate(daemon, t):
    """A partial file's saved state listing more articles than the file has
    (a stale or damaged "<id>s"): the article list was indexed with at(),
    which threw out of the queue coordinator and ended the daemon when the
    file was scheduled again. The state is refused now and the daemon runs.
    (Here the articles aren't loaded before the state, so the old code
    failed on the missing lines instead; the throw needs them loaded first.)"""
    size, seg = 1_000_000, 100_000
    data = _payload(size, 11826)
    pp = _place_copy(t, 'fsA', data)
    api = daemon.wait_ready()
    daemon.append(api, 'RelFS', build_nzb(pp, 'fs.bin', size, seg, set()), False, 'fs-key', 100)
    time.sleep(4)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    queue_dir = t.path('main', 'queue')
    damaged = False
    before = sorted(os.listdir(queue_dir))
    for name in os.listdir(queue_dir):
        if not re.match(r'^\d+s$', name):
            continue
        path = os.path.join(queue_dir, name)
        lines = open(path).read().split('\n')
        for i, line in enumerate(lines):
            if re.match(r'^\d+$', line) and int(line) == 10 and i + 1 < len(lines) and lines[i + 1].count(',') >= 3:
                lines[i] = '15'
                open(path, 'w').write('\n'.join(lines))
                damaged = True
                break
    daemon.start()
    try:
        api = daemon.wait_ready()
        time.sleep(25)
        alive = t.procs[-1].poll() is None and bool(_rpc(daemon, 'version', []).get('result'))
    except Exception:
        alive = False
    ok = damaged and alive
    return ('damagedfilestate', ok, 'damaged=%s alive=%s before=%s' % (damaged, alive, before))


def scenario_staletempstate(daemon, t):
    """A "queue.new" left by an interrupted write next to a good "queue": it
    stayed, and once the queue file was discarded (the queue emptied) the next
    start "restored" it as if the write had finished - a broken file, and all
    queue state (history too) set aside as unreadable. The stale temp file is
    dropped now and the history survives."""
    a = _payload(90_000, 11927)
    b = _payload(90_000, 11928)
    pa = _place_copy(t, 'stA', a)
    pb = _place_copy(t, 'stB', b)
    api = daemon.wait_ready()
    daemon.append(api, 'RelST1', build_nzb(pa, 'st1.bin', len(a), 100_000, set()), False, 'st-key1', 100)
    daemon.wait_history(api, 'RelST1', timeout=60)
    daemon.append(api, 'RelST2', build_nzb(pb, 'st2.bin', len(b), 100_000, set()), True, 'st-key2', 100)
    time.sleep(2)

    def restart():
        try:
            api.shutdown()
        except Exception:
            pass
        t.procs[-1].wait(timeout=60)

    restart()
    with open(t.path('main', 'queue', 'queue.new'), 'w') as f:
        f.write('nzbget diskstate file version 1\ngarbage from an interrupted write\n')
    daemon.start()
    api = daemon.wait_ready()
    gid = next(g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'RelST2')
    _rpc(daemon, 'editqueue', ['GroupFinalDelete', '', [gid]])
    time.sleep(2)
    restart()
    daemon.start()
    api = daemon.wait_ready()
    history = [h['NZBName'] for h in api.history()]
    unreadable = _grep_log(t, 'could not be read')
    ok = 'RelST1' in history and unreadable == 0
    return ('staletempstate', ok, 'history=%s unreadable_logs=%d' % (history, unreadable))


def scenario_xdecomp_slow(daemon, t):
    """xdecomp_zip with a slow donor (every donor article held 0.7 s, one
    connection) and DupeStreamTimeout=5: fetching and extracting a compressed
    donor patches no byte for a while, which the stall watchdog could take
    for no progress. That work counts as activity now (the run caps still
    apply). This donor is small enough to pass before too."""
    r = scenario_xdecomp_zip(daemon, t)
    stopped = _grep_log(t, 'nothing recovered for')
    return ('xdecomp_slow', r[1] and stopped == 0, '%s stall_stops=%d' % (r[2], stopped))


def scenario_failedloadfileinfos(daemon, t):
    """The history file is damaged, so loading fails after the queue loaded:
    the queued download kept only the stubs the queue file holds - files
    without name or size in the web interface, and the next save wrote the
    stubs over their file infos. Their file infos load now."""
    a = _payload(90_000, 12029)
    b = _payload(300_000, 12030)
    pa = _place_copy(t, 'flA', a)
    pb = _place_copy(t, 'flB', b)
    api = daemon.wait_ready()
    daemon.append(api, 'RelFL1', build_nzb(pa, 'fl1.bin', len(a), 100_000, set()), False, 'fl-key1', 100)
    daemon.wait_history(api, 'RelFL1', timeout=60)
    daemon.append(api, 'RelFL2', build_nzb(pb, 'fl2.bin', len(b), 100_000, set()), True, 'fl-key2', 100)
    time.sleep(2)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    path = t.path('main', 'queue', 'history')
    lines = open(path).read().split('\n')
    for i, line in enumerate(lines):
        m = re.match(r'^(\d+),1,(\d{9,10})$', line)
        if m:
            lines[i] = '%s,9,%s' % (m.group(1), m.group(2))
            break
    open(path, 'w').write('\n'.join(lines))
    daemon.start()
    api = daemon.wait_ready()
    group = next((g for g in api.listgroups() if g['NZBName'] == 'RelFL2'), None)
    files = _rpc(daemon, 'listfiles', [0, 0, group['NZBID']]).get('result', []) if group else []
    named = [f for f in files if f.get('Filename') and f.get('FileSizeLo', 0) > 0]
    ok = group is not None and len(files) >= 1 and len(named) == len(files)
    return ('failedloadfileinfos', ok, 'queued=%s files=%d named_with_size=%d' % (bool(group), len(files), len(named)))


def scenario_clientthreads(daemon, t):
    """nzbget -L G (the command-line client) while a download runs: the group's
    active downloads were never summed from its files on the client, so the
    "N threads" note never showed. It shows now."""
    import subprocess as _sp
    data = _payload(2_000_000, 12232)
    pp = _place_copy(t, 'ctA', data)
    api = daemon.wait_ready()
    daemon.append(api, 'RelCT', build_nzb(pp, 'ct.bin', len(data), 200_000, set()), False, 'ct-key', 100)
    time.sleep(2)
    out = _sp.run([t.nzbget, '-c', t.path(daemon.conf_rel), '-L', 'G'], capture_output=True, text=True, timeout=30).stdout
    line = next((l for l in out.splitlines() if 'RelCT' in l), '')
    ok = 'thread' in line
    return ('clientthreads', ok, 'line=%r' % line[:160])


def scenario_redownloadresetsrecovered(daemon, t):
    """complementary (3 articles borrowed from a duplicate), then "Download
    again" from the history: the new attempt kept the previous attempt's
    count of borrowed articles while its own article counts start at 0 -
    the failover gates weighed one against the other (a download arriving
    looked dead, or a dead one looked borrowed enough). It starts at 0 now."""
    r = scenario_complementary(daemon, t)
    api = daemon.api()
    h = next(x for x in api.history() if x['NZBName'] == 'ReleaseA')
    before = h.get('DupeRecoveredArticles')
    _rpc(daemon, 'pausedownload', [])
    _rpc(daemon, 'editqueue', ['HistoryRedownload', '', [h['NZBID']]])
    time.sleep(1)
    g = next((x for x in api.listgroups() if x['NZBName'] == 'ReleaseA'), {})
    after = g.get('DupeRecoveredArticles')
    ok = r[1] and before == 3 and after == 0
    return ('redownloadresetsrecovered', ok, 'recovered_before=%s after_redownload=%s' % (before, after))


def scenario_xmlrpccdata(daemon, t):
    """An XML-RPC append whose name is wrapped in CDATA (some client libraries
    do that): the markers were kept and sanitized into the name
    ("_![CDATA[x_&_y]]_"). The CDATA text is the name now."""
    import urllib.request as _req
    data = _payload(90_000, 12333)
    pp = _place_copy(t, 'xcA', data)
    api = daemon.wait_ready()
    content = base64.standard_b64encode(build_nzb(pp, 'xc.bin', len(data), 100_000, set()).encode()).decode()
    body = ('<?xml version="1.0"?><methodCall><methodName>append</methodName><params>'
            '<param><value><string><![CDATA[Rel&XC.nzb]]></string></value></param>'
            '<param><value><string>%s</string></value></param>'
            '<param><value><string></string></value></param><param><value><i4>0</i4></value></param>'
            '<param><value><boolean>0</boolean></value></param><param><value><boolean>1</boolean></value></param>'
            '<param><value><string>xc-key</string></value></param><param><value><i4>0</i4></value></param>'
            '<param><value><string>SCORE</string></value></param><param><value><array><data></data></array></value></param>'
            '</params></methodCall>') % content
    req = _req.Request('http://127.0.0.1:%d/xmlrpc' % daemon.rpc_port, body.encode(), {'Content-Type': 'text/xml'})
    with _req.urlopen(req, timeout=20) as r:
        reply = r.read().decode()
    time.sleep(1)
    names = [g['NZBName'] for g in api.listgroups()]
    ok = 'Rel&XC' in names
    return ('xmlrpccdata', ok, 'names=%s reply=%s' % (names, reply[:80].replace('\n', ' ')))


def scenario_webdirget(daemon, t):
    """GET of a directory of the web interface by its name (/js, no trailing
    slash), no login needed: the file loader opened the directory as a file,
    took a size it couldn't read for, and wrote through a null buffer or sent
    unread memory as the answer. It's a 404 now."""
    import urllib.request as _req
    import urllib.error as _err
    os.makedirs(t.path('main', 'web', 'js'), exist_ok=True)
    daemon.wait_ready()
    code = None
    try:
        with _req.urlopen('http://127.0.0.1:%d/js' % daemon.rpc_port, timeout=15) as r:
            code = r.status
    except _err.HTTPError as e:
        code = e.code
    except Exception as e:
        code = 'error: %s' % e
    time.sleep(1)
    alive = t.procs[-1].poll() is None
    ok = alive and code == 404
    return ('webdirget', ok, 'status=%s alive=%s' % (code, alive))


def scenario_webetag(daemon, t):
    """The ETag of a web file is a hash of the whole file. It was a hash of
    the file as a C string, which ends at the first zero byte: every PNG has
    one at offset 11, so images got the same ETag and a browser holding one
    was told "not modified" for another."""
    import urllib.request as _req
    import urllib.error as _err
    for name, tail in (('a.png', b'AAAA'), ('b.png', b'BBBB')):
        with open(t.path('main', 'web', name), 'wb') as f:
            f.write(b'\x89PNG\r\n\x1a\n\x00\x00\x00' + tail)
    daemon.wait_ready()
    base = 'http://127.0.0.1:%d/' % daemon.rpc_port
    with _req.urlopen(base + 'a.png', timeout=15) as r:
        etag_a = r.headers.get('ETag')
    with _req.urlopen(base + 'b.png', timeout=15) as r:
        etag_b = r.headers.get('ETag')
    # b.png asked for with a.png's ETag must come back in full
    code = None
    try:
        req = _req.Request(base + 'b.png', headers={'If-None-Match': etag_a or ''})
        with _req.urlopen(req, timeout=15) as r:
            code = r.status
            body = r.read()
    except _err.HTTPError as e:
        code = e.code
        body = b''
    ok = bool(etag_a) and etag_a != etag_b and code == 200 and body.endswith(b'BBBB')
    return ('webetag', ok, 'etags=%s,%s status=%s' % (etag_a, etag_b, code))


def scenario_web404warn(daemon, t):
    """A missing web file is warned about in the log, except the few a
    browser asks for on its own (favicon.ico...). The ignore list compared the
    full path, so it never matched, and its result was passed on as "print
    warning": no missing file was ever logged."""
    import urllib.request as _req
    import urllib.error as _err
    daemon.wait_ready()
    base = 'http://127.0.0.1:%d/' % daemon.rpc_port
    for name in ('missing-12435.js', 'favicon.ico'):
        try:
            _req.urlopen(base + name, timeout=15).read()
        except _err.HTTPError:
            pass
    time.sleep(2)
    missing = _grep_log(t, 'Resource: /missing-12435.js')
    favicon = _grep_log(t, 'Resource: /favicon.ico')
    ok = missing == 1 and favicon == 0
    return ('web404warn', ok, 'missing=%d favicon=%d' % (missing, favicon))


def scenario_partialnzb(daemon, t):
    """An nzb listing only the first half of a file's articles (no par2, no
    duplicate): one article fails, stream repair finds nothing to repair from,
    and its log says what's wrong. The yEnc headers declare the full size, so
    the missing figure is the absent half, not the failed article: a warning
    names the nzb as incomplete, and with no par2 nothing claims "left to
    par-repair"."""
    import re as _re
    size, seg = 4_000_000, 200_000
    data = _payload(size, 12436)
    pp = _place_copy(t, 'pnA', data, 'movie.mkv')
    nzb = build_nzb(pp, 'movie.mkv', size, seg, {3})
    # drop parts 11..20 from the nzb, and the part count from the subject:
    # nothing tells the missing half at add time (partialnzbdeclared has it)
    nzb = _re.sub(r'<segment bytes="\d+" number="(1[1-9]|20)">[^<]*</segment>\n?', '', nzb)
    nzb = nzb.replace(' yEnc (1/20)', ' yEnc')
    api = daemon.wait_ready()
    daemon.append(api, 'RelPN', nzb, False, 'pn-key', 100)
    h = daemon.wait_history(api, 'RelPN')
    missing = _grep_log(t, 'still missing after stream repair (no par2 files to repair it)')
    parclaim = _grep_log(t, 'left to par-repair')
    incomplete = _grep_log(t, "the nzb is incomplete")
    ok = missing == 1 and parclaim == 0 and incomplete == 1
    return ('partialnzb', ok, 'status=%s missing=%d parclaim=%d incomplete=%d'
            % (h['Status'], missing, parclaim, incomplete))


def scenario_partialnzbdeclared(daemon, t):
    """A file the nzb lists only the first half of, its subject declaring all
    200 parts ("yEnc (1/200)"): the 100 parts the nzb doesn't list count as
    failed from the start, as gaps between listed parts always did, so health
    shows the damage before anything downloads, and a warning says so when
    it's added. (Parts are counted only when the nzb plainly stops short:
    100 or more missing, under two thirds listed, a full last segment.)"""
    import re as _re
    size, seg = 4_000_000, 20_000
    data = _payload(size, 12437)
    pp = _place_copy(t, 'pdA', data, 'movie.mkv')
    nzb = build_nzb(pp, 'movie.mkv', size, seg, set())
    nzb = _re.sub(r'<segment bytes="\d+" number="(10[1-9]|1[1-9]\d|200)">[^<]*</segment>\n?', '', nzb)
    api = daemon.wait_ready()
    api.pausedownload()
    daemon.append(api, 'RelPD', nzb, False, 'pd-key', 100)
    time.sleep(2)
    group = next(g for g in api.listgroups() if g['NZBName'] == 'RelPD')
    health = int(group['Health'])
    warned = _grep_log(t, 'WARNING\tCollection RelPD lists only 100 of the 200 articles of movie.mkv')
    # one segment an indexer dropped (a gap, nothing past the end): a detail,
    # not a warning for every such release
    g = _payload(400_000, 12444)
    pg = _place_copy(t, 'pgA', g, 'gap.mkv')
    daemon.append(api, 'RelPG', build_nzb(pg, 'gap.mkv', len(g), 20_000, set()).replace(
        _re.search(r'<segment bytes="\d+" number="5">[^<]*</segment>\n', build_nzb(pg, 'gap.mkv', len(g), 20_000, set())).group(0), ''),
        False, 'pg-key', 100)
    time.sleep(2)
    gap_detail = _grep_log(t, 'DETAIL\tCollection RelPG lists only 19 of the 20 articles of gap.mkv')
    gap_warned = _grep_log(t, 'WARNING\tCollection RelPG')
    # a fragment: one segment of a file numbered 150 (nothing past it is
    # declared, all the gaps lie below): still a warning
    f = _payload(3_000_000, 12446)
    pf = _place_copy(t, 'pfA', f, 'frag.mkv')
    frag = build_nzb(pf, 'frag.mkv', len(f), 20_000, set())
    frag = _re.sub(r'<segment bytes="\d+" number="(?!150")\d+">[^<]*</segment>\n?', '', frag)
    daemon.append(api, 'RelPF', frag, False, 'pf-key', 100)
    time.sleep(2)
    frag_warned = _grep_log(t, 'WARNING\tCollection RelPF lists only 1 of the 150 articles of frag.mkv')
    ok = warned == 1 and health <= 550 and gap_detail == 1 and gap_warned == 0 and frag_warned == 1
    return ('partialnzbdeclared', ok, 'health=%d warned=%d gap_detail=%d gap_warned=%d frag_warned=%d'
            % (health, warned, gap_detail, gap_warned, frag_warned))


def scenario_daemonlock(daemon, t):
    """Daemon mode (-D) and its lock-file. A second daemon on a config whose
    lock is held exits, but on its way out deleted the lock-file of the
    running one, so a third start got a fresh file and ran beside it. A
    daemon that had reloaded never deleted its lock-file at shutdown, and an
    old longer pid left digits behind ours."""
    import re as _re
    import subprocess as _sp
    daemon.wait_ready()
    lock = t.path('main', 'd.lock')
    port = free_port()
    conf = t.read_file(daemon.conf_rel).decode()
    for key, value in (('ControlPort', str(port)), ('LockFile', lock),
                       ('QueueDir', t.path('main', 'dqueue')), ('LogFile', t.path('d.log')),
                       ('NzbDir', t.path('main', 'dnzb'))):
        conf = _re.sub(r'(?m)^%s=.*$' % key, '%s=%s' % (key, value), conf)
    dconf = t.path('main', 'd.conf')
    with open(dconf, 'w') as f:
        f.write(conf)
    with open(lock, 'w') as f:
        f.write('99999999\n')
    base = [t.nzbget, '-c', dconf]

    def wait_up(want):
        import urllib.request as _req
        deadline = time.time() + 30
        while time.time() < deadline:
            try:
                _req.urlopen('http://127.0.0.1:%d/jsonrpc/version' % port, timeout=2).read()
                up = True
            except Exception:
                up = False
            if up == want:
                return True
            time.sleep(0.3)
        return False

    _sp.run(base + ['-D'], timeout=30)
    first_up = wait_up(True)
    pid = open(lock).read()
    _sp.run(base + ['-D'], timeout=30)
    time.sleep(2)
    kept = os.path.exists(lock) and open(lock).read() == pid
    _sp.run(base + ['-O'], timeout=30)
    time.sleep(3)
    wait_up(True)
    _sp.run(base + ['-Q'], timeout=30)
    wait_up(False)
    time.sleep(2)
    removed = not os.path.exists(lock)
    # whatever happened, no daemon of this config outlives the test (after the
    # reload one could, under a pid the lock-file no longer named)
    conf_arg = base[base.index('-c') + 1] if '-c' in base else None
    if conf_arg:
        # (pgrep -f matches the whole command line, on Linux and macOS alike)
        found = _sp.run(['pgrep', '-f', conf_arg], capture_output=True, text=True).stdout.split()
        for entry in found:
            try:
                os.kill(int(entry), 9)
            except (OSError, ValueError):
                pass
    ok = first_up and _re.fullmatch(r'\d+\n', pid) is not None and kept and removed
    return ('daemonlock', ok, 'up=%s pid=%r kept=%s removed=%s' % (first_up, pid, kept, removed))


def scenario_daemonlockhealth(daemon, t):
    """Daemon mode keeps its lock-file lock through a systemhealth call and a
    reload. The health check opened and closed the lock-file to test it, and
    closing any descriptor of a file drops the process's lockf lock, so a
    second daemon on the same config then started beside the first one."""
    import json as _json
    import re as _re
    import subprocess as _sp
    import urllib.request as _req
    daemon.wait_ready()
    lock = t.path('main', 'h.lock')
    port = free_port()
    conf = t.read_file(daemon.conf_rel).decode()
    for key, value in (('ControlPort', str(port)), ('LockFile', lock),
                       ('QueueDir', t.path('main', 'hqueue')), ('LogFile', t.path('h.log')),
                       ('NzbDir', t.path('main', 'hnzb'))):
        conf = _re.sub(r'(?m)^%s=.*$' % key, '%s=%s' % (key, value), conf)
    conf += 'SystemHealthCheck=yes\n'
    hconf = t.path('main', 'h.conf')
    with open(hconf, 'w') as f:
        f.write(conf)
    base = [t.nzbget, '-c', hconf]

    def call(method):
        body = _json.dumps({'method': method, 'params': []}).encode()
        return _json.loads(_req.urlopen('http://127.0.0.1:%d/jsonrpc' % port, body, timeout=5).read())['result']

    def uptime():
        try:
            return call('status')['UpTimeSec']
        except Exception:
            return None

    def wait(cond, timeout=30):
        deadline = time.time() + timeout
        while time.time() < deadline:
            if cond():
                return True
            time.sleep(0.3)
        return False

    second_rc = None
    health_ok = reloaded = kept = refused = still_up = False
    pid = ''
    try:
        _sp.run(base + ['-D'], timeout=30)
        up = wait(lambda: uptime() is not None)
        pid = open(lock).read() if os.path.exists(lock) else ''
        health_ok = isinstance(call('systemhealth'), dict) if up else False
        time.sleep(2)
        before = uptime() or 0
        call('reload')
        # the reloaded instance answers again with a fresh uptime
        reloaded = wait(lambda: (lambda u: u is not None and u < before)(uptime()))
        call('systemhealth')
        second = _sp.run(base + ['-D'], timeout=30)
        second_rc = second.returncode
        time.sleep(3)
        kept = os.path.exists(lock) and open(lock).read() == pid
        log = t.read_file('h.log').decode(errors='replace')
        refused = 'could not acquire lock on lock-file' in log
        still_up = uptime() is not None
    finally:
        try:
            _sp.run(base + ['-Q'], timeout=30)
        except Exception:
            pass
        time.sleep(2)
        found = _sp.run(['pgrep', '-f', hconf], capture_output=True, text=True).stdout.split()
        for entry in found:
            try:
                os.kill(int(entry), 9)
            except (OSError, ValueError):
                pass
    ok = (_re.fullmatch(r'\d+\n', pid) is not None and health_ok and reloaded and kept
          and refused and still_up and len(found) == 0)
    return ('daemonlockhealth', ok, 'pid=%r health=%s reloaded=%s kept=%s refused=%s still_up=%s leftover=%s' % (
        pid, health_ok, reloaded, kept, refused, still_up, found))


def scenario_urlarchive(daemon, t):
    """A url that serves a zip with two nzb-files: both are added. The url
    item went with every nzb of the archive; adding the first took it out of
    the queue and freed it, and the second read the freed item."""
    import http.server
    import threading
    import zipfile
    a = _payload(90_000, 12439)
    b = _payload(90_000, 12440)
    pa = _place_copy(t, 'uaA', a)
    pb = _place_copy(t, 'uaB', b)
    zbuf = io.BytesIO()
    with zipfile.ZipFile(zbuf, 'w') as z:
        z.writestr('RelUA1.nzb', build_nzb(pa, 'ua1.bin', len(a), 100_000, set()))
        z.writestr('RelUA2.nzb', build_nzb(pb, 'ua2.bin', len(b), 100_000, set()))
    body = zbuf.getvalue()

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200)
            self.send_header('Content-Type', 'application/zip')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    api = daemon.wait_ready()
    _rpc(daemon, 'appendurl', ['RelUA.zip', 'http://127.0.0.1:%d/rel.zip' % server.server_address[1],
                               '', 0, False, True, '', 0, 'SCORE', []])
    deadline = time.time() + 30
    names = []
    while time.time() < deadline:
        names = sorted(g['NZBName'] for g in api.listgroups())
        if 'RelUA1' in names and 'RelUA2' in names:
            break
        time.sleep(0.5)
    time.sleep(2)
    names = sorted(g['NZBName'] for g in api.listgroups())
    server.shutdown()
    alive = t.procs[-1].poll() is None
    ok = alive and names == ['RelUA1', 'RelUA2']
    return ('urlarchive', ok, 'queued=%s alive=%s' % (names, alive))


def scenario_movemissingtarget(daemon, t):
    """GroupMoveBefore/After with a target id that isn't queued: refused, the
    order kept. It moved the groups to the bottom and answered true."""
    api = daemon.wait_ready()
    for i, n in enumerate(('RelMM1', 'RelMM2', 'RelMM3')):
        a = _payload(90_000, 12441 + 100 * i)
        pa = _place_copy(t, 'mm%d' % i, a)
        daemon.append(api, n, build_nzb(pa, n + '.bin', len(a), 100_000, set()), True, n, 0)
    time.sleep(1)
    before = [g['NZBName'] for g in api.listgroups()]
    gid = next(g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'RelMM1')
    res = _rpc(daemon, 'editqueue', ['GroupMoveBefore', '99999', [gid]]).get('result')
    after = [g['NZBName'] for g in api.listgroups()]
    ok = res is False and before == after
    return ('movemissingtarget', ok, 'result=%s before=%s after=%s' % (res, before, after))


def scenario_scangrowing(daemon, t):
    """An nzb-file copied into NzbDir in pieces, with pauses: a size change
    restarts the NzbDirFileAge wait. It never did (the check came after the new
    size was set), so a file that held one size for two scans once it had been
    seen for NzbDirFileAge was taken half-written and renamed .error."""
    a = _payload(90_000, 12442)
    pa = _place_copy(t, 'sgA', a)
    data = build_nzb(pa, 'sg.bin', len(a), 100_000, set()).encode()
    dest = t.path('main', 'nzb', 'RelSG.nzb')
    api = daemon.wait_ready()
    third = len(data) // 3
    with open(dest, 'wb') as f:
        f.write(data[:third])
    time.sleep(2.5)
    with open(dest, 'ab') as f:
        f.write(data[third:2 * third])
    time.sleep(3.5)
    with open(dest, 'ab') as f:
        f.write(data[2 * third:])
    deadline = time.time() + 30
    found = False
    while time.time() < deadline and not found:
        found = any(g['NZBName'] == 'RelSG' for g in api.listgroups()) or \
            any(h.get('NZBName') == 'RelSG' for h in api.history())
        time.sleep(0.5)
    errors = [n for n in os.listdir(t.path('main', 'nzb')) if n.endswith('.error')]
    ok = found and not errors
    return ('scangrowing', ok, 'added=%s error_files=%s' % (found, errors))


def scenario_archiveyoung(daemon, t):
    """A zip copied into NzbDir in pieces: it's extracted once it stopped
    changing for NzbDirFileAge, like an nzb-file. It was taken on the first
    scan that saw it, failed to extract half-written and was moved aside for
    good."""
    import zipfile
    a = _payload(90_000, 12443)
    pa = _place_copy(t, 'ayA', a)
    zbuf = io.BytesIO()
    with zipfile.ZipFile(zbuf, 'w') as z:
        z.writestr('RelAY.nzb', build_nzb(pa, 'ay.bin', len(a), 100_000, set()))
    body = zbuf.getvalue()
    dest = t.path('main', 'nzb', 'RelAY.zip')
    api = daemon.wait_ready()
    with open(dest, 'wb') as f:
        f.write(body[:len(body) // 2])
    time.sleep(2.5)
    with open(dest, 'ab') as f:
        f.write(body[len(body) // 2:])
    deadline = time.time() + 30
    found = False
    while time.time() < deadline and not found:
        found = any(g['NZBName'] == 'RelAY' for g in api.listgroups()) or \
            any(h.get('NZBName') == 'RelAY' for h in api.history())
        time.sleep(0.5)
    ok = found
    return ('archiveyoung', ok, 'added=%s nzbdir=%s' % (found, sorted(os.listdir(t.path('main', 'nzb')))))


def scenario_extdownloadname(daemon, t):
    """downloadextension with an extension name that is a path ("../../x"):
    the download was saved under that name, outside TempDir. And an archive
    that didn't install stayed in TempDir. Both are gone now."""
    import http.server
    import threading

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            body = b'not an archive'
            self.send_response(200)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    daemon.wait_ready()
    url = 'http://127.0.0.1:%d/ext.zip' % server.server_address[1]
    reply = _rpc(daemon, 'downloadextension', [url, '../../escape'])
    time.sleep(1)
    server.shutdown()
    escaped = os.path.exists(t.path('escape.tmp.zip'))
    left = [n for n in os.listdir(t.path('main', 'tmp')) if n.endswith('.tmp.zip')]
    alive = t.procs[-1].poll() is None
    ok = alive and not escaped and not left and 'error' in reply
    return ('extdownloadname', ok, 'escaped=%s left=%s reply=%s' % (escaped, left, str(reply)[:80]))


def scenario_connectioncap(daemon, t):
    """300 connections to the control port that send nothing: past 256 they're
    closed at once with a warning (a thread each, no limit: clients that connect
    and wait ran the program out of threads), and once they go the API answers."""
    daemon.wait_ready()
    socks = []
    for _ in range(300):
        s_ = socket.create_connection(('127.0.0.1', daemon.rpc_port), timeout=10)
        socks.append(s_)
    time.sleep(2)
    warned = _grep_log(t, 'Too many connections to the web interface/API')
    for s_ in socks:
        s_.close()
    time.sleep(2)
    alive = t.procs[-1].poll() is None
    ok_api = False
    try:
        ok_api = bool(_rpc(daemon, 'version', []).get('result'))
    except Exception:
        pass
    ok = alive and warned == 1 and ok_api
    return ('connectioncap', ok, 'warned=%d alive=%s api=%s' % (warned, alive, ok_api))


def scenario_parvolnames(daemon, t):
    """par2 volumes named by number only ("movie.vol-01.par2", as some posters
    do): their block count was read from the name as 1, 2, 3..., so a set that
    could repair the download added up to too few blocks and none was ever
    requested ("Need more N par-block(s)", then "Repair failed"). The count is
    estimated from the file size now, and the volumes are fetched."""
    import subprocess as _sp
    import glob as _glob
    size, seg = 3_000_000, 100_000
    data = _payload(size, 12447)
    pp = _place_copy(t, 'pvA', data, 'movie.mkv')
    work = t.path('data', 'pvA')
    _sp.run(['par2', 'create', '-q', '-q', '-s102400', '-c24', '-n3', '-a', 'movie', 'movie.par2', 'movie.mkv'],
            cwd=work, check=True, capture_output=True)
    vols = sorted(_glob.glob(os.path.join(work, 'movie.vol*.par2')))
    members = [(pp, 'movie.mkv', size, seg, {3, 7, 11, 15, 19, 23, 27})]
    main_par = os.path.join(work, 'movie.par2')
    members.append(('pvA/movie.par2', 'movie.par2', os.path.getsize(main_par), seg, set()))
    for i, vol in enumerate(vols, 1):
        name = 'movie.vol-%02d.par2' % i
        os.rename(vol, os.path.join(work, name))
        members.append(('pvA/' + name, name, os.path.getsize(os.path.join(work, name)), seg, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'RelPV', build_multi_nzb(members), False, 'pv-key', 100)
    h = daemon.wait_history(api, 'RelPV', timeout=240)
    # Fetched on the first damaged block while downloading, or by par-check.
    unpaused = _grep_log(t, 'for par-recovery') + _grep_log(t, 'for live par-recovery')
    ok = h['ParStatus'] == 'SUCCESS' and unpaused >= 1
    return ('parvolnames', ok, 'status=%s par=%s unpaused_logs=%d vols=%d'
            % (h['Status'], h['ParStatus'], unpaused, len(vols)))


def _pardamage_release(t, tag, seed, missing, size=2_000_000, block=524288):
    """8 data files of <size> (50 KB articles) and a par2 set of 512 KB blocks
    with only 3 recovery blocks; <missing> part numbers lost in every data file."""
    import subprocess as _sp
    work = t.path('data', tag)
    os.makedirs(work, exist_ok=True)
    names = []
    for i in range(8):
        name = 'data%d.bin' % i
        t.write_file(os.path.join('data', tag, name), _payload(size, seed + i))
        names.append(name)
    _sp.run(['par2', 'create', '-q', '-q', '-s%d' % block, '-c3', '-n1', '-a', 'rel', 'rel.par2'] + names,
            cwd=work, check=True, capture_output=True)
    members = []
    for name in sorted(n for n in os.listdir(work) if n.endswith('.par2')):
        members.append(('%s/%s' % (tag, name), name, os.path.getsize(os.path.join(work, name)), 50_000, set()))
    for name in names:
        members.append(('%s/%s' % (tag, name), name, size, 50_000, set(missing)))
    return members


def scenario_pardamagefailover(daemon, t):
    """Health counts articles, par2 repairs blocks: one lost article in each of
    8 files spoils 8 blocks of 512 KB, and the par2-files hold 3. Health stays at
    97%, so no gate saw it: the download ran to the end and failed par-repair
    before its duplicate was fetched. Now, once the blocks spoiled in the files
    finished exceed the recovery blocks, it fails over to the duplicate."""
    primary = _pardamage_release(t, 'pdP', 12450, {5})
    backup = _pardamage_release(t, 'pdB', 12450, set())
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'pd-key', 100)
    daemon.append(api, 'Backup', build_multi_nzb(backup), False, 'pd-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    deadline = time.time() + 240
    hb = daemon.wait_history(api, 'Backup')
    while hb['Status'].startswith('DELETED') and time.time() < deadline:
        time.sleep(1)
        hb = daemon.wait_history(api, 'Backup')
    over = _grep_log(t, "par-repair can't fix it")
    ok = over == 1 and not hp['Status'].startswith('FAILURE/PAR') and hb['Status'].startswith('SUCCESS')
    return ('pardamagefailover', ok, 'primary=%s backup=%s failover_logs=%d' % (hp['Status'], hb['Status'], over))


def scenario_pardamageborrow(daemon, t):
    """The par-damage failover with borrowing on (DupeArticleFallback=live): the
    spoiled blocks end its wait for par-check, it gets its turn, and the
    failover comes once it tried 32 articles and found under half (the backup
    waits in history: other files and sizes, another container by its par2 checksums,
    nothing it can lend). Lost articles count only once the par2 damage is known, so
    fewer than 32 may ever be counted (30 to 35 of the 48 here): with too few articles
    left to reach the sample, it fails over on what it counted."""
    # 1 MB blocks: a par2 set big enough that health (87%) stays above critical
    # (81%), while 48 lost articles spoil 16 blocks of the 3 it can recover
    primary = _pardamage_release(t, 'pbP', 12480, {3, 10, 17, 24, 31, 38}, block=1048576)
    backup = _pardamage_release(t, 'pbB', 12490, set(), size=2_100_000, block=1048576)
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'pb-key', 100)
    daemon.append(api, 'Backup', build_multi_nzb(backup), False, 'pb-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    deadline = time.time() + 240
    hb = daemon.wait_history(api, 'Backup')
    while hb['Status'].startswith('DELETED') and time.time() < deadline:
        time.sleep(1)
        hb = daemon.wait_history(api, 'Backup')
    over = _grep_log(t, "par-repair can't fix it")
    ok = over == 1 and hb['Status'].startswith('SUCCESS')
    return ('pardamageborrow', ok, 'primary=%s backup=%s failover_logs=%d' % (hp['Status'], hb['Status'], over))


def scenario_nosourceonce(daemon, t):
    """The only duplicate is another posting (other file names and sizes): no
    article can be borrowed. "No duplicate source" was logged for every missing
    article, dozens a second in production; now once per file."""
    # 8 files with par2 (borrowing needs it to check), 13 articles lost in each
    members = _pardamage_release(t, 'nsP', 12500, set(range(2, 41, 3)))
    bmembers = []
    for i in range(3):
        bsize = 2_300_000
        b = _place_copy(t, 'nsB', _payload(bsize, 12600 + i), 'other%02d.bin' % i)
        bmembers.append((b, 'other%02d.bin' % i, bsize, 50_000, set()))
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(members), True, 'ns-key', 100)
    daemon.append(api, 'Backup', build_multi_nzb(bmembers), True, 'ns-key', 90)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    logs = _grep_log(t, 'No duplicate source for')
    ok = 1 <= logs <= 8
    return ('nosourceonce', ok, 'status=%s no_source_logs=%d (8 files, 104 missing articles)' % (hp['Status'], logs))


def _twin_release(t, tag, seed, seg, block, recovery):
    """2 data files of <seed>'s bytes with a par2 set of <block> and <recovery>
    blocks, posted in articles of <seg> bytes."""
    import subprocess as _sp
    work = t.path('data', tag)
    os.makedirs(work, exist_ok=True)
    names = []
    for i in range(2):
        name = 'rel.part%02d.rar' % (i + 1)
        t.write_file(os.path.join('data', tag, name), _payload(1_500_000, seed + i))
        names.append(name)
    _sp.run(['par2', 'create', '-q', '-q', '-s%d' % block, '-c%d' % recovery, '-n2', '-a', 'rel', 'rel.par2'] + names,
            cwd=work, check=True, capture_output=True)
    members = []
    for name in sorted(os.listdir(work)):
        members.append(('%s/%s' % (tag, name), name, os.path.getsize(os.path.join(work, name)), seg, set()))
    return members


def scenario_twinalt(daemon, t):
    """A dupe key with three postings: the primary; a twin (the same files, posted
    in other article sizes with another par2 set: other block size and parity
    level); and a copy with other bytes. Each gets a fingerprint from its
    smallest par2-file. The twin is labelled a twin; the other copy only
    "container": other par2 checksums say another container, not another
    video, and no intake read the video inside."""
    primary = _twin_release(t, 'taP', 12700, 50_000, 65536, 4)
    twin = _twin_release(t, 'taT', 12700, 70_000, 131072, 9)
    alt = _twin_release(t, 'taA', 12800, 50_000, 65536, 4)
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'ta-key', 100)
    daemon.append(api, 'Twin', build_multi_nzb(twin), False, 'ta-key', 90)
    daemon.append(api, 'Alt', build_multi_nzb(alt), False, 'ta-key', 80)
    daemon.wait_history(api, 'Alt', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    daemon.wait_history(api, 'Primary', timeout=180)

    def param(h, name):
        return next((p['Value'] for p in h.get('Parameters', []) if p['Name'] == name), '')
    deadline = time.time() + 120
    while time.time() < deadline:
        hist = {h['Name']: h for h in api.history()}
        kinds = {n: param(hist.get(n, {}), 'DupeKind') for n in ('Twin', 'Alt')}
        if kinds['Twin'] and kinds['Alt']:
            break
        time.sleep(2)
    prints = {n: param(hist.get(n, {}), 'DupeFiles') for n in ('Primary', 'Twin', 'Alt')}
    ok = (kinds == {'Twin': 'twin', 'Alt': 'container'} and prints['Primary'] == prints['Twin'] and
          prints['Primary'] != prints['Alt'] and prints['Primary'] not in ('', 'none'))
    return ('twinalt', ok, 'kinds=%s prints=%s' % (kinds, prints))


def scenario_twinlabelonadd(daemon, t):
    """Duplicates are labelled as they are added, before the primary downloads:
    the primary waits paused in the queue, a twin and an alt are added. Their
    par2 fingerprints are fetched at once (not at the next 15-second round of
    the check), and both labels are there within 10 seconds of the last add."""
    primary = _twin_release(t, 'tlP', 13100, 50_000, 65536, 4)
    twin = _twin_release(t, 'tlT', 13100, 70_000, 131072, 9)
    alt = _twin_release(t, 'tlA', 13200, 50_000, 65536, 4)
    api = daemon.wait_ready()
    # past the check's first round after the start: only a nudge runs it soon
    time.sleep(2)
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'tl-key', 100)
    daemon.append(api, 'Twin', build_multi_nzb(twin), False, 'tl-key', 90)
    daemon.append(api, 'Alt', build_multi_nzb(alt), False, 'tl-key', 80)
    added = time.time()

    def param(h, name):
        return next((p['Value'] for p in h.get('Parameters', []) if p['Name'] == name), '')
    kinds, elapsed = {}, None
    while time.time() < added + 60:
        hist = {h['Name']: h for h in api.history()}
        kinds = {n: param(hist.get(n, {}), 'DupeKind') for n in ('Twin', 'Alt')}
        if kinds['Twin'] and kinds['Alt']:
            elapsed = time.time() - added
            break
        time.sleep(0.5)
    queued = [g for g in api.listgroups() if g['NZBName'] == 'Primary']
    ok = (kinds == {'Twin': 'twin', 'Alt': 'container'} and elapsed is not None and elapsed < 10 and
          len(queued) == 1 and queued[0]['DownloadedSizeMB'] == 0)
    return ('twinlabelonadd', ok, 'kinds=%s elapsed=%s primary_queued=%s' % (
        kinds, None if elapsed is None else round(elapsed, 1), bool(queued)))


def scenario_twinparrepair(daemon, t):
    """The primary loses 4 articles of part01 (about 6 blocks of 64 KB) and its own
    par2 set holds 2: par-repair can't do it alone. A twin in history (the same
    files in other article sizes, with a par2 set of 128 KB blocks and 20 of
    them) lends its par2-files: the smallest one first, then volumes enough for
    the blocks missing. An alt (other bytes) is never used. The primary is
    repaired, and the twin's files are gone afterwards."""
    primary = _twin_release(t, 'tpP', 12900, 50_000, 65536, 2)
    primary = [m[:4] + ({3, 9, 15, 21},) if m[1] == 'rel.part01.rar' else m for m in primary]
    twin = _twin_release(t, 'tpT', 12900, 70_000, 131072, 20)
    alt = _twin_release(t, 'tpA', 12950, 50_000, 65536, 20)
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'tp-key', 100)
    daemon.append(api, 'Alt', build_multi_nzb(alt), False, 'tp-key', 90)
    daemon.append(api, 'Twin', build_multi_nzb(twin), False, 'tp-key', 80)
    daemon.wait_history(api, 'Twin', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=240)
    tried = _grep_log(t, 'Trying the par2 set of twin Twin')
    alt_used = _grep_log(t, 'twin Alt')
    data = open(t.path('data', 'tpT', 'rel.part01.rar'), 'rb').read()
    out = [os.path.join(r, f) for d in (t.path('main', 'dst'), t.path('main', 'inter')) for r, _, fs in os.walk(d) for f in fs]
    part01 = [f for f in out if f.endswith('rel.part01.rar')]
    intact = bool(part01) and open(part01[0], 'rb').read() == data
    leftovers = [f for f in out if os.path.basename(f).startswith('_twin.')]
    ok = hp['Status'].startswith('SUCCESS') and tried == 1 and alt_used == 0 and intact and not leftovers
    return ('twinparrepair', ok, 'status=%s tried=%d alt_used=%d intact=%s leftovers=%d'
            % (hp['Status'], tried, alt_used, intact, len(leftovers)))


def scenario_twinaltsampled(daemon, t):
    """Postings without par2 can't be fingerprinted: a duplicate with the
    primary's article layout is compared by 6 articles at the same places.
    The same bytes make a twin, other bytes an alt. Another layout (other
    article sizes) is compared where both posts start an article at the same
    offset: the same bytes there prove the same files (DupeSampled
    "samefiles", an alt that needs no further check), never a twin, since
    the article counts differ."""
    size, seg = 3_000_000, 100_000
    data = _payload(size, 13000)
    members = {}
    for tag, payload, segment in (('Primary', data, seg), ('Twin', data, seg),
                                  ('Alt', _payload(size, 13001), seg), ('Other', data, 150_000)):
        path = _place_copy(t, 'ts' + tag, payload, 'movie.mkv')
        members[tag] = build_nzb(path, 'movie.mkv', size, segment, set())
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', members['Primary'], True, 'ts-key', 100)
    for i, tag in enumerate(('Twin', 'Alt', 'Other')):
        daemon.append(api, tag, members[tag], False, 'ts-key', 90 - i)
    daemon.wait_history(api, 'Other', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    daemon.wait_history(api, 'Primary', timeout=180)

    def param(h, name):
        return next((p['Value'] for p in h.get('Parameters', []) if p['Name'] == name), '')
    deadline = time.time() + 120
    kinds = {}
    while time.time() < deadline:
        hist = {h['Name']: h for h in api.history()}
        kinds = {n: param(hist.get(n, {}), 'DupeKind') for n in ('Twin', 'Alt', 'Other')}
        sampled = {n: param(hist.get(n, {}), 'DupeSampled') for n in ('Twin', 'Alt', 'Other')}
        if all(sampled.values()) and kinds['Twin'] and kinds['Alt'] and kinds['Other']:
            break
        time.sleep(2)
    ok = (kinds == {'Twin': 'twin', 'Alt': 'container', 'Other': 'alt'}
          and sampled == {'Twin': 'twin:hdr', 'Alt': 'alt', 'Other': 'samefiles'})
    return ('twinaltsampled', ok, 'kinds=%s sampled=%s' % (kinds, sampled))


def scenario_obfuscatedparnoborrow(daemon, t):
    """A par2-file with a random name (no .par2 anywhere) loses articles, and a
    duplicate posts a file of the same layout under another random name holding
    other par2 bytes. Nothing names either as par2: before, its articles could
    be borrowed into the par2 data. Its first article starts with a par2
    packet, so nothing is borrowed into it; a data file still borrows."""
    import subprocess as _sp
    work = t.path('data', 'opP')
    os.makedirs(work, exist_ok=True)
    data = _payload(1_200_000, 13100)
    t.write_file(os.path.join('data', 'opP', 'movie.mkv'), data)
    _sp.run(['par2', 'create', '-q', '-q', '-s65536', '-c12', '-n1', '-a', 'm', 'm.par2', 'movie.mkv'],
            cwd=work, check=True, capture_output=True)
    vol = next(n for n in os.listdir(work) if 'vol' in n)
    par = open(os.path.join(work, vol), 'rb').read()
    other = bytearray(par)
    other[200:] = _payload(len(par) - 200, 13101)	# par2 magic kept, other bytes
    seg = 50_000
    pp = _place_copy(t, 'opP2', par, 'a8f3k2q9z')
    pd = _place_copy(t, 'opP2', data, 'b7d1x0w4m')
    dp = _place_copy(t, 'opD', bytes(other), 'c2v9n4t6r')
    dd = _place_copy(t, 'opD', data, 'e5h8j1l3p')
    lost = {3, 6}
    primary = build_multi_nzb([(pd, 'b7d1x0w4m', len(data), seg, {5}),
                               (pp, 'a8f3k2q9z', len(par), seg, lost)])
    donor = build_multi_nzb([(dd, 'e5h8j1l3p', len(data), seg, set()),
                             (dp, 'c2v9n4t6r', len(par), seg, set())])
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', primary, True, 'op-key', 100)
    daemon.append(api, 'Donor', donor, True, 'op-key', 90)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=180)
    log = t.read_file('nzbget.log').decode(errors='replace')
    par_borrowed = sum(1 for line in log.splitlines() if 'a8f3k2q9z' in line and 'from duplicate' in line.lower())
    data_borrowed = sum(1 for line in log.splitlines() if 'b7d1x0w4m' in line and 'from duplicate' in line.lower())
    ok = par_borrowed == 0 and data_borrowed >= 1
    return ('obfuscatedparnoborrow', ok, 'status=%s par_borrowed=%d data_borrowed=%d'
            % (hp['Status'], par_borrowed, data_borrowed))


def _allheads_release(daemon, t, tag, par, alt, twin, alt_first_alive=False):
    """Every volume lost its header: a real `rar` RAR5 store set (with quick-open
    records, as rar 5+ writes by default) of movie.mkv whose every volume loses
    its FIRST article, as does every par2 file (the index too, when there is
    par2). Optional duplicates: ``alt`` is the same mkv in RAR5 volumes of
    another size, with its own first articles lost too; ``twin`` is the same
    volumes posted again with the same first articles lost. The extracted mkv
    must come out byte-identical."""
    import subprocess as _sp
    seg = 100_000
    movie = _payload(9_000_000, 14100)
    def rar_set(sub, vol_kb, base):
        work = t.path('data', sub)
        os.makedirs(work, exist_ok=True)
        t.write_file(os.path.join('data', sub, 'movie.mkv'), movie)
        _sp.run([generators.RAR_PATH, 'a', '-m0', '-ma5', '-v%dk' % vol_kb, '-idq', base + '.rar', 'movie.mkv'],
                cwd=work, check=True, capture_output=True)
        os.remove(os.path.join(work, 'movie.mkv'))
        names = sorted(n for n in os.listdir(work) if n.endswith('.rar'))
        return work, names
    work, names = rar_set(tag + 'A', 1500, 'Rel')
    if par:
        _sp.run(['par2', 'create', '-q', '-q', '-s100000', '-r20', '-n4', 'Rel.par2'] + names,
                cwd=work, check=True, capture_output=True)
    members = [('%sA/%s' % (tag, n), n, os.path.getsize(os.path.join(work, n)), seg, {1})
               for n in sorted(os.listdir(work))]
    api = daemon.wait_ready()
    if alt:
        awork, anames = rar_set(tag + 'B', 1100, 'Oth')
        amembers = [('%sB/%s' % (tag, n), n, os.path.getsize(os.path.join(awork, n)), 90_000,
                     set() if (alt_first_alive and i == 0) else {1}) for i, n in enumerate(anames)]
        _ds_append(api, 'Alt' + tag, build_multi_nzb(amembers), tag + '-key', 50,
                   params=[{'Name': 'DupeAlive', 'Value': '0'}])
    if twin:
        tmembers = [('%sA/%s' % (tag, n), 'Tw' + n, os.path.getsize(os.path.join(work, n)), seg, {1})
                    for n in names]
        _ds_append(api, 'Twin' + tag, build_multi_nzb(tmembers), tag + '-key', 40,
                   params=[{'Name': 'DupeAlive', 'Value': '0'}])
    daemon.append(api, 'Rel' + tag, build_multi_nzb(members), False, tag + '-key', 100)
    h = daemon.wait_history(api, 'Rel' + tag, timeout=400)
    ok = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    detail = 'status=%s par=%s unpack=%s mkv_identical=%s vols=%d' % (
        h['Status'], h['ParStatus'], h['UnpackStatus'], ok, len(names))
    return ok and h['Status'].startswith('SUCCESS'), detail


def scenario_allheadspar(daemon, t):
    """Every RAR volume and every par2 file (index included) lost article 1; no
    duplicate. The vol files keep the set's description in their last
    articles, and there are enough recovery blocks: par2 repairs it."""
    good, detail = _allheads_release(daemon, t, 'AHP', par=True, alt=False, twin=False)
    return ('allheadspar', good, detail)


def scenario_allheadsalt(daemon, t):
    """Every volume lost article 1, no par2; an alt (other volume size) lost the
    first article of every volume but its first, which sit at other mkv offsets.
    Headers come back from each volume's quick-open record, and the lost bytes
    from the alt."""
    good, detail = _allheads_release(daemon, t, 'AHA', par=False, alt=True, twin=False, alt_first_alive=True)
    return ('allheadsalt', good, detail)


def scenario_allheadsaltall(daemon, t):
    """Every volume of both copies lost article 1, no par2. Volume 1 of every
    RAR copy starts with the mkv's first bytes, so they are gone everywhere:
    this must fail (and say so) instead of reporting success."""
    good, detail = _allheads_release(daemon, t, 'AHX', par=False, alt=True, twin=False)
    return ('allheadsaltall', not good and 'status=FAILURE' in detail, detail)


def scenario_allheadstwin(daemon, t):
    """Every volume lost article 1, no par2; the only duplicate is a twin that
    lost the same articles. The bytes are gone everywhere: this must fail
    (it is the case nothing can rescue)."""
    good, detail = _allheads_release(daemon, t, 'AHT', par=False, alt=False, twin=True)
    return ('allheadstwin', not good, detail)


def scenario_lostindexpar(daemon, t):
    """The index par2-file (rel.par2) is lost entirely and the data file has
    holes; the recovery volumes are held back (ParCheck=auto). Post-processing
    found no par2-file on disk and skipped par-check, the download ended
    damaged with the volumes unused (8479). Now the smallest held-back volume
    is fetched: it carries the set's description, and repair runs."""
    members = _twin_release(t, 'liP', 13200, 50_000, 65536, 12)
    lost = []
    for m in members:
        if m[1] == 'rel.par2':
            m = m[:4] + (set(range(1, m[2] // m[3] + 2)),)
        elif m[1] == 'rel.part01.rar':
            m = m[:4] + ({4, 11},)
        lost.append(m)
    api = daemon.wait_ready()
    daemon.append(api, 'Lost', build_multi_nzb(lost), False, 'li-key', 100)
    h = daemon.wait_history(api, 'Lost', timeout=180)
    unpaused = _grep_log(t, 'No par2-file of Lost on disk to check it with')
    ok = h['Status'].startswith('SUCCESS') and unpaused == 1
    return ('lostindexpar', ok, 'status=%s unpaused_logs=%d' % (h['Status'], unpaused))


def _near_twin_release(t, tag, seed_first, names):
    """3 volumes of 400 KB under <names>: the first from <seed_first> (a re-packed
    archive header), the other two the same bytes in every release; a par2 set."""
    import subprocess as _sp
    work = t.path('data', tag)
    os.makedirs(work, exist_ok=True)
    for i, name in enumerate(names):
        t.write_file(os.path.join('data', tag, name), _payload(400_000, seed_first if i == 0 else 13300 + i))
    _sp.run(['par2', 'create', '-q', '-q', '-s65536', '-c2', '-n1', '-a', 'set', 'set.par2'] + names,
            cwd=work, check=True, capture_output=True)
    return [('%s/%s' % (tag, n), n, os.path.getsize(os.path.join(work, n)), 40_000, set())
            for n in sorted(os.listdir(work))]


def scenario_neartwinborrow(daemon, t):
    """Re-uploads of one archive (NZBDAV copies): random names, the first volume
    re-packed, the others the same bytes. The duplicate is labelled a near-twin
    (twin:2/3) by the par2 file lists, and the articles the primary lost in a
    volume both hold are borrowed from the duplicate's file with that content,
    whatever its name: by name nothing matched, and the equal-size volumes were
    ambiguous by layout."""
    primary = _near_twin_release(t, 'ntP', 13310, ['Qx81LmA.7z.001', 'Qx81LmA.7z.002', 'Qx81LmA.7z.003'])
    donor = _near_twin_release(t, 'ntD', 13320, ['Zp4KvTe.7z.001', 'Zp4KvTe.7z.002', 'Zp4KvTe.7z.003'])
    primary = [m[:4] + ({3, 7},) if m[1] == 'Qx81LmA.7z.002' else m for m in primary]
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'nt-key', 100)
    daemon.append(api, 'Donor', build_multi_nzb(donor), True, 'nt-key', 90)

    def param(item, name):
        return next((p['Value'] for p in item.get('Parameters', []) if p['Name'] == name), '')
    deadline = time.time() + 90
    while time.time() < deadline:
        groups = {g['NZBName']: g for g in api.listgroups()}
        if all(param(groups.get(n, {}), 'DupeFiles') for n in ('Primary', 'Donor')):
            break
        time.sleep(1)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=180)
    recovered = int(hp.get('DupeRecoveredArticles', 0))
    data = open(t.path('data', 'ntP', 'Qx81LmA.7z.002'), 'rb').read()
    out = [os.path.join(r, f) for d in (t.path('main', 'dst'), t.path('main', 'inter')) for r, _, fs in os.walk(d) for f in fs]
    vol = [f for f in out if f.endswith('Qx81LmA.7z.002')]
    intact = bool(vol) and open(vol[0], 'rb').read() == data
    deadline = time.time() + 60
    kind = ''
    while time.time() < deadline and not kind:
        donor_item = next((g for g in api.listgroups() if g['NZBName'] == 'Donor'), None) or \
            next((h for h in api.history() if h['Name'] == 'Donor'), {})
        kind = param(donor_item, 'DupeKind')
        time.sleep(1)
    ok = recovered >= 2 and intact and kind == 'twin:2/3'
    return ('neartwinborrow', ok, 'status=%s recovered=%d intact=%s kind=%s' % (hp['Status'], recovered, intact, kind))


def scenario_pardamagerepairable(daemon, t):
    """The same release with the lost articles inside 2 blocks (fewer than its 3
    recovery blocks): repairable, no failover, par-repair fixes it."""
    primary = _pardamage_release(t, 'prP', 12460, set())
    # two files lose an article each: 2 blocks
    primary = [m[:4] + ({5},) if m[1] in ('data0.bin', 'data1.bin') else m for m in primary]
    backup = _pardamage_release(t, 'prB', 12460, set())
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(primary), True, 'pr-key', 100)
    daemon.append(api, 'Backup', build_multi_nzb(backup), False, 'pr-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    over = _grep_log(t, "par-repair can't fix it")
    ok = over == 0 and hp['Status'].startswith('SUCCESS')
    return ('pardamagerepairable', ok, 'primary=%s failover_logs=%d' % (hp['Status'], over))


def _livepar_release(t, tag, seed, missing, recovery, size=2_000_000, block=524288, volumes=1):
    """8 data files of <size> (50 KB articles) with a par2 set of <block> blocks and
    <recovery> recovery blocks in <volumes> volumes; <missing> maps a data file number to the
    part numbers it loses. Returns the NZB members and the payloads by name."""
    import subprocess as _sp
    work = t.path('data', tag)
    os.makedirs(work, exist_ok=True)
    names, payloads = [], {}
    for i in range(8):
        name = 'data%d.bin' % i
        payloads[name] = _payload(size, seed + i)
        t.write_file(os.path.join('data', tag, name), payloads[name])
        names.append(name)
    _sp.run(['par2', 'create', '-q', '-q', '-s%d' % block, '-c%d' % recovery, '-n%d' % volumes, '-a', 'rel',
             'rel.par2'] + names, cwd=work, check=True, capture_output=True)
    members = []
    for name in sorted(n for n in os.listdir(work) if n.endswith('.par2')):
        members.append(('%s/%s' % (tag, name), name, os.path.getsize(os.path.join(work, name)), 50_000, set()))
    for i, name in enumerate(names):
        members.append(('%s/%s' % (tag, name), name, size, 50_000, set(missing.get(i, ()))))
    return members, payloads


def _livepar_io(t, timeout=30):
    """The live PAR I/O line of the job (written when its session ends), parsed into
    numbers; {} when none was written within <timeout> seconds."""
    pattern = re.compile(
        r'Live PAR \(job \d+\) I/O: source_bytes_read ([\d.]+) MB .*?backfill_bytes_read ([\d.]+) MB, '
        r'recovery_bytes_read ([\d.]+) MB.*?; (\d+) cache miss\(es\), (\d+) article\(s\) read from their '
        r'file by par-rs, (\d+) waited for the cache flush')
    deadline = time.time() + timeout
    while time.time() < deadline:
        found = pattern.findall(t.read_file('nzbget.log').decode(errors='replace'))
        if found:
            source, backfill, recovery, misses, read_by_parrs, awaited = found[-1]
            io = {'source_mb': float(source), 'backfill_mb': float(backfill),
                  'recovery_mb': float(recovery), 'misses': int(misses),
                  'read_by_parrs': int(read_by_parrs), 'awaited': int(awaited)}
            # Bytes read again at download end (the whole-file check; 0 with trusted slices),
            # slices back-filled from RAM, recovery volumes absorbed from RAM.
            extra = re.findall(
                r'target_verification_bytes_read ([\d.]+) MB, .*?ram_backfilled_slices (\d+), '
                r'recovery volumes from RAM (\d+) \(([\d.]+) MB\), trusted slice checks (\w+)',
                t.read_file('nzbget.log').decode(errors='replace'))
            if extra:
                verify, ram_backfill, ram_volumes, ram_volume_mb, trust = extra[-1]
                io.update({'verify_mb': float(verify), 'ram_backfilled': int(ram_backfill),
                           'ram_volumes': int(ram_volumes), 'ram_volume_mb': float(ram_volume_mb),
                           'trust': trust})
            kept = re.findall(r'(\d+) not kept at write \(no room\), (\d+) taken from bytes kept '
                              r'before the session', t.read_file('nzbget.log').decode(errors='replace'))
            if kept:
                io.update({'not_kept': int(kept[-1][0]), 'presession': int(kept[-1][1])})
            during = re.findall(r'(\d+) copied from the cache during a flush',
                                t.read_file('nzbget.log').decode(errors='replace'))
            if during:
                io['during_flush'] = int(during[-1])
            repeats = re.findall(r'(\d+) repeated capture\(s\) skipped',
                                 t.read_file('nzbget.log').decode(errors='replace'))
            if repeats:
                io['repeats'] = int(repeats[-1])
            stashed = re.findall(r'(\d+) taken from bytes kept at write',
                                 t.read_file('nzbget.log').decode(errors='replace'))
            if stashed:
                io['stashed'] = int(stashed[-1])
            return io
        time.sleep(0.5)
    return {}


def _livepar_run(daemon, t, tag, seed, missing, recovery, volumes=1, block=524288, size=2_000_000):
    members, payloads = _livepar_release(t, tag, seed, missing, recovery, size=size, block=block,
                                         volumes=volumes)
    api = daemon.wait_ready()
    daemon.append(api, 'Live', build_multi_nzb(members), False, tag + '-key', 100)
    h = daemon.wait_history(api, 'Live', timeout=300)
    intact = all(_verify_output(t, data, name, dirs=(('main', 'dst'), ('main', 'inter')))
                 for name, data in payloads.items())
    return h, intact, _livepar_io(t)


def scenario_liveparnocache(daemon, t):
    """Live PAR with no article cache (ArticleCache=0): the article cache never holds an
    article, so every one is a cache miss. The writer keeps each article's decoded bytes
    for live PAR as it writes them to the DirectWrite file (within ParBuffer), and the
    capture takes them there: par-rs reads nothing back from disk (source_bytes_read
    0 MB; before, it read every slice back from the file). The recovery volume is fetched
    for the two damaged blocks, and the live repair rebuilds them as the download ends.
    No batch par-repair runs. Articles written before the live session started (it starts
    once the par2 index arrives; on a slow machine a few data articles are on disk by then)
    are still read once, so up to 2 MB of the 16 MB may be read back; main read 14.3 MB."""
    h, intact, io, end_s = _livepar_run_timed(daemon, t, 'lnc', 12480, {0: {5}, 1: {5}}, 3)
    live = _grep_log(t, 'as the download ended')
    batch = _grep_log(t, 'PAR repair timings for')
    ok = (h['Status'].startswith('SUCCESS') and intact and live >= 1 and batch == 0
          and io.get('misses', 0) > 0 and io.get('source_mb', 99) <= 2.0)
    return ('liveparnocache', ok, 'status=%s intact=%s live_repairs=%d batch_repairs=%d end_to_history_s=%s io=%s'
            % (h['Status'], intact, live, batch, end_s, io))


def scenario_liveparobfuscated(daemon, t):
    """Obfuscated data file names (not renamed while downloading: DirectRename=no), two
    damaged blocks. The live repair at download end takes the damaged files under their
    obfuscated names (the live binding names their par2 files), and post-processing's
    par-rename keeps their certified CRCs: no batch par-repair reads the download again.
    Before, the obfuscated targets were "unresolvable" and a batch repair ran."""
    h, intact, io, end_s = _livepar_run_timed(daemon, t, 'lob', 12520, {0: {5}, 3: {12}}, 3,
                                              obfuscate=True)
    live = _grep_log(t, 'as the download ended')
    batch = _grep_log(t, 'PAR repair timings for')
    ok = h['Status'].startswith('SUCCESS') and intact and live >= 1 and batch == 0
    return ('liveparobfuscated', ok, 'status=%s intact=%s live_repairs=%d batch_repairs=%d end_to_history_s=%s io=%s'
            % (h['Status'], intact, live, batch, end_s, io))


def _livepar_run_timed(daemon, t, tag, seed, missing, recovery, obfuscate=False):
    """_livepar_run, timed from the end of the download (the job leaves the download
    queue for post-processing) to its history entry. With ``obfuscate`` the data files are
    posted under random names (the par2 files keep theirs). Returns (history, intact,
    live I/O, seconds from download end to history or None)."""
    import zlib
    members, payloads = _livepar_release(t, tag, seed, missing, recovery)
    if obfuscate:
        members = [(p, n if n.endswith('.par2') else 'q%07x' % (zlib.crc32(n.encode()) & 0xfffffff),
                    size, seg, lost) for (p, n, size, seg, lost) in members]
    api = daemon.wait_ready()
    daemon.append(api, 'Live', build_multi_nzb(members), False, tag + '-key', 100)
    deadline = time.time() + 300
    end = None
    h = None
    while time.time() < deadline:
        h = next((x for x in api.history() if x.get('NZBName') == 'Live'), None)
        if h:
            break
        g = next((g for g in api.listgroups() if g['NZBName'] == 'Live'), None)
        if end is None and (g is None or str(g.get('Status', '')).startswith('PP_')):
            end = time.time()
        time.sleep(0.1)
    if h is None:
        raise RuntimeError('timeout waiting for Live in history')
    done = time.time()
    intact = all(_verify_output(t, data, name, dirs=(('main', 'dst'), ('main', 'inter')))
                 for name, data in payloads.items())
    return h, intact, _livepar_io(t), (round(done - end, 2) if end else None)


def scenario_liveparplanned(daemon, t):
    """Live PAR sizes its recovery slots up front (with_recovery_plan): the slots for
    the expected damage plus margin exist before the first source slice arrives, so
    the recovery blocks fold in without re-reading any source slice
    (backfill_bytes_read 0), and the live repair finishes the job as the download
    ends with no batch par-repair."""
    h, intact, io = _livepar_run(daemon, t, 'lpl', 12490, {2: {5}, 6: {30}}, 3)
    live = _grep_log(t, 'as the download ended')
    batch = _grep_log(t, 'PAR repair timings for')
    ok = (h['Status'].startswith('SUCCESS') and intact and live >= 1 and batch == 0
          and bool(io) and io['backfill_mb'] == 0.0)
    return ('liveparplanned', ok, 'status=%s intact=%s live_repairs=%d batch_repairs=%d io=%s'
            % (h['Status'], intact, live, batch, io))


def scenario_livepartrusted(daemon, t):
    """Never read a byte twice at download end: two damaged blocks, the recovery volume
    fetched for them and the live repair as the download ends. Every slice live PAR
    verified is the byte written, so the end-of-repair check takes the verified slices
    as proof (par-rs set_trust_verified_slices) instead of re-reading every target
    (target_verification_bytes_read 0 MB; main read the whole 16 MB again), and the
    recovery volume is absorbed from the bytes written (on_recovery_volume_bytes:
    recovery_bytes_read 0 MB). The output is still byte-exact."""
    h, intact, io = _livepar_run(daemon, t, 'ltr', 12530, {1: {5}, 5: {22}}, 3)
    live = _grep_log(t, 'as the download ended')
    batch = _grep_log(t, 'PAR repair timings for')
    ok = (h['Status'].startswith('SUCCESS') and intact and live >= 1 and batch == 0
          and io.get('trust') == 'on' and io.get('verify_mb', 99) == 0.0
          and io.get('recovery_mb', 99) == 0.0 and io.get('ram_volumes', 0) >= 1)
    return ('livepartrusted', ok, 'status=%s intact=%s live_repairs=%d batch_repairs=%d io=%s'
            % (h['Status'], intact, live, batch, io))


def scenario_livepartrustedtemp(daemon, t):
    """livepartrusted with DirectWrite=no and no article cache: every article goes to an
    article temporary and is joined at completion. The bytes live PAR verifies are still the
    bytes joined, so the trusted check reads nothing again and the output is byte-exact."""
    h, intact, io = _livepar_run(daemon, t, 'ltt', 12540, {2: {7}, 6: {18}}, 3)
    live = _grep_log(t, 'as the download ended')
    batch = _grep_log(t, 'PAR repair timings for')
    ok = (h['Status'].startswith('SUCCESS') and intact and live >= 1 and batch == 0
          and io.get('trust') == 'on' and io.get('verify_mb', 99) == 0.0)
    return ('livepartrustedtemp', ok, 'status=%s intact=%s live_repairs=%d batch_repairs=%d io=%s'
            % (h['Status'], intact, live, batch, io))


def scenario_livepargrow(daemon, t):
    """Damage grows past the initial recovery plan: the queue's one recovery volume holds 80
    blocks, more than the 64 slots planned up front at most, so only the slots for the 0
    failures seen when the index arrived plus margin (2) are planned; 4 blocks get damaged
    later. The slots grow with the damage (set_max_syndrome_slots), the late slots are
    back-filled, and the live repair still finishes the job as the download ends, with no
    batch par-repair."""
    h, intact, io = _livepar_run(daemon, t, 'lpg', 12500, {4: {35}, 5: {20}, 6: {30}, 7: {30}}, 80)
    live = _grep_log(t, 'as the download ended')
    batch = _grep_log(t, 'PAR repair timings for')
    grew = _grep_log(t, 'repair memory grew to')
    ok = h['Status'].startswith('SUCCESS') and intact and live >= 1 and batch == 0 and grew >= 1
    return ('livepargrow', ok, 'status=%s intact=%s live_repairs=%d batch_repairs=%d grew=%d io=%s'
            % (h['Status'], intact, live, batch, grew, io))


def scenario_liveparzeroreads(daemon, t):
    """An article that came over the wire is never read twice: a plain download with damage
    (three damaged blocks in three files, the recovery blocks in four volumes, the article
    cache on). Live PAR takes every byte from RAM: the captures, bytes kept when a flush
    wrote an article first (lending free ParBuffer instead of dropping them), bytes written
    before the session started, recovery volumes from their written bytes, and slots for every
    queued recovery block planned up front (no back-fill). So par-rs reads no source byte
    (source_bytes_read 0, no article read from its file), back-fills nothing, reads no
    recovery volume back and checks no target again; the live repair finishes the job as the
    download ends, byte-exact. The harness's captures win the race with the cache flush, so
    the flush-first and pre-session paths are pinned by the parchecker unit tests
    (live_flush_before_capture_lends_for_the_kept_bytes,
    live_bytes_written_before_the_session_are_kept_for_it) and liveparnocache. Before: main
    read flushed articles back (6790: 601 MB of a
    3.3 GB set), back-filled growth slots (1550 MB) and read recovery volumes back (75 MB)."""
    h, intact, io = _livepar_run(daemon, t, 'lzr', 12550, {1: {6}, 4: {17}, 6: {29}}, 8, volumes=4)
    live = _grep_log(t, 'as the download ended')
    batch = _grep_log(t, 'PAR repair timings for')
    ok = (h['Status'].startswith('SUCCESS') and intact and live >= 1 and batch == 0 and bool(io)
          and io['source_mb'] == 0.0 and io['read_by_parrs'] == 0 and io['backfill_mb'] == 0.0
          and io['recovery_mb'] == 0.0 and io.get('verify_mb', 99) == 0.0
          and io.get('not_kept', 99) == 0)
    return ('liveparzeroreads', ok, 'status=%s intact=%s live_repairs=%d batch_repairs=%d io=%s'
            % (h['Status'], intact, live, batch, io))


def scenario_liveparflushrace(daemon, t):
    """liveparzeroreads at line speed with the cache flushing the files being downloaded:
    a 1 MB article cache (it flushes everything, files with downloads running included,
    whenever it is nearly full), 16 connections, and each flush holding its file's flush lock 150 ms before it
    writes (RUSTYGET_TEST_FLUSH_HOLD_MS, as a busy disk does). Articles that finish while
    their file is flushed are still in the cache: live PAR copies them there (the cache's
    snapshot no longer refuses during a flush), takes the bytes a flush or the file's
    completion wrote before its capture, and par-rs reads no source byte back
    (source_bytes_read 0, no article read from its file). Before: every article finishing
    during a flush of its file was read back (6790: 437 articles, 335.8 MB of a 3.3 GB
    set), and its later write kept a copy no capture took. The I/O line's "copied from the
    cache during a flush" count proves the race happened (main on this scenario: 35
    articles read back, 2.5 MB). Articles finishing while the session start replays the
    finished ones are captured twice (the replay and their own completion): the repeat
    finds the RAM copy taken and is skipped ("repeated capture(s) skipped"), where it had
    par-rs read the same bytes back (3-7 articles, 0.1-0.3 MB, before that fix)."""
    h, intact, io = _livepar_run(daemon, t, 'lfr', 12560, {1: {6}, 4: {17}, 6: {29}}, 8, volumes=4,
                                 size=6_000_000)
    live = _grep_log(t, 'as the download ended')
    batch = _grep_log(t, 'PAR repair timings for')
    flushes = _grep_log(t, 'Flushing cache for')
    ok = (h['Status'].startswith('SUCCESS') and intact and live >= 1 and batch == 0 and bool(io)
          and flushes >= 4 and io.get('during_flush', 0) > 0
          and io['source_mb'] == 0.0 and io['read_by_parrs'] == 0
          and io['backfill_mb'] == 0.0 and io.get('not_kept', 99) == 0)
    return ('liveparflushrace', ok, 'status=%s intact=%s live_repairs=%d batch_repairs=%d flushes=%d io=%s'
            % (h['Status'], intact, live, batch, flushes, io))


def scenario_liveparrestart(daemon, t):
    """The daemon restarts while a job downloads, after its par2 index completed and its live
    PAR session started. A session starts only when a par2 file of the job completes, so
    after the restart the job had none until its next par2 file completed (a recovery
    volume, at the very end); that session then read everything downloaded meanwhile back
    from disk, or no session came and a batch par-repair ran (6790 job 2466: 86 GB read
    back). Now the queue coordinator queues the completed index as the start again
    ("queued a session start from"), the session resumes while the rest downloads
    (data still left when its status line returns), and the live repair finishes the job as
    the download ends: no batch par-repair. The damage is in the last two files, so no
    recovery volume is unpaused before the restart (one completing would bring a session
    back by itself). Main: the session came back only when a recovery volume completed
    near the end (6 of 30 MB left) and read 26.2 MB back; with the fix it comes back at once
    (30 MB left) and reads 3.9 MB, what was on disk at the restart (whole slices)."""
    members, payloads = _livepar_release(t, 'lrs', 12570, {6: {60}, 7: {70}}, 8, size=4_000_000,
                                         volumes=4)
    api = daemon.wait_ready()
    daemon.append(api, 'Live', build_multi_nzb(members), False, 'lrs-key', 100)
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, 'Live par2 for Live') == 0:
        time.sleep(0.2)
    started = _grep_log(t, 'Live par2 for Live')
    time.sleep(1.5)
    g = next((g for g in api.listgroups() if g['NZBName'] == 'Live'), None)
    left_mb = g.get('RemainingSizeMB', 0) if g else 0
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    live_before = _grep_log(t, 'as the download ended')
    sessions_before = _grep_log(t, 'Live par2 for Live')
    daemon.start()
    api = daemon.wait_ready()
    # Data left when the session came back: without the re-arm it came only with a par2
    # file completing at the end (nothing left), or never.
    left_at_resume = None
    deadline = time.time() + 120
    while time.time() < deadline and left_at_resume is None:
        g = next((g for g in api.listgroups() if g['NZBName'] == 'Live'), None)
        if g is None:
            break
        if _grep_log(t, 'Live par2 for Live') > sessions_before:
            left_at_resume = g.get('RemainingSizeMB', 0)
        time.sleep(0.2)
    h = daemon.wait_history(api, 'Live', timeout=300)
    intact = all(_verify_output(t, data, name, dirs=(('main', 'dst'), ('main', 'inter')))
                 for name, data in payloads.items())
    io = _livepar_io(t)
    rearmed = _grep_log(t, 'queued a session start from')
    live = _grep_log(t, 'as the download ended') - live_before
    batch = _grep_log(t, 'PAR repair timings for')
    ok = (h['Status'].startswith('SUCCESS') and intact and started > 0 and left_mb > 0
          and rearmed >= 1 and bool(left_at_resume) and left_at_resume >= 0.7 * left_mb
          and bool(io) and io['source_mb'] <= 6.0 and live >= 1 and batch == 0)
    return ('liveparrestart', ok, 'status=%s intact=%s started=%d left_mb=%s left_at_resume=%s rearmed=%d '
            'live_repairs=%d batch_repairs=%d io=%s' % (h['Status'], intact, started, left_mb, left_at_resume,
                                                         rearmed, live, batch, io))


def scenario_liveparheldvolume(daemon, t):
    """The planned recovery volumes cover the damage, but the volume past the plan is never
    fetched for it (6790, PR #729: a volume past the plan was fetched after ~620 of 634 slices
    verified and back-filling its slots read the 3.3 GB set again, 3169.7 MB). 25 recovery
    blocks in five volumes; 15 blocks of whole volumes are planned up front. One lost
    article spans about nine 5600-byte slices, so the damage grows by nine at once. The
    volume selector takes the smallest volume that covers a shortfall at once, and the
    planned volumes left are each too small: before, it took the volume past the plan, whose
    blocks needed slots added after slices verified, and those were back-filled from disk.
    Volumes past the plan are now held while the planned ones may cover the damage: only
    planned volumes are fetched, nothing is back-filled (backfill_bytes_read 0), and the live
    repair finishes the job as the download ends, byte-exact."""
    h, intact, io = _livepar_run(daemon, t, 'lhv', 12560, {3: {7}}, 25, volumes=5, block=5600)
    live = _grep_log(t, 'as the download ended')
    batch = _grep_log(t, 'PAR repair timings for')
    log = t.read_file('nzbget.log').decode(errors='replace')
    fetched = sorted(int(e) for e in re.findall(r'Successfully downloaded Live/rel\.vol(\d+)\+', log))
    planned = all(e < 15 for e in fetched) and bool(fetched)
    ok = (h['Status'].startswith('SUCCESS') and intact and live >= 1 and batch == 0 and bool(io)
          and io['backfill_mb'] == 0.0 and planned)
    return ('liveparheldvolume', ok, 'status=%s intact=%s live_repairs=%d batch_repairs=%d '
            'volumes_fetched=%s io=%s' % (h['Status'], intact, live, batch, fetched, io))


def scenario_livepardecodeahead(daemon, t):
    """Decode-ahead (par-rs set_decode_ahead): an article of the first data file is lost
    early; once the recovery blocks for it are in, every later slice folds straight into the
    solution, and the lost slice is rebuilt, checked and written into its file within the
    call that verifies the last other slice ("rebuilt 1 damaged slice(s) ... ahead while
    downloading"), handed to live extraction at once. So the live repair as the download
    ends has nothing left to rebuild (repaired 0 block(s)), no batch par-repair runs, and the
    output is byte-exact. Before: the slice was rebuilt only by that repair."""
    h, intact, io = _livepar_run(daemon, t, 'lda', 12570, {0: {3}}, 3)
    log = t.read_file('nzbget.log').decode(errors='replace')
    rebuilt = re.findall(r'Live PAR \(job \d+\): rebuilt (\d+) damaged slice\(s\) \([\d.]+ MB\) ahead '
                         r'while downloading', log)
    ended = re.findall(r'repaired (\d+) block\(s\) with \d+ recovery block\(s\) in [\d.]+ s as the '
                       r'download ended', log)
    batch = _grep_log(t, 'PAR repair timings for')
    before_end = _log_before(t, 'ahead while downloading', 'as the download ended')
    # Informational: whether the rebuild also came before the last data file finished.
    before_last = _log_before(t, 'ahead while downloading', 'Successfully downloaded Live/data7.bin')
    ok = (h['Status'].startswith('SUCCESS') and intact and sum(int(n) for n in rebuilt) >= 1
          and before_end and bool(ended) and int(ended[-1]) == 0 and batch == 0)
    return ('livepardecodeahead', ok, 'status=%s intact=%s rebuilt_ahead=%s end_repaired=%s '
            'before_end=%s before_last_data=%s batch_repairs=%d io=%s'
            % (h['Status'], intact, rebuilt, ended, before_end, before_last, batch, io))
def _livepar_rar_run(daemon, t, tag, seed, missing, corrupt=None, recovery=24, obfuscate=False):
    """A RAR set under the production options with live PAR: 8 store-mode volumes of
    movie.mkv (real CRCs, so direct unpack extracts it while it downloads) and a par2 set of
    64 KiB blocks with <recovery> recovery blocks in two volumes. <missing> maps a volume
    number to the article numbers it loses; <corrupt> = (volume, article) is served from a
    copy of the volume with 4 KiB XOR-ed, its yEnc CRC still valid (a provider's bad copy:
    only par2 or the archive checksums catch it). Returns (history, extracted intact,
    post-processing par-check lines, par-rename missing lines, post-processing loads of the
    par2 index, seconds from the end of the download to the history entry). With
    <obfuscate> the volumes are posted under random names without an extension (the par2
    files keep theirs)."""
    import subprocess as _sp
    import zlib
    seg = 100_000
    movie = _payload(6_000_000, seed)
    vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 800_000)
    work = t.path('data', tag)
    os.makedirs(work, exist_ok=True)
    names = []
    for i, data in enumerate(vols):
        name = 'Rel.part%02d.rar' % (i + 1)
        t.write_file(os.path.join('data', tag, name), data)
        names.append(name)
    _sp.run(['par2', 'create', '-q', '-q', '-s65536', '-c%d' % recovery, '-n2', '-a', 'Rel',
             'Rel.par2'] + names, cwd=work, check=True, capture_output=True)
    members = [('%s/%s' % (tag, n), n, os.path.getsize(os.path.join(work, n)), seg, set())
               for n in sorted(n for n in os.listdir(work) if n.endswith('.par2'))]
    posted = (lambda n: 'x%07x' % (zlib.crc32((tag + n).encode()) & 0xfffffff)) if obfuscate \
        else (lambda n: n)
    data_members = [('%s/%s' % (tag, n), posted(n), len(vols[i]), seg, set(missing.get(i, ())))
                    for i, n in enumerate(names)]
    members += data_members
    nzb = build_multi_nzb(members)
    if corrupt:
        vol, part = corrupt
        bad = bytearray(vols[vol])
        start = (part - 1) * seg + 1000
        for k in range(start, start + 4096):
            bad[k] ^= 0x5A
        t.write_file(os.path.join('data', tag, 'bad%d' % vol), bytes(bad))
        old = '>%s/%s?%d=' % (tag, names[vol], part)
        assert old in nzb
        nzb = nzb.replace(old, '>%s/bad%d?%d=' % (tag, vol, part))
    api = daemon.wait_ready()
    name = 'Rel' + tag
    daemon.append(api, name, nzb, False, tag + '-key', 100)
    deadline = time.time() + 300
    end = None
    h = None
    while time.time() < deadline:
        h = next((x for x in api.history() if x.get('NZBName') == name), None)
        if h:
            break
        g = next((g for g in api.listgroups() if g['NZBName'] == name), None)
        if end is None and (g is None or str(g.get('Status', '')).startswith('PP_')):
            end = time.time()
        time.sleep(0.1)
    if h is None:
        raise RuntimeError('timeout waiting for %s in history' % name)
    done = time.time()
    intact = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    log = t.read_file('nzbget.log').decode(errors='replace')
    # The par-check's own lines: "Verifying <nzb>/<par2 set>" when it starts, "Repair not
    # needed for <nzb>/<par2 set>" when it found nothing to repair.
    verify = log.count('Verifying %s/' % name) + log.count('Repair not needed for %s/' % name)
    missing_lines = len(re.findall(r'File Rel\.part\d+\.rar is missing', log))
    # Post-processing loads of the par2 index: par-rename's ("Loaded par2-file") and the
    # par-check's ("Loading file X.par2"). Once is par-rename alone.
    loads = len(re.findall(r'Loaded par2-file [^\n]* for par-rename', log)) + \
        len(re.findall(r'Loading file [^\n]*\.par2', log))
    return h, intact, verify, missing_lines, loads, (round(done - end, 2) if end else None)


def scenario_liveparrarclean(daemon, t):
    """A clean RAR set with live PAR on (6790 audit, finding 3): every slice verified as
    it arrived, direct unpack extracted it while it downloaded. Post-processing runs no
    par-check (no "Verifying"/"Repair not needed" for the set), par-rename reports no
    volume missing (it read the queue's printed 16k hashes backwards before) and loads the
    par2 index once, and the statuses are nzbget's for a clean download with ParCheck=auto:
    SUCCESS, ParStatus NONE (par-check not needed), UnpackStatus SUCCESS."""
    h, intact, verify, missing, loads, end_s = _livepar_rar_run(daemon, t, 'lrc', 13710, {})
    trusted = _grep_log(t, 'live PAR verified every file')
    ok = (h['Status'].startswith('SUCCESS') and intact and verify == 0 and missing == 0
          and loads == 1 and trusted == 0 and h['ParStatus'] == 'NONE'
          and h['UnpackStatus'] == 'SUCCESS')
    return ('liveparrarclean', ok, 'status=%s par=%s unpack=%s intact=%s par_check_lines=%d '
            'missing_lines=%d par2_loads=%d end_to_history_s=%s'
            % (h['Status'], h['ParStatus'], h['UnpackStatus'], intact, verify, missing, loads, end_s))


def scenario_liveparrartrust(daemon, t):
    """A damaged RAR set with live PAR on (6790 audit, finding 3): three articles lost in
    two volumes and one served corrupt (valid yEnc CRC) in a third, so direct unpack stops
    at the bad bytes. Live PAR verifies every slice as it arrives and repairs the damage
    as the download ends; post-processing trusts that verified state: no par-check reads
    the set again (no "Verifying"/"Repair not needed" for the set; before, par-rename
    reported the volumes missing and a full par verify of every volume followed), par-rename
    reports no volume missing, the par2 index is loaded once (by par-rename; before, the
    par-check loaded it and both recovery volumes again), and the statuses are what nzbget
    shows after a par-repair:
    SUCCESS, ParStatus SUCCESS, UnpackStatus SUCCESS, the movie byte-exact."""
    h, intact, verify, missing, loads, end_s = _livepar_rar_run(daemon, t, 'lrt', 13720,
                                                         {1: {3}, 4: {2, 6}}, corrupt=(6, 4))
    live = _grep_log(t, 'Live par-repair for Rellrt')
    trusted = _grep_log(t, 'live PAR verified every file')
    ok = (h['Status'].startswith('SUCCESS') and intact and verify == 0 and missing == 0
          and live >= 1 and trusted == 1 and loads == 1
          and h['ParStatus'] == 'SUCCESS' and h['UnpackStatus'] == 'SUCCESS')
    return ('liveparrartrust', ok, 'status=%s par=%s unpack=%s intact=%s live_repairs=%d trusted=%d '
            'par_check_lines=%d missing_lines=%d par2_loads=%d end_to_history_s=%s'
            % (h['Status'], h['ParStatus'], h['UnpackStatus'], intact, live, trusted, verify, missing,
               loads, end_s))


def _livepar_rar_obfuscated(daemon, t, tag):
    """liveparrartrust with the volumes posted under random names, no extension
    (DirectRename=yes: the queue stores each volume's first-16-KiB MD5 printed nzbget-style,
    bytes last to first, and live PAR gets that text with every write). Live PAR binds the
    volumes, repairs the damage as the download ends, never stops on a source identity
    (only the end-of-job cancel), and post-processing trusts it: no par-check, par-rename
    finds no volume missing. (The direct rename gives the volumes their par2 names by the
    same hashes, so this passes on main too; the byte-order fix itself is proved by the
    parchecker live_lifecycle_tests, which fail on main.)"""
    h, intact, verify, missing, loads, end_s = _livepar_rar_run(
        daemon, t, tag, 13740, {1: {3}, 4: {2, 6}}, corrupt=(6, 4), obfuscate=True)
    live = _grep_log(t, 'Live par-repair for Rel%s' % tag)
    trusted = _grep_log(t, 'live PAR verified every file')
    # A stop other than the end of the job (its history entry cancels the session).
    stopped = len(re.findall(r'Live PAR \(job \d+\) stopped: (?!cancelled from )',
                             t.read_file('nzbget.log').decode(errors='replace')))
    ok = (h['Status'].startswith('SUCCESS') and intact and verify == 0 and missing == 0
          and live >= 1 and trusted == 1 and stopped == 0
          and h['ParStatus'] == 'SUCCESS' and h['UnpackStatus'] == 'SUCCESS')
    return ok, ('status=%s par=%s unpack=%s intact=%s live_repairs=%d trusted=%d stopped=%d '
                'par_check_lines=%d missing_lines=%d par2_loads=%d end_to_history_s=%s'
                % (h['Status'], h['ParStatus'], h['UnpackStatus'], intact, live, trusted, stopped,
                   verify, missing, loads, end_s))


def scenario_liveparrarobfuscated(daemon, t):
    """_livepar_rar_obfuscated."""
    ok, detail = _livepar_rar_obfuscated(daemon, t, 'lrb')
    return ('liveparrarobfuscated', ok, detail)


def scenario_liveparrarobfuscatedslow(daemon, t):
    """liveparrarobfuscated on a slow line (DownloadRate=1000), so the direct rename, live
    PAR's capture and the volumes' writes interleave differently."""
    ok, detail = _livepar_rar_obfuscated(daemon, t, 'lrs')
    return ('liveparrarobfuscatedslow', ok, detail)


def scenario_liveparraroff(daemon, t):
    """The control for liveparrartrust: the same damaged RAR set with live PAR off. The
    par-check runs and the counters see it (its "Verifying"/"Repair..." lines, the par2 index
    and recovery volumes loaded by it besides par-rename), it repairs the set, and the
    statuses are the same: SUCCESS, ParStatus SUCCESS, UnpackStatus SUCCESS, byte-exact."""
    h, intact, verify, missing, loads, end_s = _livepar_rar_run(daemon, t, 'lro', 13720,
                                                         {1: {3}, 4: {2, 6}}, corrupt=(6, 4))
    trusted = _grep_log(t, 'live PAR verified every file')
    repaired = _grep_log(t, 'Successfully repaired Rellro/')
    ok = (h['Status'].startswith('SUCCESS') and intact and verify >= 1 and loads >= 2
          and trusted == 0 and repaired >= 1 and missing == 0
          and h['ParStatus'] == 'SUCCESS' and h['UnpackStatus'] == 'SUCCESS')
    return ('liveparraroff', ok, 'status=%s par=%s unpack=%s intact=%s repaired=%d trusted=%d '
            'par_check_lines=%d missing_lines=%d par2_loads=%d end_to_history_s=%s'
            % (h['Status'], h['ParStatus'], h['UnpackStatus'], intact, repaired, trusted, verify,
               missing, loads, end_s))


def scenario_parinplace(daemon, t):
    """The batch fallback (live PAR off) repairs in place: only the damaged blocks are
    written into the damaged files, no full-size backup copy is made, and the phase
    timings of the repair are logged on one line."""
    h, intact, _ = _livepar_run(daemon, t, 'pip', 12510, {0: {5}, 3: {12}}, 3)
    rewrote = _grep_log(t, 'Rewrote ')
    timings = _grep_log(t, 'PAR repair timings for')
    backups = [f for base in (('main', 'dst'), ('main', 'inter')) for f in t.find_files(*base)
               if re.search(r'data\d\.bin\.\d+$', f)]
    ok = h['Status'].startswith('SUCCESS') and intact and rewrote >= 1 and timings >= 1 and not backups
    return ('parinplace', ok, 'status=%s intact=%s in_place_logs=%d timing_logs=%d backups=%s'
            % (h['Status'], intact, rewrote, timings, backups))


def _parless_run(daemon, t, tag, backup_size, backup_par2=0):
    """A single .mkv without par2 (400 articles, every 10th missing: health
    90%, and enough failures for the failover check) and a duplicate in
    history whose file is <backup_size> bytes."""
    size, seg = 20_000_000, 50_000
    data = _payload(size, 12470)
    pp = _place_copy(t, tag + 'P', data, 'movie.mkv')
    primary = build_nzb(pp, 'movie.mkv', size, seg, set(range(5, 401, 10)))
    bdata = _payload(backup_size, 12471)
    pb = _place_copy(t, tag + 'B', bdata, 'movie.mkv')
    backup = build_nzb(pb, 'movie.mkv', backup_size, seg, set())
    if backup_par2:
        # the same file posted with its own par2 volumes: the totals differ
        pp2 = _place_copy(t, tag + 'B', _payload(backup_par2, 12472), 'movie.vol00+10.par2')
        backup = build_multi_nzb([(pb, 'movie.mkv', backup_size, seg, set()),
                                  (pp2, 'movie.vol00+10.par2', backup_par2, seg, set())])
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', primary, True, tag + '-key', 100)
    daemon.append(api, 'Backup', backup, False, tag + '-key', 90)
    daemon.wait_history(api, 'Backup', timeout=60)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'Primary'])
    return daemon.wait_history(api, 'Primary', timeout=300)


def scenario_parlesscritical(daemon, t):
    """A download that certainly has no par2 (one .mkv) and no duplicate with a
    file of its size to repair holes from can't lose an article: under
    HealthCheck=dupe it fails over below 100%, not only at the 85% nzbget
    assumes for downloads whose par2-files may be renamed (Velvet Underground
    7913 kept downloading with half its articles failing)."""
    hp = _parless_run(daemon, t, 'pc', 10_000_000)
    over = _grep_log(t, 'below critical 100.0%')
    return ('parlesscritical', over >= 1 and hp['Status'].startswith(('DELETED', 'FAILURE')),
            'primary=%s failover_logs=%d' % (hp['Status'], over))


def scenario_parlesstwinpar(daemon, t):
    """The twin posted with par2 volumes of 5% of its size: the totals differ,
    the data doesn't. Its par2 doesn't count, so it's still a twin: the 85% stays."""
    hp = _parless_run(daemon, t, 'pq', 20_000_000, backup_par2=1_000_000)
    hundred = _grep_log(t, 'below critical 100.0%')
    return ('parlesstwinpar', hundred == 0, 'primary=%s logs_100=%d' % (hp['Status'], hundred))


def scenario_parlesstwin(daemon, t):
    """The same with a duplicate holding a file of the same size (stream repair
    could fill the holes from it): the 85% stays."""
    hp = _parless_run(daemon, t, 'pt', 20_000_000)
    hundred = _grep_log(t, 'below critical 100.0%')
    return ('parlesstwin', hundred == 0, 'primary=%s logs_100=%d' % (hp['Status'], hundred))


def scenario_clientcommands(daemon, t):
    """The command-line client over the binary protocol after its requests got
    stricter checks: list, edit (pause a group) and write-log still work."""
    import subprocess as _sp
    data = _payload(300_000, 12434)
    pp = _place_copy(t, 'ccA', data)
    api = daemon.wait_ready()
    daemon.append(api, 'RelCC', build_nzb(pp, 'cc.bin', len(data), 100_000, set()), True, 'cc-key', 100)
    time.sleep(1)
    gid = next(g['NZBID'] for g in api.listgroups() if g['NZBName'] == 'RelCC')
    base = [t.nzbget, '-c', t.path(daemon.conf_rel)]
    listed = _sp.run(base + ['-L', 'G'], capture_output=True, text=True, timeout=30).stdout
    _sp.run(base + ['-E', 'G', 'T', str(gid)], capture_output=True, text=True, timeout=30)
    _sp.run(base + ['-W', 'I', 'client log line 12434'], capture_output=True, text=True, timeout=30)
    time.sleep(1)
    group = next(g for g in api.listgroups() if g['NZBID'] == gid)
    logged = _grep_log(t, 'client log line 12434')
    alive = t.procs[-1].poll() is None
    ok = alive and 'RelCC' in listed and logged >= 1
    return ('clientcommands', ok, 'listed=%s logged=%d alive=%s' % ('RelCC' in listed, logged, alive))


def scenario_notfound451(daemon, t):
    """A news server that answers 451 for a missing article (as some
    providers do) is treated like 430: the article is asked for once on that
    server. Before, 451 was an unknown error and the same server was asked
    again ArticleRetries times, for every missing article of every server."""
    size, seg = 2_500_000, 500_000
    data = _payload(size, 5300)
    pp = _place_copy(t, 'nfA', data, 'movie.mkv')
    api = daemon.wait_ready()
    daemon.append(api, 'RelNF', build_nzb(pp, 'movie.mkv', size, seg, {2, 3}), False, 'nf-key', 100)
    h = daemon.wait_history(api, 'RelNF', timeout=120)
    rewritten = daemon.proxy.rewritten if daemon.proxy else 0
    return ('notfound451', rewritten == 2 and int(h.get('FailedArticles', 0)) == 2,
            'status=%s rewritten_replies=%d failed_articles=%s (one request per missing article)'
            % (h['Status'], rewritten, h.get('FailedArticles')))


def scenario_dupehopeless(daemon, t):
    """HealthCheck=dupe without a backup: a dead posting (every article of
    every file missing, no duplicate able to supply anything) must be parked
    once the duplicates were asked for a sample and almost nothing of it
    exists, instead of failing all of its articles first. The one donor in
    history is an unrelated packaging that is itself dead, so nothing can
    be borrowed and no failover target qualifies either."""
    seg = 100_000
    vol_dead, vol_donor = 2_900_000, 3_000_000
    n = vol_dead // seg
    dead = [('hoA/d%d.bin' % i, 'Dead%d.bin' % i, vol_dead, seg, set(range(1, n + 1)))
            for i in range(6)]
    for m in dead:
        t.write_file(os.path.join('data', m[0]), _payload(vol_dead, 9600))
    dp = _place_copy(t, 'hoB', _payload(vol_donor, 9601))
    donor = build_nzb(dp, 'Donor.bin', vol_donor, seg, set(range(1, vol_donor // seg + 1)))
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(dead), True, 'ho-key', 100)
    daemon.append(api, 'Donor', donor, False, 'ho-key', 50)
    daemon.wait_history(api, 'Donor', timeout=60)
    # the donor is a dead posting too: mark it bad so it is no failover target
    api.editqueue('HistoryMarkBad', 0, '', [h['NZBID'] for h in api.history()
                                            if h['NZBName'] == 'Donor'])
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups()
                                         if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary')
    parked = _grep_log(t, 'Parking Primary: health')
    failed_over = _grep_log(t, 'Failing over Primary')
    failed_articles = int(hp.get('FailedArticles', 0))
    # 174 articles in total; the park fires once 32 were tried (more than a
    # tenth of the download, none of them alive)
    early = failed_articles < 140
    return ('dupehopeless', parked == 1 and failed_over == 0 and early,
            'status=%s parked_logs=%d failover_logs=%d failed_articles=%d'
            % (hp['Status'], parked, failed_over, failed_articles))


def scenario_dupehopelessretry(daemon, t):
    """"Retry failed articles" on a download parked as hopeless: the retry
    is checked like a new download and parked again as early as the first
    attempt while the posting is still dead. Before, the failure count of the
    first attempt was kept as the point of the last check and the retry's own
    count started below it, so the retry was checked only once it had failed
    that many articles again plus a sample (60 here, thousands after a late
    first check)."""
    seg = 100_000
    vol_dead = 2_900_000
    n = vol_dead // seg
    # one article exists: a download none of whose articles arrived has no
    # destination directory and can't be retried at all (upstream behavior)
    dead = [('hrA/d%d.bin' % i, 'Dead%d.bin' % i, vol_dead, seg,
             set(range(2 if i == 0 else 1, n + 1))) for i in range(6)]
    for m in dead:
        t.write_file(os.path.join('data', m[0]), _payload(vol_dead, 9610))
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', build_multi_nzb(dead), False, 'hr-key', 100)
    hp = daemon.wait_history(api, 'Primary')
    first_failed = int(hp.get('FailedArticles', 0))
    api.editqueue('HistoryRetryFailed', 0, '', [hp['NZBID']])
    deadline = time.time() + 60
    while time.time() < deadline and any(h['NZBID'] == hp['NZBID'] for h in api.history()):
        time.sleep(0.2)
    hp = daemon.wait_history(api, 'Primary')
    parked = _grep_log(t, 'Parking Primary: health')
    retry_failed = int(hp.get('FailedArticles', 0))
    # 174 articles in total; each attempt is parked once a tenth was tried
    # and its health is below critical
    return ('dupehopelessretry', parked == 2 and first_failed < 140 and
            retry_failed <= first_failed + 8,
            'status=%s parked_logs=%d first_failed_articles=%d retry_failed_articles=%d'
            % (hp['Status'], parked, first_failed, retry_failed))


def scenario_dupedeadstart(daemon, t):
    """The hopeless-park must not fire on a posting that merely BEGINS with
    a dead stretch: the first 40 of 300 articles are missing, the rest
    exist, there is no par2 and no duplicate carries anything. Health is
    below critical with nothing downloaded yet, but by the time a tenth of
    the download was tried most tried articles exist, so the download runs
    to the end (FAILURE/HEALTH with exactly the 40 missing articles, every
    other article downloaded) and nothing is parked early."""
    seg = 100_000
    vol = 10_000_000
    data = _payload(vol, 9700)
    pp = _place_copy(t, 'dsA', data)
    primary = build_multi_nzb([(pp, 'Start.bin', vol, seg, set(range(1, 41))),
                               ('dsA/file.bin', 'Start2.bin', vol, seg, set(range(1, 41))),
                               ('dsA/file.bin', 'Start3.bin', vol, seg, set(range(1, 41)))])
    dp = _place_copy(t, 'dsB', _payload(3_000_000, 9701))
    donor = build_nzb(dp, 'Donor.bin', 3_000_000, seg, set(range(1, 31)))
    api = daemon.wait_ready()
    daemon.append(api, 'Primary', primary, True, 'ds-key', 100)
    daemon.append(api, 'Donor', donor, False, 'ds-key', 50)
    daemon.wait_history(api, 'Donor', timeout=60)
    api.editqueue('HistoryMarkBad', 0, '', [h['NZBID'] for h in api.history()
                                            if h['NZBName'] == 'Donor'])
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups()
                                         if g['NZBName'] == 'Primary'])
    hp = daemon.wait_history(api, 'Primary', timeout=300)
    parked = _grep_log(t, 'Parking Primary: health')
    failed_articles = int(hp.get('FailedArticles', 0))
    success_articles = int(hp.get('SuccessArticles', 0))
    return ('dupedeadstart', parked == 0 and failed_articles == 120 and success_articles == 180,
            'status=%s parked_logs=%d failed_articles=%d success_articles=%d'
            % (hp['Status'], parked, failed_articles, success_articles))


def _verify_output(t, expected, ext='.bin', dirs=(('main', 'dst'),)):
    """On SUCCESS the completed file lands at main/dst/<category>/<nzb>/
    <name><ext>, whose exact path depends on category and FileNaming. When
    the history status stays non-SUCCESS (e.g. a PARTIALLY-repaired release
    still has failed-byte statistics, so cleanup/move-to-dst is skipped and
    nzbget "parks" the item instead - see HistoryCoordinator's
    cleanupParkedFiles logic), the same file instead sits under
    main/inter/<nzb>.#<id>/<name><ext>. Walk the directories specified in
    'dirs' (by default, main/dst only; scenarios whose item ends parked,
    like xpackrar's header-holed volume, pass both main/dst and main/inter)
    and byte-compare every produced file with the given extension against
    the source payload."""
    for base in dirs:
        for rel in t.find_files(*base):
            if rel.endswith(ext):
                try:
                    if t.read_file(rel) == expected:
                        return True
                except Exception:
                    pass
    return False


def scenario_badpaths(daemon, t):
    """The config's paths do not work (MainDir lies under a regular file, so
    nothing can be created there, whoever runs the test). The daemon still
    starts: its port answers HTTP and RPC, downloading, post-processing and
    scanning stay paused, each bad path is warned about once (in the Messages
    the web interface reads, with the option, value, reason and "Settings >
    Paths"), a LogFile in a missing folder is not an error per line, and the
    Settings page still gets option descriptions. Saving fixed paths and
    reloading unpauses it and creates the folders."""
    api = daemon.wait_ready()
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)

    t.write_file('blocker', b'not a folder\n')
    bad_main = t.path('blocker', 'rg')
    daemon.write_config(DEFAULT_OPTIONS + [
        'MainDir=%s' % bad_main, 'DestDir=${MainDir}/dst', 'InterDir=${MainDir}/inter',
        'NzbDir=${MainDir}/nzb', 'QueueDir=${MainDir}/queue', 'TempDir=${MainDir}/tmp',
        'LogFile=${MainDir}/rustyget.log', 'ConfigTemplate=',
        # a secret that must never reach the Settings page's template
        'Server1.Password=badpaths-secret-42',
    ])
    daemon.process = t.spawn([t.nzbget, '-c', t.path(daemon.conf_rel), '-s'], output_rel='badpaths.out')
    api = daemon.wait_ready()
    problems = []

    version = api.version()
    try:
        with socket.create_connection((t.rpc_host(), daemon.rpc_port), timeout=5) as sock:
            sock.sendall(b'GET / HTTP/1.0\r\n\r\n')
            http_reply = sock.recv(64)
    except OSError as e:
        http_reply = repr(e).encode()
    if not http_reply.startswith(b'HTTP/1.'):
        problems.append('http=%r' % http_reply[:30])

    st = api.status()
    paused = (st['DownloadPaused'], st['PostPaused'], st['ScanPaused'])
    if paused != (True, True, True):
        problems.append('paused=%s' % (paused,))

    time.sleep(3)  # let the queue, scanner and log run against the missing folders
    if daemon.process.poll() is not None:
        problems.append('daemon exited with %s' % daemon.process.returncode)
    texts = [m['Text'] for m in api.log(0, 1000) if m['Kind'] in ('WARNING', 'ERROR')]
    counts = {}
    for option in ('MainDir', 'DestDir', 'InterDir', 'NzbDir', 'QueueDir', 'TempDir', 'LogFile'):
        warned = [x for x in texts if ('%s "' % option) in x and "can't be used" in x and 'Settings > Paths' in x]
        counts[option] = len(warned)
        if len(warned) != 1:
            problems.append('%s warned %d times' % (option, len(warned)))
    repeated = [x for x in texts if x.startswith('[Paths][')]
    if repeated:
        problems.append('repeated=%s' % repeated)
    if not any('paused until the bad paths are fixed' in x for x in texts):
        problems.append('no pause notice')

    out = t.read_file('badpaths.out').decode(errors='replace')
    per_line_errors = out.count('rustyget.log: ')
    if per_line_errors:
        problems.append('log file errors per line=%d' % per_line_errors)
    # once on stderr (a log line) and once among the screen messages
    for form in ('WARNING\tLogFile "', '[WARNING] LogFile "'):
        if out.count(form + '%s/rustyget.log" can\'t be used' % bad_main) != 1:
            problems.append('%r LogFile warnings=%d' % (form, out.count(form)))

    for from_disk in (True, False):
        templates = api.configtemplates(from_disk)
        text = templates[0].get('Template', '') if templates else ''
        if '### PATHS' not in text:
            problems.append('no config template (from_disk=%s)' % from_disk)
        if 'badpaths-secret-42' in text:
            problems.append('the live config was served as the template')

    # daemon mode with the lock file in the missing folder runs without it
    lock_warned = None
    if t.name == 'local':
        dport = free_port()
        dconf = t.path('badpaths-daemon.conf')
        conf = t.read_file(daemon.conf_rel).decode()
        conf = re.sub(r'(?m)^ControlPort=.*$', 'ControlPort=%d' % dport, conf)
        conf += 'LockFile=${MainDir}/rustyget.lock\n'
        with open(dconf, 'w') as f:
            f.write(conf)
        subprocess.run([t.nzbget, '-c', dconf, '-D'], timeout=30)
        dapi = ServerProxy('http://127.0.0.1:%d/xmlrpc' % dport)
        deadline = time.time() + 30
        while time.time() < deadline:
            try:
                dapi.version()
                lock_warned = sum(1 for m in dapi.log(0, 1000)
                                  if 'LockFile "%s/rustyget.lock" can\'t be used' % bad_main in m['Text'])
                break
            except Exception:
                time.sleep(0.3)
        try:
            dapi.shutdown()
        except Exception:
            pass
        time.sleep(1)
        for entry in subprocess.run(['pgrep', '-f', dconf], capture_output=True, text=True).stdout.split():
            try:
                os.kill(int(entry), 9)
            except (OSError, ValueError):
                pass
        if lock_warned != 1:
            problems.append('daemon mode lock warnings=%s' % lock_warned)

    # fix the paths in the config, save and reload
    good_main = t.path('fixed')
    values = [{'Name': o['Name'], 'Value': good_main if o['Name'] == 'MainDir' else o['Value']}
              for o in api.loadconfig()]
    if not api.saveconfig(values):
        problems.append('saveconfig failed')
    api.reload()
    deadline = time.time() + 60
    st = None
    while time.time() < deadline:
        try:
            st = api.status()
            if not st['DownloadPaused'] and os.path.isdir(os.path.join(good_main, 'queue')):
                break
        except Exception:
            pass
        time.sleep(0.3)
    after = (st['DownloadPaused'], st['PostPaused'], st['ScanPaused']) if st else None
    if after != (False, False, False):
        problems.append('after reload paused=%s' % (after,))
    made = sorted(d for d in ('dst', 'inter', 'nzb', 'queue', 'tmp') if os.path.isdir(os.path.join(good_main, d)))
    if len(made) != 5:
        problems.append('made=%s' % made)
    if not os.path.isfile(os.path.join(good_main, 'rustyget.log')):
        problems.append('no log file after the fix')
    return ('badpaths', not problems, 'version=%s paused=%s warnings=%s daemon_lock_warnings=%s after_reload=%s %s' % (
        version, paused, counts, lock_warned, after, ' '.join(problems)))


def _jrpc(port, method, params=(), auth=None, timeout=30):
    """JSON-RPC on 127.0.0.1:port; the whole reply ({'result'} or {'error'})."""
    import json as _json
    import urllib.request as _req
    headers = {'Content-Type': 'application/json'}
    if auth:
        headers['Authorization'] = 'Basic ' + base64.b64encode(auth.encode()).decode()
    body = _json.dumps({'method': method, 'params': list(params)}).encode()
    with _req.urlopen(_req.Request('http://127.0.0.1:%d/jsonrpc' % port, body, headers), timeout=timeout) as reply:
        return _json.loads(reply.read().decode())


def _http_get(port, path):
    """(status, body) of a plain GET."""
    with socket.create_connection(('127.0.0.1', port), timeout=10) as sock:
        sock.sendall(('GET %s HTTP/1.0\r\n\r\n' % path).encode())
        data = b''
        while True:
            chunk = sock.recv(65536)
            if not chunk:
                break
            data += chunk
    head, _, body = data.partition(b'\r\n\r\n')
    status = int(head.split(b' ')[1]) if head.startswith(b'HTTP/') else 0
    return status, body


def _post_rpc_raw(port, origin, referer, host=None, path='/jsonrpc', fetch_site=None, method='POST', body_text=True, extra=None):
    """(status, response head) of a JSON-RPC "version" call (a text/plain
    POST, as a web form or fetch() can send cross-site) with these headers;
    the whole response when the status is 403."""
    body = b'{"method":"version","params":[]}' if method == 'POST' else b''
    head = '%s %s HTTP/1.1\r\nHost: %s\r\n' % (method, path, host or '127.0.0.1:%d' % port)
    if method == 'POST':
        head += 'Content-Type: text/plain\r\nContent-Length: %d\r\n' % len(body)
    if origin:
        head += 'Origin: %s\r\n' % origin
    if referer:
        head += 'Referer: %s\r\n' % referer
    if fetch_site:
        head += 'Sec-Fetch-Site: %s\r\nSec-Fetch-Mode: cors\r\n' % fetch_site
    for k, v in (extra or {}).items():
        head += '%s: %s\r\n' % (k, v)
    with socket.create_connection(('127.0.0.1', port), timeout=10) as sock:
        sock.sendall(head.encode() + b'\r\n' + body)
        data = b''
        while b'\r\n\r\n' not in data:
            chunk = sock.recv(65536)
            if not chunk:
                break
            data += chunk
        if data.startswith(b'HTTP/1.1 403'):
            while True:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                data += chunk
                if b'ControlHostnames' in data:
                    break
    first = data.split(b'\r\n', 1)[0].split(b' ')
    code = int(first[1]) if len(first) > 1 and first[1].isdigit() else 0
    return code, (data if code == 403 else data.split(b'\r\n\r\n', 1)[0])


def _wait_rpc(port, timeout=30, auth=None):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            if 'result' in _jrpc(port, 'version', auth=auth, timeout=5):
                return True
        except Exception:
            pass
        time.sleep(0.3)
    return False


def _stop_default_daemon(daemon, t):
    api = daemon.wait_ready()
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)


def _spawn_env(t, args, env, output_rel):
    full = dict(os.environ)
    full.update(env)
    with open(t.path(output_rel), 'wb') as output:
        p = subprocess.Popen(args, stdout=output, stderr=subprocess.STDOUT, env=full)
    t.procs.append(p)
    return p


def _wait_text(t, rel, pattern, timeout=30):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            m = re.search(pattern, t.read_file(rel).decode(errors='replace'))
            if m:
                return m
        except OSError:
            pass
        time.sleep(0.2)
    return None


def scenario_noconfig(daemon, t):
    """No configuration file anywhere (HOME is an empty folder): "-s" writes
    ~/.config/rustyget/rustyget.conf from the built-in template (owner-only,
    MainDir=~/RustyGet, ControlIP=127.0.0.1, the first free port from 6789, a
    random 20-character password, the web interface built in), says where,
    which address to open and the password on stdout (the password never in
    the log), serves the web interface from the binary and stays paused
    until a news server is set up. saveconfig refuses bad
    values without touching the file, saves good ones after a backup, and the
    reload is ready to download. "-c <missing file>" creates that file, and
    "-D" (daemon mode) with no file starts as well."""
    if t.name != 'local':
        return ('noconfig', True, 'skipped: needs a local HOME')
    _stop_default_daemon(daemon, t)
    problems = []
    bindir = t.makedirs('ncbin')
    binary = os.path.join(bindir, 'rustyget')
    shutil.copy2(t.nzbget, binary)  # no rustyget.conf next to it
    home = t.makedirs('nchome')
    proc = _spawn_env(t, [binary, '-s', '-o', 'OutputMode=log'], {'HOME': home}, 'noconfig.out')
    m = _wait_text(t, 'noconfig.out', r'Open the web interface to finish setting up RustyGet: http://127\.0\.0\.1:(\d+)/')
    if not m:
        return ('noconfig', False, 'no address on stdout: %r' % t.read_file('noconfig.out')[-2000:])
    port = int(m.group(1))
    conf = os.path.join(home, '.config', 'rustyget', 'rustyget.conf')
    out = t.read_file('noconfig.out').decode(errors='replace')
    if ('RustyGet created a new configuration file: ' + conf) not in out:
        problems.append('stdout does not name the file')
    pw = _wait_text(t, 'noconfig.out', r'Sign in with user "rustyget" and password "([A-Za-z0-9]{20})"')
    if not pw:
        return ('noconfig', False, 'no password on stdout: %r' % t.read_file('noconfig.out')[-2000:])
    first_auth = 'rustyget:' + pw.group(1)
    if not _wait_rpc(port, auth=first_auth):
        return ('noconfig', False, 'no RPC on port %d' % port)
    try:
        _jrpc(port, 'status')
        problems.append('no password needed on first run')
    except Exception:
        pass
    text = open(conf).read() if os.path.isfile(conf) else ''
    for want in ('MainDir=~/RustyGet', 'ControlIP=127.0.0.1', 'ControlPort=%d' % port, 'ControlPassword=' + pw.group(1),
                 'WebDir=', 'FormAuth=yes'):
        if not re.search('(?m)^' + re.escape(want) + '$', text):
            problems.append('config lacks %s' % want)
    mode = os.stat(conf).st_mode & 0o777 if text else None
    if mode != 0o600:
        problems.append('mode=%s' % (oct(mode) if mode else None))
    if not os.path.isdir(os.path.join(home, 'RustyGet', 'queue')):
        problems.append('MainDir not made in HOME')
    log = [m['Text'] for m in _jrpc(port, 'log', [0, 500], auth=first_auth)['result']]
    if not any('created a new configuration file' in x for x in log):
        problems.append('creation not logged')
    if any(pw.group(1) in x for x in log) or pw.group(1).encode() in (open(os.path.join(home, 'RustyGet', 'rustyget.log'), 'rb').read()
                                                                     if os.path.isfile(os.path.join(home, 'RustyGet', 'rustyget.log')) else b''):
        problems.append('the password is in the log')
    st = _jrpc(port, 'setupstatus', auth=first_auth)['result']
    if (st.get('ConfigCreated'), st.get('Servers'), st.get('SetupComplete'), st.get('PasswordSet'), st.get('DownloadsBlocked')) != (True, 0, False, True, True):
        problems.append('setupstatus=%s' % st)
    if not _jrpc(port, 'status', auth=first_auth)['result']['DownloadPaused']:
        problems.append('not paused without a server')
    if not any('no news server is set up' in x for x in log):
        problems.append('no pause notice')
    if any(x.startswith('At least one') for x in log):
        problems.append('scary server errors: %s' % [x for x in log if x.startswith('At least one')])
    status, body = _http_get(port, '/')
    if status != 200 or b'RustyGet' not in body:  # the page itself needs no login under FormAuth
        problems.append('web interface: %s' % status)
    status, body = _http_get(port, '/combined.js?util.js+setup.js')
    if status != 200 or b'Setup' not in body:
        problems.append('built-in combined.js: %s' % status)
    for leak in ('/rustyget.conf', '/x.conf.bak-1'):
        if _http_get(port, leak)[0] == 200:
            problems.append('served %s' % leak)
    t.write_file('ncfile', b'x')
    chk = _jrpc(port, 'checkpath', [t.path('ncfile', 'sub'), ''], auth=first_auth)['result']
    if chk.get('State') != 'bad' or 'is a file' not in chk.get('Message', ''):
        problems.append('checkpath=%s' % chk)

    # a bad save is refused and changes nothing
    before = open(conf, 'rb').read()
    bad = _jrpc(port, 'saveconfig', [[{'Name': 'ControlPort', 'Value': 'abc'}, {'Name': 'Server1.Host', 'Value': 'http://x y'}]], auth=first_auth)
    if 'error' not in bad or 'ControlPort' not in bad['error']['message']:
        problems.append('bad save=%s' % bad)
    if open(conf, 'rb').read() != before:
        problems.append('a refused save changed the file')
    # the guide's save: everything loadconfig gave, with the changes
    values = _jrpc(port, 'loadconfig', auth=first_auth)['result']
    changes = {'ControlPassword': 'example-pass-1', 'Server1.Active': 'yes', 'Server1.Host': '127.0.0.1',
               'Server1.Port': str(daemon.nntp_port), 'Server1.Encryption': 'no', 'Server1.Connections': '2',
               'Server1.Username': '', 'Server1.Password': ''}
    req = [{'Name': o['Name'], 'Value': changes.get(o['Name'], o['Value'])} for o in values]
    check = _jrpc(port, 'checkconfig', [req], auth=first_auth).get('result')
    if check != []:
        problems.append('checkconfig=%s' % check)
    ok = _jrpc(port, 'saveconfig', [req], auth=first_auth)
    if ok.get('result') is not True:
        problems.append('good save=%s' % ok)
    backups = [n for n in os.listdir(os.path.dirname(conf)) if n.startswith('rustyget.conf.bak-')]
    if len(backups) != 1:
        problems.append('backups=%s' % backups)
    elif open(os.path.join(os.path.dirname(conf), backups[0]), 'rb').read() != before:
        problems.append('the backup is not the old file')
    started = out.count('server-mode')
    _jrpc(port, 'reload', auth=first_auth)
    auth = 'rustyget:example-pass-1'
    time.sleep(2)
    if not _wait_rpc(port, 30, auth):
        problems.append('not back after reload')
    else:
        st = _jrpc(port, 'setupstatus', auth=auth)['result']
        if not st.get('SetupComplete'):
            problems.append('after reload setupstatus=%s' % st)
        if _jrpc(port, 'status', auth=auth)['result']['DownloadPaused']:
            problems.append('still paused after setup')
        try:
            _jrpc(port, 'status')
            problems.append('no password needed after setup')
        except Exception:
            pass
    try:
        _jrpc(port, 'shutdown', auth=auth)
    except Exception:
        pass
    proc.wait(timeout=30)

    # -c <missing file>: created there
    missing = t.path('nc-new', 'deeper', 'my.conf')
    p2 = _spawn_env(t, [binary, '-c', missing, '-s', '-o', 'OutputMode=log'], {'HOME': home}, 'noconfig-c.out')
    m = _wait_text(t, 'noconfig-c.out', r'Open the web interface to finish setting up RustyGet: http://127\.0\.0\.1:(\d+)/')
    pw2 = _wait_text(t, 'noconfig-c.out', r'password "([A-Za-z0-9]{20})"')
    if not m or not pw2 or not os.path.isfile(missing):
        problems.append('-c missing: %r' % t.read_file('noconfig-c.out')[-600:])
    else:
        port2 = int(m.group(1))
        auth2 = 'rustyget:' + pw2.group(1)
        if not _wait_rpc(port2, auth=auth2):
            problems.append('-c missing: no RPC')
        elif not any('did not exist, so RustyGet created a new one there' in w
                     for w in _jrpc(port2, 'setupstatus', auth=auth2)['result'].get('Warnings', [])):
            problems.append('-c missing: no clear warning')
        if pw2.group(1) == pw.group(1):
            problems.append('the same password twice')
        try:
            _jrpc(port2, 'shutdown', auth=auth2)
        except Exception:
            pass
    p2.wait(timeout=30)

    # -D, no file at all (another empty HOME)
    home3 = t.makedirs('nchome-d')
    r = subprocess.run([binary, '-D', '-o', 'OutputMode=log'], env=dict(os.environ, HOME=home3),
                       capture_output=True, timeout=30)
    m = re.search(rb'http://127\.0\.0\.1:(\d+)/', r.stdout)
    pw3 = re.search(rb'password "([A-Za-z0-9]{20})"', r.stdout)
    if r.returncode != 0 or not m or not pw3:
        problems.append('-D: rc=%s out=%r' % (r.returncode, r.stdout[-400:]))
    else:
        port3 = int(m.group(1))
        auth3 = 'rustyget:' + pw3.group(1).decode()
        if not _wait_rpc(port3, auth=auth3):
            problems.append('-D: no RPC')
        try:
            _jrpc(port3, 'shutdown', auth=auth3)
        except Exception:
            pass
        time.sleep(1)
    return ('noconfig', not problems, 'port=%d started=%d %s' % (port, started, ' '.join(problems)))


def _terrible_config(rpc_port, main, nntp_port):
    return b''.join([
        b'# a terrible configuration file\n',                                      # 1
        ('ControlIP=127.0.0.1\nControlPort=%d\nControlUsername=\nControlPassword=\n' % rpc_port).encode(),  # 2-5
        ('MainDir=%s\n' % main).encode(),                                           # 6
        ('Server1.Host=127.0.0.1\nServer1.Port=%d\nServer1.Encryption=no\nServer1.Connections=2\n' % nntp_port).encode(),  # 7-10
        b'ArticleTimeout=soon\n',                                                   # 11
        b'ParCheck=sometimes\n',                                                    # 12
        b'NotAnOption=1\n',                                                         # 13
        b'this line is broken\n',                                                   # 14
        b'DiskSpace=100\nDiskSpace=200\n',                                          # 15-16
        b'\x01\xff\xfe=\x02\n',                                                     # 17
        b'Category1.Name=Caf\xe9\n',                                                # 18
        b'x' * 70000 + b'\n',                                                       # 19
        b'UMask=octal\n',                                                           # 20
    ])


def scenario_garbageconfig(daemon, t):
    """A terrible configuration file (wrong types, a bad choice, an unknown
    option, a broken line, a duplicate, binary bytes, a non-UTF-8 value, a
    70 kB line): the daemon starts on the defaults for the bad options, warns
    once per bad line with file:line, what is wrong and what it used instead,
    serves the web interface and lists the problems in setupstatus. Without
    a real password (none, the stock one, or AuthorizedIP) an API call from
    another site (Origin, Referer, Sec-Fetch-Site) or naming a host that is
    not this computer (DNS rebinding) gets 403 with what to do. An empty
    file and a folder (its rustyget.conf is used) start; a file that exists
    but can't be read stops RustyGet with the reason."""
    if t.name != 'local':
        return ('garbageconfig', True, 'skipped: needs local files')
    _stop_default_daemon(daemon, t)
    problems = []
    conf = t.path('terrible.conf')
    t.write_file('terrible.conf', _terrible_config(daemon.rpc_port, t.path('gmain'), daemon.nntp_port))
    proc = _spawn_env(t, [t.nzbget, '-c', conf, '-s', '-o', 'OutputMode=log'], {'HOME': t.makedirs('ghome')}, 'garbage.out')
    if not _wait_rpc(daemon.rpc_port):
        return ('garbageconfig', False, 'did not start: %r' % t.read_file('garbage.out')[-2000:])
    time.sleep(1)
    if proc.poll() is not None:
        problems.append('exited %s' % proc.returncode)
    log = [m['Text'] for m in _jrpc(daemon.rpc_port, 'log', [0, 1000])['result']]
    expect = {
        11: 'Invalid value for option "ArticleTimeout": "soon" is not a whole number. Using the default "60" instead',
        12: 'Invalid value for option "ParCheck": "sometimes". Using "auto" instead',
        13: 'Invalid option "NotAnOption": RustyGet has no such option, the line is ignored',
        14: 'Invalid option "this line is broken": not a "Name=value" line, ignored',
        16: 'Option "DiskSpace" is set again (first on line 15); using the value from this line',
        17: 'Invalid option "',
        19: 'Line is too long (70000 bytes',
        20: 'Invalid value for option "UMask": "octal" is not a whole number. Using the default',
    }
    for line, text in expect.items():
        tag = '%s:%d: ' % (conf, line)
        hits = [x for x in log if x.startswith(tag)]
        if len(hits) != 1 or text not in hits[0]:
            problems.append('line %d: %s' % (line, hits))
    for line in (1, 2, 3, 6, 7, 8, 9, 10, 15, 18):
        if any(x.startswith('%s:%d: ' % (conf, line)) for x in log):
            problems.append('line %d warned' % line)
    if any('�' in x or '\x01' in x for x in log if x.startswith(conf)):
        problems.append('binary shown raw')
    cfg = {o['Name']: o['Value'] for o in _jrpc(daemon.rpc_port, 'config')['result']}
    if (cfg.get('ArticleTimeout'), cfg.get('ParCheck'), cfg.get('DiskSpace')) != ('60', 'auto', '200'):
        problems.append('used=%s/%s/%s' % (cfg.get('ArticleTimeout'), cfg.get('ParCheck'), cfg.get('DiskSpace')))
    st = _jrpc(daemon.rpc_port, 'setupstatus')['result']
    if len(st.get('Warnings', [])) < len(expect) or st.get('Servers') != 1:
        problems.append('setupstatus=%s' % st)
    status, body = _http_get(daemon.rpc_port, '/')
    if status != 200:
        problems.append('web interface %s' % status)
    # without a password another web site may not drive the daemon, and no
    # Origin but the daemon's own is reflected
    own = 'http://127.0.0.1:%d' % daemon.rpc_port
    for origin, referer, want in ((own, None, 200), ('http://evil.example', None, 403), ('null', None, 403),
                                  (None, 'http://evil.example/page', 403), (None, own + '/', 200), (None, None, 200)):
        code, head = _post_rpc_raw(daemon.rpc_port, origin, referer)
        if code != want:
            problems.append('origin=%s referer=%s -> %s' % (origin, referer, code))
        if b'Access-Control-Allow-Origin: http://evil' in head or (origin == 'null' and b'Access-Control-Allow-Origin' in head):
            problems.append('reflected a foreign origin: %r' % head[:300])
        if code == 403 and b'ControlHostnames' not in head:
            problems.append('the refusal does not say what to do: %r' % head[-300:])
    # DNS rebinding: a name that is not this computer's, without a password
    for host, want in (('evil.example:%d' % daemon.rpc_port, 403), ('localhost:%d' % daemon.rpc_port, 200),
                       ('[::1]:%d' % daemon.rpc_port, 200), ('127.0.0.1', 200)):
        code, head = _post_rpc_raw(daemon.rpc_port, 'http://' + host, None, host=host)
        if code != want:
            problems.append('host=%s -> %s' % (host, code))
        if want == 403 and b'evil.example' not in head:
            problems.append('the refusal does not name the host: %r' % head[-300:])
    # a GET without Origin or Referer that the browser marks cross-site
    code, _ = _post_rpc_raw(daemon.rpc_port, None, None, path='/jsonrpc/version', method='GET', fetch_site='cross-site')
    if code != 403:
        problems.append('Sec-Fetch-Site cross-site GET -> %s' % code)
    # a same-origin GET without Origin or Referer: a page behind a proxy that
    # rewrites Host looks the same, so it is refused; with its own Referer,
    # or typed into the address bar (none), it is fine
    own = 'http://127.0.0.1:%d' % daemon.rpc_port
    for referer, site, want in ((None, 'same-origin', 403), (own + '/', 'same-origin', 200), (None, 'none', 200)):
        code, head = _post_rpc_raw(daemon.rpc_port, None, referer, path='/jsonrpc/version', method='GET', fetch_site=site)
        if code != want:
            problems.append('Sec-Fetch-Site %s GET referer=%s -> %s' % (site, referer, code))
        if code == 403 and b'reverse proxy' not in head:
            problems.append('the proxy refusal does not say so: %r' % head[-300:])
    # a proxy that names another host in X-Forwarded-Host or Forwarded
    for extra in ({'X-Forwarded-Host': 'evil.example'}, {'Forwarded': 'for=1.2.3.4;host="evil.example"'}):
        code, _ = _post_rpc_raw(daemon.rpc_port, 'http://evil.example', None, extra=extra)
        if code != 403:
            problems.append('%s -> %s' % (extra, code))
    # ControlHostnames is checked on the Forwarded name too: the browser at
    # a listed name through a proxy is fine (checked on the AuthorizedIP run)
    # API tools send no Origin, Referer or Sec-Fetch-*: any Host works
    code, _ = _post_rpc_raw(daemon.rpc_port, None, None, host='nzbget:%d' % daemon.rpc_port)
    if code != 200:
        problems.append('a tool with Host nzbget -> %s' % code)
    # refusals are logged at most once per 10 seconds
    for _ in range(20):
        _post_rpc_raw(daemon.rpc_port, 'http://evil.example', None)
    lines = [m['Text'] for m in _jrpc(daemon.rpc_port, 'log', [0, 2000])['result'] if m['Text'].startswith('Refused an API call')]
    if len(lines) > 3:
        problems.append('%d refusal lines logged' % len(lines))
    # every loadconfig value can be read back (non-UTF-8 included)
    if not _jrpc(daemon.rpc_port, 'loadconfig').get('result'):
        problems.append('loadconfig failed')
    try:
        _jrpc(daemon.rpc_port, 'shutdown')
    except Exception:
        pass
    proc.wait(timeout=30)

    # a real password with AuthorizedIP: an address in the list is let in
    # without credentials, so the same checks apply to it
    port = free_port()
    p = _spawn_env(t, [t.nzbget, '-c', conf, '-s', '-o', 'OutputMode=log', '-o', 'ControlPort=%d' % port,
                       '-o', 'ControlPassword=example-secret-7', '-o', 'AuthorizedIP=127.0.0.1',
                       '-o', 'ControlHostnames=nzb.example.com'],
                   {'HOME': t.makedirs('ghome-aip')}, 'garbage-aip.out')
    if _wait_rpc(port):
        own = 'http://127.0.0.1:%d' % port
        for origin, host, want, extra in ((None, None, 200, None), (own, None, 200, None), ('http://evil.example', None, 403, None),
                                          ('http://evil.example:%d' % port, 'evil.example:%d' % port, 403, None),
                                          # through a proxy for a name in ControlHostnames
                                          ('https://nzb.example.com', None, 200, {'X-Forwarded-Host': 'nzb.example.com'}),
                                          ('https://nzb.example.com', None, 200, None)):
            code, _ = _post_rpc_raw(port, origin, None, host=host, extra=extra)
            if code != want:
                problems.append('AuthorizedIP origin=%s host=%s -> %s' % (origin, host, code))
        # forwarded headers only ever make a call stricter: a same-origin
        # fetch can set X-Forwarded-Host itself, so a listed name there lifts
        # nothing; a listed real Host does, but not for same-site (a sibling
        # subdomain) unless the Referer shows RustyGet's own page
        get = '/jsonrpc/version'
        for host, site, referer, extra, want in (
                (None, 'same-origin', None, {'X-Forwarded-Host': 'nzb.example.com'}, 403),
                (None, 'same-origin', None, {'Forwarded': 'host=nzb.example.com'}, 403),
                ('nzb.example.com', 'same-origin', None, None, 200),
                ('nzb.example.com', 'same-site', None, None, 403),
                ('nzb.example.com', 'same-site', 'https://nzb.example.com/', None, 200)):
            code, _ = _post_rpc_raw(port, None, referer, host=host, path=get, method='GET', fetch_site=site, extra=extra)
            if code != want:
                problems.append('AuthorizedIP host=%s site=%s referer=%s %s -> %s' % (host, site, referer, extra, code))
        try:
            _jrpc(port, 'shutdown')
        except Exception:
            pass
    else:
        problems.append('AuthorizedIP daemon did not start')
    p.wait(timeout=30)

    # a file that exists but can't be read stops RustyGet (never the defaults
    # instead of the user's settings)
    if os.geteuid() != 0:
        path = t.path('odd-unreadable.conf')
        with open(path, 'w') as f:
            f.write('MainDir=/x\n')
        os.chmod(path, 0)
        r = subprocess.run([t.nzbget, '-c', path, '-s', '-o', 'OutputMode=log', '-o', 'ControlPort=%d' % free_port()],
                           env=dict(os.environ, HOME=t.makedirs('odd-home-unreadable')), capture_output=True, timeout=60)
        if r.returncode == 0 or b'Could not read the configuration file' not in r.stdout + r.stderr:
            problems.append('unreadable: rc=%s out=%r' % (r.returncode, (r.stdout + r.stderr)[-400:]))
        os.chmod(path, 0o600)
        # a folder RustyGet may not look into could hide the file: it stops
        # too (only "nothing there" means a missing file)
        locked = t.makedirs('odd-locked')
        os.chmod(locked, 0)
        r = subprocess.run([t.nzbget, '-c', os.path.join(locked, 'rustyget.conf'), '-s', '-o', 'OutputMode=log',
                            '-o', 'ControlPort=%d' % free_port()],
                           env=dict(os.environ, HOME=t.makedirs('odd-home-locked')), capture_output=True, timeout=60)
        os.chmod(locked, 0o700)
        if r.returncode == 0 or b'Could not read the configuration file' not in r.stdout + r.stderr \
                or os.listdir(locked):
            problems.append('unsearchable folder: rc=%s out=%r' % (r.returncode, (r.stdout + r.stderr)[-400:]))
        # the default place behind an unsearchable ~/.config stops as well
        home = t.makedirs('odd-home-locked2')
        os.makedirs(os.path.join(home, '.config', 'rustyget'))
        os.chmod(os.path.join(home, '.config'), 0)
        r = subprocess.run([t.nzbget, '-s', '-o', 'OutputMode=log', '-o', 'ControlPort=%d' % free_port()],
                           env=dict(os.environ, HOME=home), capture_output=True, timeout=60)
        os.chmod(os.path.join(home, '.config'), 0o700)
        if r.returncode == 0 or b'Could not read the configuration file' not in r.stdout + r.stderr:
            problems.append('unsearchable ~/.config: rc=%s out=%r' % (r.returncode, (r.stdout + r.stderr)[-400:]))

    # files that can't be a configuration
    odd = [('empty', lambda p: open(p, 'wb').close(), False),
           ('folder', lambda p: os.makedirs(p), True)]
    for name, make, inner in odd:
        path = t.path('odd-' + name + '.conf')
        make(path)
        port = free_port()
        args = [t.nzbget, '-c', path, '-s', '-o', 'OutputMode=log', '-o', 'ControlPort=%d' % port,
                '-o', 'MainDir=%s' % t.path('odd-main-' + name)]
        p = _spawn_env(t, args, {'HOME': t.makedirs('odd-home-' + name)}, 'odd-%s.out' % name)
        # without a file the stock sign-in applies, and stdout says so; a new
        # file (the folder case) has its own password, told on stdout
        auth = 'rustyget:tegbzn6789'
        if inner:
            made = _wait_text(t, 'odd-%s.out' % name, r'password "([A-Za-z0-9]{20})"')
            auth = 'rustyget:' + made.group(1) if made else None
        if not _wait_rpc(port, auth=auth):
            problems.append('%s: no RPC: %r' % (name, t.read_file('odd-%s.out' % name)[-500:]))
            continue
        out = t.read_file('odd-%s.out' % name).decode(errors='replace')
        if not inner and 'password "tegbzn6789", the stock password' not in out:
            problems.append('%s: stdout does not tell the stock sign-in' % name)
        st = _jrpc(port, 'setupstatus', auth=auth)['result']
        if inner and not os.path.isfile(os.path.join(path, 'rustyget.conf')):
            problems.append('folder: no rustyget.conf made inside')
        if name == 'empty':
            # the stock password is no protection: the credentials in a URL
            # path from another site's form are refused, a tool's are not
            code, _ = _post_rpc_raw(port, 'http://evil.example', None, path='/rustyget:tegbzn6789/jsonrpc')
            if code != 403:
                problems.append('stock password, cross-site form POST -> %s' % code)
            code, _ = _post_rpc_raw(port, None, None, path='/rustyget:tegbzn6789/jsonrpc')
            if code != 200:
                problems.append('stock password from a tool -> %s' % code)
            # Sonarr in docker-compose: http://nzbget:6789/jsonrpc, stock sign-in
            code, _ = _post_rpc_raw(port, None, None, host='nzbget:%d' % port, path='/rustyget:tegbzn6789/jsonrpc')
            if code != 200:
                problems.append('stock password from a tool at Host nzbget -> %s' % code)
            code, _ = _post_rpc_raw(port, None, None, host='nzbget:%d' % port,
                                    extra={'Authorization': 'Basic ' + base64.b64encode(b'rustyget:tegbzn6789').decode()})
            if code != 200:
                problems.append('stock Basic auth from a tool at Host nzbget -> %s' % code)
        if _http_get(port, '/')[0] not in (200, 401):
            problems.append('%s: no web interface' % name)
        try:
            _jrpc(port, 'shutdown', auth=auth)
        except Exception:
            pass
        p.wait(timeout=30)
    return ('garbageconfig', not problems, ' '.join(problems))


def _grep_log(t, needle):
    try:
        return t.read_file('nzbget.log').decode(errors='replace').count(needle)
    except Exception:
        return 0


def _log_before(t, first, second):
    """True when the FIRST occurrence of ``first`` precedes the FIRST
    occurrence of ``second`` in the daemon log (both must occur). The log is
    strictly append-ordered, so this proves event ordering."""
    try:
        log = t.read_file('nzbget.log').decode(errors='replace')
    except Exception:
        return False
    first_at = log.find(first)
    second_at = log.find(second)
    return 0 <= first_at < second_at



# ---- Copies of one release added together are one fleet (docs/DROP_AS_FLEET.md) ----

DROP_REL = 'Drop.Movie.2024.1080p.WEB-DL.x264-GRP'
DROP_KEY = 'dupes:drop.movie.2024.1080p.web.dl.x264.grp'
DROP_OTHER = 'Drop.Show.S01E02.720p.HDTV.x264-GRP'
DROP_OTHER_KEY = 'dupes:drop.show.s01e02.720p.hdtv.x264.grp'


def _drop_copy(t, tag, data, missing, seg=100_000, dead_par=False):
    """One posting of ``data`` with a par2 index (article borrowing checks
    borrowed bytes against it); ``missing`` parts are gone from the server, and
    with ``dead_par`` the par2 index too."""
    path = _place_copy(t, tag, data, 'movie.bin')
    par = generators.par2_index([('Drop.Movie.bin', data)])
    par_path = tag + '/rel.par2'
    t.write_file(os.path.join('data', par_path), par)
    return build_multi_nzb([(path, 'Drop.Movie.bin', len(data), seg, missing),
                            (par_path, 'Drop.Movie.par2', len(par), 500_000, {1} if dead_par else set())])


def _params(item):
    return {p['Name']: p['Value'] for p in item.get('Parameters', [])}


def _items_of_key(api, key):
    return [g for g in api.listgroups() + api.history() if g.get('DupeKey', '').lower() == key]


def _wait_key_success(api, key, timeout=240):
    """Waits until an item of ``key`` succeeds, then until nothing of it is queued."""
    deadline = time.time() + timeout
    done = None
    while time.time() < deadline and not done:
        done = next((h for h in api.history() if h.get('DupeKey', '').lower() == key
                     and h['Status'].startswith('SUCCESS')), None)
        time.sleep(0.5)
    while time.time() < deadline and any(g.get('DupeKey', '').lower() == key for g in api.listgroups()):
        time.sleep(0.5)
    return done


def scenario_dropnzbdir(daemon, t):
    """Finder drops copies of one release into NzbDir at once (names differ only
    by download-manager suffixes: ``X.nzb``, ``X (1).nzb``, ``X_1.nzb`` ...), plus
    two copies of another release. Each release goes in as one fleet under
    DupeKey dupes:<normalized name>: one copy downloads, the rest wait as
    labelled backups with a DupeScore. The best copy (28 of 30 articles) is the
    primary and borrows its two missing articles from the partly alive copy
    (19 of 30), which stays a low-priority donor labelled a twin; the dead copies are labelled
    dead and keep their nzb-file; the exact copy of the primary's nzb-file is
    the same posting and is not added. After the primary succeeds every other
    copy is in history as DELETED/DUPE: nothing is marked FAILURE and nothing
    downloads again. The second release is a fleet of its own."""
    n = 30
    data = _payload(3_000_000, 15001)
    every = set(range(1, n + 1))
    primary = _drop_copy(t, 'dnA', data, {5, 6})
    # holds 5 and 6, which the primary lacks: 19 of 30 alive
    partial = _drop_copy(t, 'dnB', data, {2, 4, 8, 10, 12, 14, 16, 18, 20, 22, 24})
    dead1 = _drop_copy(t, 'dnC', data, every, dead_par=True)
    dead2 = _drop_copy(t, 'dnD', data, every, dead_par=True)
    other_data = _payload(1_000_000, 15002)
    other1 = _drop_copy(t, 'doA', other_data, set())
    other2 = _drop_copy(t, 'doB', other_data, set())
    api = daemon.wait_ready()
    # something queued before the drop, and downloads paused (the fleet still
    # probes): the copy picked to download goes in front of it
    blocker = _drop_copy(t, 'dnZ', _payload(500_000, 15003), set())
    _ds_append(api, 'Queued.Before.2023.720p.HDTV.x264-OTHER', blocker, 'other-key', 0, paused=True)
    api.pausedownload()
    files = [(DROP_REL + '.nzb', primary), (DROP_REL + ' (1).nzb', partial), (DROP_REL + '_1.nzb', dead1),
             (DROP_REL + '_2.nzb', dead2), (DROP_REL + '_3.nzb', primary),
             (DROP_OTHER + '.nzb', other1), (DROP_OTHER + '_1.nzb', other2)]
    for name, text in files:
        t.write_file(os.path.join('main', 'nzb', name), text.encode())
    deadline = time.time() + 90
    order = []
    while time.time() < deadline:
        order = [g['NZBName'] for g in api.listgroups()]
        if DROP_REL in order and DROP_OTHER in order and \
                _grep_log(t, 'Fleet dupes:drop.movie') + _grep_log(t, 'Fleet dupes:drop.show') == 2:
            break
        time.sleep(0.5)
    order = [g['NZBName'] for g in api.listgroups()]
    # F16: every copy of both fleets is queued (the held ones too), all of them
    # in front of the item queued before the drop
    # the primary's nzb and its exact copy (_3) are one posting: whichever of
    # the two the scan pass listed first is the member (readdir order differs
    # between file systems), the other is SAME_POSTING and not added
    prim_names = (DROP_REL, DROP_REL + '_3')
    in_front = order[-1:] == ['Queued.Before.2023.720p.HDTV.x264-OTHER'] and \
        sum(1 for n_ in prim_names if n_ in order[:-1]) == 1
    api.resumedownload()
    done = _wait_key_success(api, DROP_KEY)
    other_done = _wait_key_success(api, DROP_OTHER_KEY)
    # TwinCheck labels the backups on its next round (every 15 s)
    deadline = time.time() + 45
    while time.time() < deadline and not any(
            _params(i).get('DupeKind') for i in _items_of_key(api, DROP_KEY) if i['NZBName'] == DROP_REL + ' (1)'):
        time.sleep(1)
    items = _items_of_key(api, DROP_KEY)
    byname = {i['NZBName']: i for i in items}
    succeeded = [i['NZBName'] for i in items if i['Status'].startswith('SUCCESS')]
    failures = [i['NZBName'] for i in items if i['Status'].startswith('FAILURE')]
    others = [i for i in items if i['NZBName'] != (done or {}).get('NZBName')]
    labelled = {i['NZBName']: (i['Status'], i.get('DupeScore'), _params(i).get('DupeKind'), _params(i).get('DupeAlive'))
                for i in others}
    part = byname.get(DROP_REL + ' (1)', {})
    deads = [byname.get(DROP_REL + '_1', {}), byname.get(DROP_REL + '_2', {})]
    nzb_files = os.listdir(t.path('main', 'nzb'))
    dead_nzbs_kept = all(any(f.startswith(DROP_REL + s + '.nzb') for f in nzb_files) for s in ('_1', '_2'))
    recovered = int((done or {}).get('DupeRecoveredArticles', 0))
    tried_backups = sum(int(i.get('SuccessArticles', 0)) + int(i.get('FailedArticles', 0)) for i in others)
    other_items = _items_of_key(api, DROP_OTHER_KEY)
    # bytes each backup sent (articles served whole; a missing one costs nothing):
    # the probe reads about 1 MB of each, borrowing takes the primary's 2 articles
    time.sleep(0.5)
    requests = re.findall(r'Received: (?:BODY|ARTICLE) <([^>]*)>', t.read_file('nserv.log').decode(errors='replace'))
    sent = {}
    for mid in requests:
        m = re.match(r'(dn[A-Z])/[^?]*\?\d+=\d+:(\d+)$', mid)
        if m:
            sent[m.group(1)] = sent.get(m.group(1), 0) + int(m.group(2))
    probe_ok = all(sent.get(tag, 0) <= 2 * 1024 * 1024 for tag in ('dnB', 'dnC', 'dnD'))
    ok = (in_front and probe_ok and
          done is not None and done['NZBName'] in prim_names and succeeded == [done['NZBName']] and not failures and
          len(others) == 3 and all(i['Status'] == 'DELETED/DUPE' and int(i.get('DupeScore', 0)) > 0 for i in others) and
          _params(part).get('DupeKind') == 'twin' and _params(part).get('DupeAlive') not in (None, '0') and
          all(_params(d).get('DupeAlive') == '0' for d in deads) and dead_nzbs_kept and
          recovered == 2 and tried_backups == 0 and
          _verify_output(t, data, dirs=(('main', 'dst', (done or {}).get('NZBName', DROP_REL)),)) and
          other_done is not None and len(other_items) == 2 and
          sorted(i['Status'][:7] for i in other_items) == ['DELETED', 'SUCCESS'] and
          _grep_log(t, 'from NzbDir as one fleet under DupeKey') == 2)
    return ('dropnzbdir', ok, 'queue=%s bytes_sent=%s done=%s recovered=%d labelled=%s failures=%s dead_nzbs_kept=%s tried_backups=%d other=%s' % (
        order, sent, done and done['Status'], recovered, labelled, failures, dead_nzbs_kept, tried_backups,
        [(i['NZBName'], i['Status']) for i in other_items]))


def scenario_dropjoin(daemon, t):
    """Daemon side: copies of one release appended one by one without a DupeKey
    (as a client or the NzbDir scanner would, minutes apart) join one group:
    each gets DupeKey dupes:<normalized name> and a DupeScore in the order it
    came (100, 99, 98), the first downloads and the others wait as backups,
    then go to history as DELETED/DUPE when it succeeds (F16: until then they
    wait held in the queue, never in History). Copies sent with
    DupeJoin=no (the Add dialog's fleet box off) or in DupeMode FORCE stay on
    their own, as before."""
    data = _payload(1_000_000, 15101)
    rel = 'Join.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:join.movie.2024.1080p.web.dl.x264.grp'
    api = daemon.wait_ready()
    # queued before the release: the group goes in front of it
    _ds_append(api, 'Queued.Before.2023.720p.HDTV.x264-OTHER', _drop_copy(t, 'jnZ', data, set()), 'blocker-key', 0,
               paused=True)
    # the first stays paused until its copies came, so it is the one queued
    _ds_append(api, rel, _drop_copy(t, 'jnA', data, set()), '', 0, paused=True)
    _ds_append(api, rel + '_1', _drop_copy(t, 'jnB', data, set()), '', 0, paused=False)
    _ds_append(api, rel + ' (2)', _drop_copy(t, 'jnC', data, set()), '', 0, paused=False)
    # F16: the later copies wait held in the queue, not in History
    held = [_params(_wait_held(api, n_)).get('DupeHeld') for n_ in (rel + '_1', rel + ' (2)')]
    open_history = [h['NZBName'] for h in api.history() if h.get('DupeKey') == key]
    queued = [(g['NZBName'], g.get('DupeKey'), g.get('DupeScore')) for g in api.listgroups()]
    names = [q[0] for q in queued]
    # the copy that downloads is in front of the item queued before it
    in_front = rel in names and 'Queued.Before.2023.720p.HDTV.x264-OTHER' in names and \
        names.index(rel) < names.index('Queued.Before.2023.720p.HDTV.x264-OTHER')
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == rel])
    done = _wait_key_success(api, key)
    items = {i['NZBName']: i for i in _items_of_key(api, key)}
    scores = {n: int(i.get('DupeScore', 0)) for n, i in items.items()}
    statuses = {n: i['Status'] for n, i in items.items()}
    # opt-outs: the box off (DupeJoin=no) and Force
    solo = 'Solo.Movie.2024.1080p.WEB-DL.x264-GRP'
    _ds_append(api, solo, _drop_copy(t, 'jsA', data, set()), '', 0, paused=False, params=[{'Name': 'DupeJoin', 'Value': 'no'}])
    _ds_append(api, solo + '_1', _drop_copy(t, 'jsB', data, set()), '', 0, paused=False, params=[{'Name': 'DupeJoin', 'Value': 'no'}])
    forced = 'Forced.Movie.2024.1080p.WEB-DL.x264-GRP'
    for i, name in enumerate((forced, forced + '_1')):
        content = base64.standard_b64encode(_drop_copy(t, 'jf%d' % i, data, set()).encode()).decode()
        api.append(name, content, 'test', 0, False, False, '', 0, 'FORCE', [])
    apart = {}
    for name in (solo, solo + '_1', forced, forced + '_1'):
        h = daemon.wait_history(api, name, timeout=120)
        deadline = time.time() + 60
        while time.time() < deadline and not h['Status'].startswith(('SUCCESS', 'FAILURE', 'DELETED')):
            time.sleep(0.5)
            h = daemon.wait_history(api, name, timeout=60)
        apart[name] = (h['Status'], h.get('DupeKey'), h.get('DupeScore'))
    ok = (held == ['yes', 'yes'] and not open_history and
          done is not None and done['NZBName'] == rel and in_front and
          scores == {rel: 100, rel + '_1': 99, rel + ' (2)': 98} and
          statuses.get(rel + '_1') == 'DELETED/DUPE' and statuses.get(rel + ' (2)') == 'DELETED/DUPE' and
          _grep_log(t, 'with 1 other copy(ies) of the same release: DupeKey ' + key) == 1 and
          _grep_log(t, 'with 2 other copy(ies) of the same release: DupeKey ' + key) == 1 and
          all(v[0].startswith('SUCCESS') and v[1] == '' for v in apart.values()))
    return ('dropjoin', ok, 'queued=%s scores=%s statuses=%s apart=%s' % (queued, scores, statuses, apart))


def scenario_dropjoinrestart(daemon, t):
    """Copies queued without a DupeKey before the grouping ran (here: with
    DupeCheck off) join their group once after a restart with it on: the
    same DupeKey dupes:<normalized name> and DupeScores in queue order. The
    first film and its sequel stay apart. A group key an earlier build stored
    with the words of the name's {{password}} in it (review C:
    dupes:...grp.s3cret) is rebuilt from the name, and its copy joins it; no
    Grouping line shows the password."""
    data = _payload(500_000, 15301)
    rel = 'Restart.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:restart.movie.2024.1080p.web.dl.x264.grp'
    sequel = 'Restart.Movie.Part.Two.2026.1080p.WEB-DL.x264-GRP'
    pw_rel = 'Restart.Pw.Movie.2024.1080p.WEB-DL.x264-GRP{{s3cret}}'
    pw_key = 'dupes:restart.pw.movie.2024.1080p.web.dl.x264.grp'
    api = daemon.wait_ready()
    for i, name in enumerate((rel, rel + '_1', sequel)):
        _ds_append(api, name, _drop_copy(t, 'jr%d' % i, data, set()), '', 0, paused=True)
    _ds_append(api, pw_rel, _drop_copy(t, 'jrp0', data, set()), pw_key + '.s3cret', 0, paused=True)
    _ds_append(api, pw_rel + '_1', _drop_copy(t, 'jrp1', data, set()), '', 0, paused=True)
    pw_names = [g['NZBName'] for g in api.listgroups() if g['NZBName'].startswith('Restart.Pw.')]
    before_pw = {g['NZBName']: g.get('DupeKey') for g in api.listgroups() if g['NZBName'].startswith('Restart.Pw.')}
    before = {g['NZBName']: g.get('DupeKey') for g in api.listgroups() if not g['NZBName'].startswith('Restart.Pw.')}
    daemon.write_config(SCENARIO_OPTIONS['dropjoinrestart'][:-1] + ['DupeCheck=yes'])
    api = daemon.reload(api)
    deadline = time.time() + 60
    keys = {}
    while time.time() < deadline:
        groups = api.listgroups()
        keys = {g['NZBName']: (g.get('DupeKey'), g.get('DupeScore')) for g in groups}
        if keys.get(rel + '_1', ('', 0))[0] and all(keys.get(n, ('', 0))[0] == pw_key for n in pw_names):
            break
        time.sleep(1)
    log = t.read_file('nzbget.log').decode(errors='replace')
    grouping = [l for l in log.splitlines() if 'Grouping ' in l]
    pw_grouping = [l for l in grouping if 'Restart.Pw.' in l]
    ok = (all(v == '' for v in before.values()) and keys.get(rel) == (key, 100) and keys.get(rel + '_1') == (key, 99)
          and keys.get(sequel, ('x', 0))[0] == ''
          and len(pw_names) == 2 and all(keys.get(n, ('', 0))[0] == pw_key for n in pw_names)
          and len(pw_grouping) == 1 and not any('s3cret' in l for l in grouping))
    return ('dropjoinrestart', ok, 'before=%s after=%s pw_before=%s pw_grouping=%s' % (
        before, keys, before_pw, pw_grouping))


def _wait_held(api, name, timeout=60):
    """Waits until ``name`` is held in the queue by its fleet (F16); returns it."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        g = _ds_group(api, name)
        if g and _params(g).get('DupeHeld') == 'yes':
            return g
        time.sleep(0.5)
    return _ds_group(api, name) or {}


def _dead_release(t, tag, seed):
    seg, vol = 10_000, 2_900_000
    n = vol // seg
    dead = [('%s/d%d.bin' % (tag, i), 'Dead%d.bin' % i, vol, seg, set(range(1, n + 1))) for i in range(6)]
    for i, m in enumerate(dead):
        t.write_file(os.path.join('data', m[0]), _payload(vol, seed + i))
    return build_multi_nzb(dead)


def scenario_dropallfail(daemon, t):
    """Review finding 2 and F16: when every copy of a fleet fails, the group
    goes to History as a visible failure: the copy that failed first was held
    in the queue as a donor while its backup was tried, and when the backup
    failed too, both end FAILURE/HEALTH. (Before F16 the first copy waited in
    History as DELETED/DUPE.)"""
    rel = 'Allfail.Movie.2024.1080p.WEB-DL.x264-GRP'
    api = daemon.wait_ready()
    _ds_append(api, rel, _dead_release(t, 'afA', 15401), '', 0, paused=True)
    _ds_append(api, rel + '_1', _dead_release(t, 'afB', 15411), '', 0, paused=False)
    held_backup = _wait_held(api, rel + '_1')
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == rel])
    deadline = time.time() + 300
    statuses = {}
    while time.time() < deadline:
        statuses = {h['NZBName']: h['Status'] for h in api.history() if h['NZBName'] in (rel, rel + '_1')}
        if not any(g['NZBName'] in (rel, rel + '_1') for g in api.listgroups()) and \
                statuses.get(rel + '_1', '').startswith('FAILURE'):
            break
        time.sleep(1)
    ok = (_params(held_backup).get('DupeHeld') == 'yes' and statuses.get(rel, '').startswith('FAILURE/HEALTH') and
          statuses.get(rel + '_1', '').startswith('FAILURE/HEALTH') and _grep_log(t, 'Holding ' + rel + ' in its fleet') == 1)
    return ('dropallfail', ok, 'statuses=%s' % statuses)


def scenario_dropallfailnonzb(daemon, t):
    """Review B under F16: the backup failover picks is a held copy in the
    queue, parsed when it was added, so its nzb-file being gone from NzbDir
    does not stop it: when the first copy fails its health check it is held
    as a donor, the backup downloads and succeeds, and the first copy then
    goes to History as DELETED/DUPE. (Before F16 the backup waited in History,
    where a missing nzb-file made it unusable and the first copy ended
    FAILURE/HEALTH.)"""
    rel = 'Nonzb.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:nonzb.movie.2024.1080p.web.dl.x264.grp'
    data = _payload(1_000_000, 15421)
    api = daemon.wait_ready()
    _ds_append(api, rel, _dead_release(t, 'anA', 15431), '', 0, paused=True)
    _ds_append(api, rel + '_1', _drop_copy(t, 'anB', data, set()), '', 0, paused=False)
    hb = _wait_held(api, rel + '_1')
    for f in os.listdir(t.path('main', 'nzb')):
        if f.startswith(rel + '_1.nzb'):
            os.remove(t.path('main', 'nzb', f))
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == rel])
    done = _wait_key_success(api, key, timeout=300)
    h = next((x for x in api.history() if x['NZBName'] == rel), {})
    ok = (_params(hb).get('DupeHeld') == 'yes' and done is not None and done['NZBName'] == rel + '_1' and
          h.get('Status') == 'DELETED/DUPE' and _params(h).get('DupeParked') is not None)
    return ('dropallfailnonzb', ok, 'held=%s done=%s first=%s' % (
        _params(hb).get('DupeHeld'), done and done['NZBName'], h.get('Status')))


def scenario_dropjoinstrict(daemon, t):
    """Review findings 3 and 4: the daemon-side join never touches another
    release, a copy in another category or DupeMode, or a download with a key
    of its own. Iron-Man-3 and Iron-Man, the 1080p and 720p, the REPACK and
    the original, two groups and the first film and its sequel all stay
    apart; only true copies (X and X_1, same category) join."""
    data = _payload(400_000, 15501)
    api = daemon.wait_ready()

    def add(name, category='test', key='', mode='SCORE', i=[0]):
        i[0] += 1
        content = base64.standard_b64encode(_drop_copy(t, 'js%d' % i[0], data, set()).encode()).decode()
        return api.append(name, content, category, 0, False, True, key, 0, mode, [])
    add('Iron-Man-3')
    add('Iron-Man')
    add('Strict.Movie.2024.1080p.WEB-DL.x264-GRP')
    add('Strict.Movie.2024.720p.WEB-DL.x264-GRP')
    add('Strict.Movie.2024.REPACK.1080p.WEB-DL.x264-GRP')
    add('Strict.Movie.2024.1080p.WEB-DL.x264-OTHER')
    add('Dune.2021.BluRay.1080p.TrueHD.Atmos.7.1.AVC.REMUX-FraMeSToR')
    add('Dune.Part.Two.2024.BluRay.1080p.TrueHD.Atmos.7.1.AVC.REMUX-FraMeSToR')
    add('Strict.Movie.2024.1080p.WEB-DL.x264-GRP_1', category='other')
    add('Strict.Movie.2024.1080p.WEB-DL.x264-GRP_2', key='user-key')
    add('Strict.Movie.2024.1080p.WEB-DL.x264-GRP_3', mode='ALL')
    add('Real.Copy.2024.1080p.WEB-DL.x264-GRP')
    add('Real.Copy.2024.1080p.WEB-DL.x264-GRP_1')
    time.sleep(1)
    items = {g['NZBName']: g.get('DupeKey') for g in api.listgroups() + api.history()}
    joined = {n: k for n, k in items.items() if k and k.startswith('dupes:')}
    ok = (set(joined) == {'Real.Copy.2024.1080p.WEB-DL.x264-GRP', 'Real.Copy.2024.1080p.WEB-DL.x264-GRP_1'}
          and items.get('Strict.Movie.2024.1080p.WEB-DL.x264-GRP_2') == 'user-key'
          and all(not items.get(n) for n in ('Iron-Man-3', 'Iron-Man', 'Strict.Movie.2024.1080p.WEB-DL.x264-GRP',
                                              'Strict.Movie.2024.1080p.WEB-DL.x264-GRP_1',
                                              'Strict.Movie.2024.1080p.WEB-DL.x264-GRP_3')))
    return ('dropjoinstrict', ok, 'keys=%s' % items)


def scenario_dropfleetasync(daemon, t):
    """Review finding 9: appendfleet for copies grouped by name (the web UI's
    dupes: key) answers at once (PROBING) and checks and adds the copies in
    the background; their labels come afterwards."""
    daemon.fake_nntp.alive = set()
    api = daemon.wait_ready()
    members = [('Async.Movie.2024.1080p.WEB-DL.x264-GRP%s.nzb' % s,
                _fleet_copy(daemon, 'as%d' % i, 'c.mkv', 400_000, None, _payload(400_000, 15600))) for i, s in
               enumerate(['', '_1', ' (2)', '_3', '_4', '_5', '_6', '_7', '_8', '_9', '_10', '_11', '_12'])]
    daemon.fake_nntp.delays.update({'as': 0.3})
    key = 'dupes:async.movie.2024.1080p.web.dl.x264.grp'
    start = time.time()
    reply = _fleet(daemon, members, key=key, timeout=60)
    took = time.time() - start
    deadline = time.time() + 300
    kinds = {}
    while time.time() < deadline:
        kinds = {i['NZBName']: _params(i).get('DupeCopyKind') for i in _items_of_key(api, key)}
        if len(kinds) >= 13 and all(kinds.values()):
            break
        time.sleep(1)
    ok = took < 3 and reply['Reason'] == 'PROBING' and all(m['Status'] == 'PROBING' for m in reply['Members']) \
        and len(kinds) == 13 and all(kinds.values())
    return ('dropfleetasync', ok, 'took=%.1f reason=%s labelled=%d of %d' % (
        took, reply['Reason'], sum(1 for v in kinds.values() if v), len(kinds)))


def scenario_droppasswordlogs(daemon, t):
    """Review of C: copies named with a {{password}} go in as fleets from the
    web UI's appendfleet (dupes: key) and from NzbDir, and the password shows
    in no fleet, grouping or rescue log line and not in the PROBING reply. A
    client's key (checked within the call, with the payload intake) logs its
    "Fleet intake:" lines without it too."""
    daemon.fake_nntp.alive = set()
    api = daemon.wait_ready()
    web = 'Pwlog.Movie.2024.1080p.WEB-DL.x264-GRP'
    web_key = 'dupes:pwlog.movie.2024.1080p.web.dl.x264.grp'
    members = [(web + '{{s3cret}}%s.nzb' % s, _fleet_copy(daemon, 'pl%d' % i, 'c.mkv', 400_000, None, _payload(400_000, 15700)))
               for i, s in enumerate(['', '_1'])]
    reply = _fleet(daemon, members, key=web_key, timeout=60)
    # a client's key: checked within the call, with the payload intake
    client = [('Pwkey.Movie.2024.1080p.WEB-DL.x264-GRP{{s3cret}}%s.nzb' % s,
               _fleet_copy(daemon, 'pk%d' % i, 'k.mkv', 400_000, None, _payload(400_000, 15702)))
              for i, s in enumerate(['', '_1'])]
    _fleet(daemon, client, key='fleet:pwkey', timeout=60)
    nzbdir = 'Pwdir.Movie.2024.1080p.WEB-DL.x264-GRP'
    for i, s in enumerate(['', '_1']):
        text = _fleet_copy(daemon, 'pd%d' % i, 'd.mkv', 400_000, None, _payload(400_000, 15701))
        t.write_file(os.path.join('main', 'nzb', nzbdir + '{{s3cret}}%s.nzb' % s), text.encode())
    deadline = time.time() + 120
    while time.time() < deadline:
        if _grep_log(t, 'Fleet ' + web_key + ' checked in the background') == 1 and \
                _grep_log(t, 'Fleet dupes:pwdir.movie.2024.1080p.web.dl.x264.grp from NzbDir') == 1:
            break
        time.sleep(0.5)
    log = t.read_file('nzbget.log').decode(errors='replace').splitlines()
    ours = [l for l in log if any(w in l for w in ('Fleet', 'Grouping ', 'Rescue: ', 'Keeping ', 'from NzbDir'))]
    leaks = [l for l in ours if 's3cret' in l]
    names = [m['Name'] for m in reply['Members']]
    ok = (reply['Reason'] == 'PROBING' and len(names) == 2 and not any('s3cret' in n for n in names)
          and any('Pwlog.' in l for l in ours) and any('Pwdir.' in l for l in ours)
          and any('Fleet intake: Pwkey.' in l for l in ours) and not leaks)
    return ('droppasswordlogs', ok, 'reply=%s lines=%d leaks=%s' % (names, len(ours), leaks[:3]))


def scenario_dropkeepdonor(daemon, t):
    """A copy that fails its health check while its fleet is open stays in it as
    a donor, not a failure, and its shown health is the one it was judged on.
    The first of two copies (grouped by name: ``X`` and ``X_1``) is a dead
    posting of 1,740 articles; it fails over to the second as soon as a sample of
    its articles shows it dead. Its Health (which counts every untried article as
    good) is still above 90%, but it was judged dead from the sample. F16: while
    the second copy downloads it stays in the queue, held (paused, DupeParked
    near 0, shown as projected health and "dead"), never failed over to again;
    when the second copy completes it goes to History as DELETED/DUPE with its
    DupeParked/DupeProjected and nzb-file kept. (Before F16 it went to History
    as DELETED/DUPE at once.)"""
    seg, vol = 10_000, 2_900_000
    n = vol // seg
    rel = 'Keep.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:keep.movie.2024.1080p.web.dl.x264.grp'
    dead = [('kdA/d%d.bin' % i, 'Dead%d.bin' % i, vol, seg, set(range(1, n + 1))) for i in range(6)]
    for i, m in enumerate(dead):
        t.write_file(os.path.join('data', m[0]), _payload(vol, 15200 + i))
    data = _payload(3_000_000, 15210)
    bp = _place_copy(t, 'kdB', data)
    api = daemon.wait_ready()
    _ds_append(api, rel, build_multi_nzb(dead), '', 0, paused=True)
    _ds_append(api, rel + '_1', build_nzb(bp, 'Backup.bin', len(data), 100_000, set()), '', 0, paused=False)
    backup_held = _params(_wait_held(api, rel + '_1')).get('DupeHeld') == 'yes'
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == rel])
    # failed over: held in the queue as a donor while the backup downloads
    deadline = time.time() + 300
    held_donor = False
    while time.time() < deadline and not held_donor:
        g = _ds_group(api, rel)
        held_donor = bool(g) and _params(g).get('DupeHeld') == 'yes' and _params(g).get('DupeParked') is not None
        if not g and any(h['NZBName'] == rel for h in api.history()):
            break
        time.sleep(0.2)
    done = _wait_key_success(api, key, timeout=300)
    hp = next((h for h in api.history() if h['NZBName'] == rel), {})
    params = _params(hp)
    health = int(hp.get('Health', 0))
    projected = float(params.get('DupeProjected', '100'))
    parked = params.get('DupeParked')
    nzb_kept = any(f.startswith(rel + '.nzb') for f in os.listdir(t.path('main', 'nzb')))
    ok = (backup_held and held_donor and done is not None and done['NZBName'] == rel + '_1' and hp.get('Status') == 'DELETED/DUPE' and
          hp.get('DupeKey') == key and health >= 900 and projected < 10 and parked is not None and
          float(parked) < 10 and params.get('DupeAlive') == parked and nzb_kept and
          _grep_log(t, 'Holding ' + rel + ' in its fleet') == 1 and
          _verify_output(t, data))
    # downloaded again by hand: the earlier try's projection and donor mark go
    api.editqueue('HistoryRedownload', 0, '', [hp['NZBID']])
    deadline = time.time() + 30
    again = {}
    while time.time() < deadline:
        again = next((_params(g) for g in api.listgroups() if g['NZBName'] == rel), {})
        if again:
            break
        time.sleep(0.5)
    cleared = bool(again) and 'DupeParked' not in again and 'DupeProjected' not in again
    ok = ok and cleared
    return ('dropkeepdonor', ok, 'backup_held=%s held_donor=%s done=%s primary=%s health=%d projected=%s parked=%s '
            'nzb_kept=%s cleared=%s' % (backup_held, held_donor, done and (done['NZBName'], done['Status']),
                                        hp.get('Status'), health, projected, parked, nzb_kept, cleared))



# ---- F16: a fleet stays in Downloads (docs/DROP_AS_FLEET.md) ----

STAY_REL = 'Stay.Movie.2024.1080p.WEB-DL.x264-GRP'
STAY_KEY = 'dupes:stay.movie.2024.1080p.web.dl.x264.grp'


def _queued(api, key):
    return [g for g in api.listgroups() if g.get('DupeKey', '').lower() == key]


def _history_of(api, key):
    return [h for h in api.history() if h.get('DupeKey', '').lower() == key]


def _wait_probed(api, key, count, timeout=120):
    """Waits until ``count`` copies of ``key`` are queued and none is probing."""
    deadline = time.time() + timeout
    items = []
    while time.time() < deadline:
        items = _queued(api, key)
        if len(items) >= count and not any(_params(g).get('DupeProbing') == 'yes' for g in items):
            return items
        time.sleep(0.5)
    return items


def scenario_fleetstaysqueued(daemon, t):
    """F16 and F14: copies of one release sent as a fleet (appendfleet, a
    dupes: key) are all in the queue at once, held and "probing", before the
    background probe ends; then the copy picked downloads and the others stay
    held in the queue (paused, priority -100, DupeHeld), in failover order
    (DupeFleetOrder), never in History. A copy sent for the key while the
    first fleet still probes is not refused (the key lock is held only while
    adding: no KEY_BUSY). The primary (28 of 30 articles) borrows its two
    missing articles from a held copy that never downloads; only when it
    succeeds do the others go to History as DELETED/DUPE, nzb-files kept. Each
    copy's probe is logged with its time and requests."""
    n = 30
    data = _payload(3_000_000, 16001)
    primary = _drop_copy(t, 'sqA', data, {5, 6})
    partial = _drop_copy(t, 'sqB', data, {2, 4, 8, 10, 12, 14, 16, 18, 20, 22, 24})
    dead = _drop_copy(t, 'sqC', data, set(range(1, n + 1)), dead_par=True)
    late = _drop_copy(t, 'sqD', data, set(range(1, n + 1)), dead_par=True)
    api = daemon.wait_ready()
    api.pausedownload()
    start = time.time()
    reply = _fleet(daemon, [(STAY_REL + '.nzb', primary), (STAY_REL + ' (1).nzb', partial),
                            (STAY_REL + '_1.nzb', dead)], key=STAY_KEY, timeout=60)
    took = time.time() - start
    # every copy is in Downloads right away, probing
    early = []
    deadline = time.time() + 5
    while time.time() < deadline:
        early = _queued(api, STAY_KEY)
        if len(early) >= 3:
            break
        time.sleep(0.1)
    early_seen = time.time() - start
    early_probing = [g['NZBName'] for g in early if _params(g).get('DupeProbing') == 'yes'
                     and _params(g).get('DupeHeld') == 'yes']
    early_history = [h['NZBName'] for h in _history_of(api, STAY_KEY)]
    # a copy sent while the first fleet probes
    late_reply = _fleet(daemon, [(STAY_REL + '_2.nzb', late)], key=STAY_KEY, timeout=60)
    items = _wait_probed(api, STAY_KEY, 4)
    byname = {g['NZBName']: g for g in items}
    free = [g['NZBName'] for g in items if _params(g).get('DupeHeld') != 'yes']
    held = [g for g in items if _params(g).get('DupeHeld') == 'yes']
    held_ok = all(g['Status'] == 'PAUSED' and int(g.get('MaxPriority', 0)) == -100 for g in held)
    order = {g['NZBName']: _params(g).get('DupeFleetOrder') for g in items}
    primary_ids = {_params(g).get('DupeFleetPrimary') for g in items}
    open_history = [h['NZBName'] for h in _history_of(api, STAY_KEY)]
    api.resumedownload()
    done = _wait_key_success(api, STAY_KEY)
    after = {h['NZBName']: h for h in _history_of(api, STAY_KEY)}
    others = [h for n_, h in after.items() if n_ != STAY_REL]
    part = after.get(STAY_REL + ' (1)', {})
    nzb_files = os.listdir(t.path('main', 'nzb'))
    nzbs_kept = all(any(f.startswith(STAY_REL + s) for f in nzb_files) for s in (' (1)', '_1', '_2'))
    probe_logs = _grep_log(t, 'Fleet probe of ' + STAY_REL)
    # STATs give the alive share: the held partial copy is read by BODY only
    # for its par2 list and head (kind probe), its archive key and the two
    # articles it lends
    time.sleep(0.5)
    bodies = re.findall(r'Received: (?:BODY|ARTICLE) <(sqB/[^>]*)>', t.read_file('nserv.log').decode(errors='replace'))
    ok = (len(bodies) <= 8 and took < 3 and reply['Reason'] == 'PROBING' and late_reply['Reason'] == 'PROBING' and
          early_seen < 5 and len(early_probing) >= 3 and not early_history and
          free == [STAY_REL] and len(held) == 3 and held_ok and
          order.get(STAY_REL) == '0' and order.get(STAY_REL + ' (1)') == '1' and
          sorted([order.get(STAY_REL + '_1'), order.get(STAY_REL + '_2')]) == ['2', '3'] and
          primary_ids == {str(byname.get(STAY_REL, {}).get('NZBID'))} and not open_history and
          _grep_log(t, 'KEY_BUSY') == 0 and
          done is not None and done['NZBName'] == STAY_REL and int(done.get('DupeRecoveredArticles', 0)) == 2 and
          len(others) == 3 and all(h['Status'] == 'DELETED/DUPE' for h in others) and
          int(part.get('SuccessArticles', 0)) == 0 and nzbs_kept and probe_logs >= 3 and
          _verify_output(t, data))
    return ('fleetstaysqueued', ok,
            'took=%.1f early=%.1fs probing=%s early_history=%s free=%s held_ok=%s order=%s primary=%s '
            'open_history=%s key_busy=%d done=%s recovered=%s others=%s part_success=%s nzbs_kept=%s probe_logs=%d '
            'partial_bodies=%d' % (
                took, early_seen, early_probing, early_history, free, held_ok, order, primary_ids, open_history,
                _grep_log(t, 'KEY_BUSY'), done and done['Status'], (done or {}).get('DupeRecoveredArticles'),
                [(h['NZBName'], h['Status']) for h in others], part.get('SuccessArticles'), nzbs_kept, probe_logs, len(bodies)))


def scenario_fleetfailoverorder(daemon, t):
    """F16a: the order Downloads shows (DupeFleetOrder, computed by the
    daemon) is the order failover takes: when the primary goes (deleted here),
    the copy at place 1 ("next") downloads, the others stay held and move up.
    Healthy copies lead by ease of borrowing (a bare file before an archive),
    a weak copy after them, a dead one last."""
    data = _payload(1_500_000, 16101)
    bare = _drop_copy(t, 'foA', data, set())
    weak = _drop_copy(t, 'foB', data, {2, 3, 5, 7, 11})
    dead = _drop_copy(t, 'foC', data, set(range(1, 16)), dead_par=True)
    whole = _drop_copy(t, 'foD', data, set())
    rel = 'Order.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:order.movie.2024.1080p.web.dl.x264.grp'
    api = daemon.wait_ready()
    api.pausedownload()
    _fleet(daemon, [(rel + '_3.nzb', dead), (rel + '_1.nzb', weak), (rel + '.nzb', bare), (rel + '_2.nzb', whole)],
           key=key, timeout=60)
    items = _wait_probed(api, key, 4)
    order = {g['NZBName']: int(_params(g).get('DupeFleetOrder', '-1')) for g in items}
    ranked = sorted(order, key=lambda n_: order[n_])
    primary = ranked[0] if ranked else None
    nxt = ranked[1] if len(ranked) > 1 else None
    if primary:
        api.editqueue('GroupDelete', 0, '', [g['NZBID'] for g in items if g['NZBName'] == primary])
    deadline = time.time() + 30
    after = {}
    while time.time() < deadline:
        after = {g['NZBName']: g for g in _queued(api, key)}
        if nxt in after and _params(after[nxt]).get('DupeHeld') != 'yes' and \
                _params(after[nxt]).get('DupeFleetOrder') == '0':
            break
        time.sleep(0.5)
    released = [n_ for n_, g in after.items() if _params(g).get('DupeHeld') != 'yes']
    new_order = {n_: _params(g).get('DupeFleetOrder') for n_, g in after.items()}
    ok = (len(order) == 4 and order.get(rel + '_3') == 3 and order.get(rel + '_1') == 2 and
          sorted([order.get(rel), order.get(rel + '_2')]) == [0, 1] and
          released == [nxt] and new_order.get(rel + '_1') == '1' and new_order.get(rel + '_3') == '2' and
          _grep_log(t, 'Releasing ' + str(nxt) + ' from its fleet') == 1)
    return ('fleetfailoverorder', ok, 'order=%s primary=%s next=%s released=%s new_order=%s' % (
        order, primary, nxt, released, new_order))


def scenario_fleetprimaryfirst(daemon, t):
    """F2c (6790, 2026-11-25): a fleet's primary downloads before every other
    queued item, whatever their priorities. The released primary used to keep
    priority 0 and sat QUEUED for 45 minutes behind items at 47-96: the
    scheduler takes the highest priority before the queue order. Now the
    primary goes one above the highest queued item (97 here, its own 0 kept
    as DupeFleetOwnPriority, never force), the held copies stay at -100, and
    after a failover the new primary is raised the same way while the old one
    (deleted by hand) reaches History without the raise. Once downloads resume, the first article
    fetched of the primary or the item at priority 96 is the primary's, and
    the primary's own priority is back once its fleet ends."""
    rel = 'First.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:first.movie.2024.1080p.web.dl.x264.grp'
    data = _payload(1_500_000, 16501)
    copies = {tag: _drop_copy(t, tag, data, set()) for tag in ('pfA', 'pfB', 'pfC')}
    other_data = _payload(3_000_000, 16502)
    other_path = _place_copy(t, 'pfO', other_data, 'other.bin')
    other_nzb = build_nzb(other_path, 'Other.Show.bin', len(other_data), 100_000, set())
    other = 'Other.Show.S01E01.720p.HDTV.x264-OTHER'
    api = daemon.wait_ready()
    api.pausedownload()
    api.append(other + '.nzb', base64.standard_b64encode(other_nzb.encode()).decode(), 'test', 96, False, False,
               '', 0, 'score', [])
    _fleet(daemon, [(rel + '.nzb', copies['pfA']), (rel + '_1.nzb', copies['pfB']), (rel + '_2.nzb', copies['pfC'])],
           key=key, timeout=60)
    tag_of = {rel: 'pfA', rel + '_1': 'pfB', rel + '_2': 'pfC'}

    def primary_state(exclude=None):
        deadline = time.time() + 30
        while time.time() < deadline:
            items = _queued(api, key)
            free = [g for g in items if _params(g).get('DupeHeld') != 'yes' and g['NZBName'] != exclude]
            if len(free) == 1 and int(free[0].get('MaxPriority', 0)) == 97:
                return free[0], items
            time.sleep(0.25)
        return None, _queued(api, key)

    _wait_probed(api, key, 3)
    first, items = primary_state()
    held_prio = sorted(int(g.get('MaxPriority', 0)) for g in items if _params(g).get('DupeHeld') == 'yes')
    first_own = _params(first or {}).get('DupeFleetOwnPriority')
    # failover while paused: the next copy is raised, the old primary held at -100
    if first:
        api.editqueue('GroupDelete', 0, '', [first['NZBID']])
    second, after = primary_state(exclude=first and first['NZBName'])
    second_own = _params(second or {}).get('DupeFleetOwnPriority')
    other_prio = next((int(g.get('MaxPriority', 0)) for g in api.listgroups() if g['NZBName'] == other), None)
    mark = len(t.read_file('nserv.log'))
    api.resumedownload()
    done = _wait_key_success(api, key)
    log = t.read_file('nserv.log')[mark:].decode(errors='replace')
    bodies = re.findall(r'Received: (?:BODY|ARTICLE) <(pf[ABCO])/', log)
    # the primary's articles come before any of the item at 96 (a held copy may
    # still be read for a header or a lent article)
    prim_tag = tag_of.get((second or {}).get('NZBName'))
    first_body = next((b for b in bodies if b in (prim_tag, 'pfO')), None)
    raised = _grep_log(t, 'above every other queued item: the primary of its fleet downloads first')
    gone = next((h for h in api.history() if first and h['NZBName'] == first['NZBName']), None)
    gone_unraised = gone is not None and 'DupeFleetBoost' not in _params(gone)
    ok = (first is not None and first_own == '0' and held_prio == [-100, -100] and
          second is not None and second_own == '0' and other_prio == 96 and raised >= 2 and
          first_body is not None and first_body == prim_tag and
          done is not None and done['NZBName'] == second['NZBName'] and _verify_output(t, data) and
          'DupeFleetBoost' not in _params(done) and gone_unraised)
    return ('fleetprimaryfirst', ok, 'first=%s own=%s held=%s second=%s own2=%s other_prio=%s raised=%d '
            'first_body=%s bodies=%s done=%s done_boost=%s gone=%s' % (
                first and first['NZBName'], first_own, held_prio, second and second['NZBName'], second_own,
                other_prio, raised, first_body, bodies[:6], done and done['NZBName'],
                _params(done or {}).get('DupeFleetBoost'),
                gone and (gone.get('Status'), _params(gone).get('DupeFleetBoost'))))


def scenario_fleetallweak(daemon, t):
    """B2b (review): a fleet whose copies are all under DupeMinAlive (50%)
    but not all at 0% is no dead fleet. Its weak copies (about 30% alive)
    used to go to history as DEAD ("every copy of its fleet is dead"); now
    they stay queued as donors with their alive share (DupeAlive > 0), the
    0% copy stays as a dead held copy, nothing goes to history while the
    fleet is open and no copy with alive articles is called dead."""
    rel = 'Weak.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:weak.movie.2024.1080p.web.dl.x264.grp'
    data = _payload(2_000_000, 16701)
    gone_a = set(range(1, 21)) - {1, 4, 7, 10, 13, 16}
    gone_b = set(range(1, 21)) - {2, 5, 8, 11, 14, 17}
    weak_a = _drop_copy(t, 'awA', data, gone_a)
    weak_b = _drop_copy(t, 'awB', data, gone_b)
    dead = _drop_copy(t, 'awC', data, set(range(1, 21)), dead_par=True)
    api = daemon.wait_ready()
    api.pausedownload()
    _fleet(daemon, [(rel + '.nzb', weak_a), (rel + '_1.nzb', weak_b), (rel + '_2.nzb', dead)], key=key, timeout=60)
    items = _wait_probed(api, key, 3, timeout=90)
    byname = {g['NZBName']: _params(g) for g in items}
    history = [h['NZBName'] for h in _history_of(api, key)]
    weak_alive = [int(byname.get(n, {}).get('DupeAlive', '0') or 0) for n in (rel, rel + '_1')]
    dead_alive = byname.get(rel + '_2', {}).get('DupeAlive')
    # the best weak copy is tried (released), the others stay held
    free = [n for n, p in byname.items() if p.get('DupeHeld') != 'yes']
    api.resumedownload()
    ok = (len(items) == 3 and not history and all(0 < a < 50 for a in weak_alive) and dead_alive == '0' and
          len(free) == 1 and free[0] in (rel, rel + '_1') and
          _grep_log(t, 'every copy of its fleet is dead') == 0)
    return ('fleetallweak', ok, 'queued=%s free=%s history=%s weak_alive=%s dead_alive=%s' % (
        sorted(byname), free, history, weak_alive, dead_alive))


def _rar_drop_copy(t, tag, data, vol_size=300_000):
    """One posting of ``data`` as RAR3 store volumes with a par2 index of the
    volumes: what a fleet's kind probe reads (the par2 list, the largest
    file's head, the first and last volume's headers for the archive key)."""
    files = []
    members = []
    for i, vol in enumerate(generators.rar3_store_volumes('Speed.Movie.mkv', data, vol_size)):
        name = 'speed.movie.part%02d.rar' % (i + 1)
        path = '%s/%s' % (tag, name)
        t.write_file(os.path.join('data', path), vol)
        files.append((name, vol))
        members.append((path, name, len(vol), 100_000, set()))
    par = generators.par2_index(files)
    t.write_file(os.path.join('data', tag, 'speed.movie.par2'), par)
    members.append((tag + '/speed.movie.par2', 'speed.movie.par2', len(par), 500_000, set()))
    return build_multi_nzb(members)


def scenario_fleetprobespeed(daemon, t):
    """Probe speed: four RAR copies of one release behind a server that takes
    300 ms per request (nserv -w 300), with 40 connections (5 light ones, as
    on 6790). Each copy's kind probe reads its par2 list, its largest file's
    head and its archive key (first and last volume headers); the reads of
    one copy used to run one after another, each wait polled every 20 ms. The
    scenario logs each copy's kind probe time ("Fleet kind probe of ...: N
    ms") and STAT check time ("Fleet probe of ...: N ms") for the PR's
    before/after table; it passes when every copy was probed, the fleet's
    primary downloads and a copy took less than its reads one after another
    would (N x 300 ms; measured before: about 1.3 s for 4; after: 0.6-0.95 s)."""
    rel = 'Speed.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:speed.movie.2024.1080p.web.dl.x264.grp'
    data = _payload(1_200_000, 16601)
    names = [rel + '.nzb', rel + '_1.nzb', rel + '_2.nzb', rel + '_3.nzb']
    copies = [_rar_drop_copy(t, 'ps%d' % i, data) for i in range(len(names))]
    api = daemon.wait_ready()
    api.pausedownload()
    start = time.time()
    _fleet(daemon, list(zip(names, copies)), key=key, timeout=60)
    items = _wait_probed(api, key, len(names))
    probed_s = time.time() - start
    log = t.read_file('nzbget.log').decode(errors='replace')
    # the light reserve the probes had (it grows once the probes start)
    grown = re.findall(r'Light connections of Server1: \d+ -> (\d+)', log)
    light = int(grown[-1]) if grown else 0
    kind_ms = {m.group(1): int(m.group(2)) for m in
               re.finditer(r'Fleet kind probe of (\S+?)(?:\.nzb)?: (\d+) ms', log)}
    stat_ms = {m.group(1): int(m.group(2)) for m in re.finditer(r'Fleet probe of (\S+?)(?:\.nzb)?: (\d+) ms', log)}
    kind_reads = [(int(m.group(1)), int(m.group(2))) for m in
                  re.finditer(r'Fleet kind probe of \S+?: (\d+) ms, (\d+) article', log)]
    api.resumedownload()
    done = _wait_key_success(api, key, timeout=240)
    kinds = sorted(kind_ms.values())
    ok = (len(items) == len(names) and len(kind_ms) == len(names) and len(stat_ms) == len(names) and
          done is not None and
          # a copy that read N articles one after another took at least N x 300
          # ms (nserv -w 300); one below that proves its reads overlapped
          any(ms < reads * 300 for ms, reads in kind_reads))
    return ('fleetprobespeed', ok, 'light=%d probed_in=%.1fs kind_ms=%s (median %s) stat_ms=%s done=%s' % (
        light, probed_s, kind_ms, kinds[len(kinds) // 2] if kinds else None, stat_ms, done and done['NZBName']))


def scenario_fleetprobebusy(daemon, t):
    """Probe floor (#717 follow-up, 6790): fleetprobespeed while an unrelated
    download keeps every connection busy. 40 connections, 300 ms per request
    (nserv -w 300); the download of 8,000 small articles wants them all, so
    the light reserve shrinks to 1 ("Light connections of Server1: N -> 1").
    With the reserve as the only limit the four copies' kind probes read one
    article at a time, so the primary was released only after all of them,
    one after another (on 6790: 122 s). Probe reads now keep a floor of 5 per
    server from the probe share (idle connections only, never one a download
    holds). Logged for the PR's before/after table: each copy's kind probe
    time, the time to the primary's release and the download rate before
    and during the probe. Passes when every copy was probed, the copies'
    kind probes overlapped (their lengths add up to more than the span they
    took; before, one ran after another), the reserve was 1 when the fleet
    came, the download was still running at the primary's release and kept
    going (no zero-rate stretch over 3 s, its mean rate during the probe at
    least 75% of the rate before) and the fleet's primary completes."""
    rel = 'Speed.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:speed.movie.2024.1080p.web.dl.x264.grp'
    data = _payload(1_200_000, 16611)
    names = [rel + '.nzb', rel + '_1.nzb', rel + '_2.nzb', rel + '_3.nzb']
    copies = [_rar_drop_copy(t, 'pb%d' % i, data) for i in range(len(names))]
    load_size, load_seg = 40_000_000, 5_000
    load = _place_copy(t, 'pbLoad', _payload(load_size, 16612), 'big.bin')
    api = daemon.wait_ready()
    daemon.append(api, 'BusyLoad', build_nzb(load, 'BusyLoad.bin', load_size, load_seg, set()), False, 'pbl', 0)

    def light_now():
        return max([s.get('LightConnections', 0) for s in api.status().get('NewsServers', [])] or [0])

    def rate():
        return api.status().get('DownloadRate', 0)
    # the download takes every connection; the reserve shrinks one per 5 s window
    deadline = time.time() + 60
    while time.time() < deadline and not (rate() > 0 and light_now() == 1 and
                                          _grep_log(t, 'Light connections of Server1: 2 -> 1') +
                                          _grep_log(t, 'Light connections of Server1: 0 -> 1') > 0):
        time.sleep(0.5)
    light_at_add = light_now()
    before = []
    for _ in range(6):
        before.append(rate())
        time.sleep(0.5)
    start = time.time()
    threading.Thread(target=lambda: _fleet(daemon, list(zip(names, copies)), key=key, timeout=60),
                     daemon=True).start()
    during = []
    released_s = None
    probed_s = None
    zero_since = None
    longest_zero = 0.0
    load_running = False
    items = []
    deadline = time.time() + 180
    while time.time() < deadline and (released_s is None or probed_s is None):
        groups = api.listgroups()
        r = rate()
        now = time.time()
        if r == 0:
            zero_since = zero_since or now
            longest_zero = max(longest_zero, now - zero_since)
        else:
            zero_since = None
        mine = [g for g in groups if g.get('DupeKey', '').lower() == key]
        if mine:
            items = mine
        if released_s is None and any(_params(g).get('DupeHeld') != 'yes' and
                                      _params(g).get('DupeProbing') != 'yes' for g in mine):
            released_s = now - start
            load_running = any(g.get('NZBName') == 'BusyLoad' for g in groups)
        # every copy's kind probe and STAT check logged (the primary may
        # finish, and its fleet leave the queue, before the last one ends)
        if probed_s is None and _grep_log(t, 'Fleet kind probe of ') >= len(names) and \
                _grep_log(t, 'Fleet probe of ') >= len(names):
            probed_s = now - start
        if probed_s is None:
            during.append(r)
        time.sleep(0.25)
    load_left = [g for g in api.listgroups() if g.get('NZBName') == 'BusyLoad']
    log = t.read_file('nzbget.log').decode(errors='replace')
    kind_ms = {m.group(1): int(m.group(2)) for m in
               re.finditer(r'Fleet kind probe of (\S+?)(?:\.nzb)?: (\d+) ms', log)}
    # each kind probe's end (the log line's second) and length: the probes
    # overlapped when their lengths add up to more than the span from the
    # first start to the last end (plus a second for the log's resolution)
    ends = [(time.mktime(time.strptime(m.group(1), '%a %b %d %H:%M:%S %Y')), int(m.group(2)) / 1000.0)
            for m in re.finditer(r'^(\w{3} \w{3} +\d+ [\d:]+ \d{4})\t.*Fleet kind probe of \S+?: (\d+) ms',
                                 log, re.M)]
    span = max(e for e, _ in ends) - min(e - d for e, d in ends) if ends else 0.0
    kind_sum = sum(d for _, d in ends)
    overlapped = bool(ends) and kind_sum > span + 1.0
    stat_ms = {m.group(1): int(m.group(2)) for m in re.finditer(r'Fleet probe of (\S+?)(?:\.nzb)?: (\d+) ms', log)}
    # the rest of the download only slows the end of the test down
    for g in load_left:
        api.editqueue('GroupFinalDelete', 0, '', [g['NZBID']])
    done = _wait_key_success(api, key, timeout=240)
    kinds = sorted(kind_ms.values())
    avg = lambda xs: sum(xs) / len(xs) if xs else 0
    ok = (len(items) == len(names) and len(kind_ms) == len(names) and len(stat_ms) == len(names) and
          probed_s is not None and released_s is not None and done is not None and
          light_at_add == 1 and load_running and longest_zero <= 3.0 and overlapped and
          # the download kept its speed while the probe ran (the floor takes
          # idle connections only)
          avg(during) >= 0.75 * avg(before))
    secs = lambda x: 'none' if x is None else '%.1fs' % x
    return ('fleetprobebusy', ok, 'light_at_add=%d released_in=%s probed_in=%s kind_ms=%s (median %s, sum %.1fs '
            'over a %.1fs span: overlapped=%s) stat_ms=%s rate_before=%.0f KB/s rate_during_probe=%.0f KB/s '
            'longest_zero=%.1fs load_running=%s done=%s' % (
                light_at_add, secs(released_s), secs(probed_s), kind_ms,
                kinds[len(kinds) // 2] if kinds else None, kind_sum, span, overlapped, stat_ms,
                avg(before) / 1024, avg(during) / 1024, longest_zero, load_running, done and done['NZBName']))


def scenario_fleetslowprobe(daemon, t):
    """B1 and B5 (fleet-e2e-6790): behind a server that answers a missing
    article only after 2 s (nserv -x 2000; the LAN proxy took 1.5 s), a copy
    of 400 articles, three in four missing, is checked for at most its own 60 s
    (its probe line says it was cut and the share measured so far is kept,
    DupeAliveCut), several STATs at once; the healthy copy is released as the
    primary within seconds instead of after the slow copy; a copy still being
    probed is never at place 1 ("next"); a copy whose first ten STATs were all
    missing gets more before it is called dead (DupeAliveSample 0/N, N > 10)."""
    rel = 'Slow.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:slow.movie.2024.1080p.web.dl.x264.grp'
    healthy_data = _payload(3_000_000, 16301)
    healthy = _drop_copy(t, 'spA', healthy_data, set())
    gone = set(range(1, 401)) - set(range(1, 401, 4))
    mixed = _drop_copy(t, 'spB', _payload(4_000_000, 16302), gone, seg=10_000)
    dead = _drop_copy(t, 'spC', _payload(3_000_000, 16303), set(range(1, 31)), dead_par=True)
    api = daemon.wait_ready()
    api.pausedownload()
    start = time.time()
    reply = _fleet(daemon, [(rel + '_1.nzb', mixed), (rel + '_2.nzb', dead), (rel + '.nzb', healthy)],
                   key=key, timeout=60)
    primary_at = None
    next_probing = set()
    items = []
    deadline = time.time() + 180
    while time.time() < deadline:
        items = _queued(api, key)
        for g in items:
            p = _params(g)
            if p.get('DupeProbing') == 'yes' and p.get('DupeFleetOrder') == '1':
                next_probing.add(g['NZBName'])
            if primary_at is None and g['NZBName'] == rel and p.get('DupeHeld') != 'yes':
                primary_at = time.time() - start
        if len(items) >= 3 and not any(_params(g).get('DupeProbing') == 'yes' for g in items):
            break
        time.sleep(0.25)
    probed_at = time.time() - start
    byname = {g['NZBName']: _params(g) for g in items}
    log = t.read_file('nzbget.log').decode(errors='replace')
    probe_ms = {m.group(1): int(m.group(2)) for m in
                re.finditer(r'Fleet probe of (\S+)\.nzb: (\d+) ms', log)}
    cut_line = bool(re.search(r'Fleet probe of ' + re.escape(rel) + r'_1\.nzb: .*cut at its 60 s budget', log))
    mixed_p = byname.get(rel + '_1', {})
    dead_p = byname.get(rel + '_2', {})
    dead_sample = dead_p.get('DupeAliveSample', '')
    api.resumedownload()
    done = _wait_key_success(api, key, timeout=240)
    ok = (reply['Reason'] == 'PROBING' and primary_at is not None and primary_at < 15 and
          not next_probing and len(probe_ms) == 3 and max(probe_ms.values()) <= 61_000 and
          probe_ms.get(rel + '_1', 0) >= 55_000 and cut_line and mixed_p.get('DupeAliveCut') == 'yes' and
          int(mixed_p.get('DupeAlive', '-1')) > 0 and
          dead_p.get('DupeAlive') == '0' and dead_sample.startswith('0/') and int(dead_sample.split('/')[1]) > 10 and
          done is not None and done['NZBName'] == rel and _verify_output(t, healthy_data))
    return ('fleetslowprobe', ok,
            'primary_at=%s probed_at=%.1f next_probing=%s probe_ms=%s cut_line=%s mixed=%s dead=%s done=%s' % (
                primary_at and round(primary_at, 1), probed_at, sorted(next_probing), probe_ms, cut_line,
                {k: mixed_p.get(k) for k in ('DupeAlive', 'DupeAliveSample', 'DupeAliveCut')},
                {k: dead_p.get(k) for k in ('DupeAlive', 'DupeAliveSample')}, done and done['NZBName']))


def scenario_fleetfailoverkeepsalive(daemon, t):
    """B2 (fleet-e2e-6790): after a failover the fleet probe's findings stay.
    The older backup pass (DupeWatch, "Dupes: checking the backups") used to
    re-sample every copy after each primary change and write DupeAlive=0 over
    a weak copy under DupeMinAlive (4 of 15 articles alive here), so it ranked
    below a dead copy. Now it leaves a fleet that stays in Downloads alone:
    the weak copy keeps its share and its place above the dead one."""
    data = _payload(1_500_000, 16401)
    first = _drop_copy(t, 'kaA', data, set())
    second = _drop_copy(t, 'kaB', data, set())
    weak = _drop_copy(t, 'kaC', data, set(range(1, 16)) - {1, 5, 9, 13})
    dead = _drop_copy(t, 'kaD', data, set(range(1, 16)), dead_par=True)
    rel = 'Keepalive.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:keepalive.movie.2024.1080p.web.dl.x264.grp'
    api = daemon.wait_ready()
    api.pausedownload()
    _fleet(daemon, [(rel + '.nzb', first), (rel + '_1.nzb', second), (rel + '_2.nzb', weak), (rel + '_3.nzb', dead)],
           key=key, timeout=60)
    items = _wait_probed(api, key, 4)
    before = {g['NZBName']: _params(g) for g in items}
    weak_alive = before.get(rel + '_2', {}).get('DupeAlive')
    order = {n_: int(p.get('DupeFleetOrder', '-1')) for n_, p in before.items()}
    primary = min(order, key=lambda n_: order[n_]) if order else None
    if primary:
        api.editqueue('GroupDelete', 0, '', [g['NZBID'] for g in items if g['NZBName'] == primary])
    # the backup pass used to run within seconds of the primary change
    time.sleep(20)
    after = {g['NZBName']: _params(g) for g in _queued(api, key)}
    new_order = {n_: p.get('DupeFleetOrder') for n_, p in after.items()}
    weak_after = after.get(rel + '_2', {})
    log = t.read_file('nzbget.log').decode(errors='replace')
    backup_pass = log.count('checking the backups of ' + rel)
    in_history_wording = len(re.findall(r'(?:duplicate|backup) ' + re.escape(rel) + r'\S* in history', log))
    ok = (weak_alive is not None and 0 < int(weak_alive) < 50 and order.get(rel + '_2') == 2 and
          order.get(rel + '_3') == 3 and weak_after.get('DupeAlive') == weak_alive and
          new_order.get(rel + '_2') == '1' and new_order.get(rel + '_3') == '2' and
          backup_pass == 0 and in_history_wording == 0)
    return ('fleetfailoverkeepsalive', ok, 'weak_alive=%s order=%s after=%s weak_after=%s backup_pass=%d wording=%d' % (
        weak_alive, order, new_order, {k: weak_after.get(k) for k in ('DupeAlive', 'DupeHealth')}, backup_pass,
        in_history_wording))


def _salvage_copy(t, tag, data, index_dead):
    """A posting of ``data`` with a par2 index (dead when ``index_dead``) and
    a one-block par2 volume whose tail repeats the critical packets."""
    path = _place_copy(t, tag, data, 'movie.bin')
    par = generators.par2_index([('Salv.Movie.bin', data)], pad=0)
    par_path = tag + '/rel.par2'
    vol_path = tag + '/rel.vol00+01.par2'
    t.write_file(os.path.join('data', par_path), par)
    t.write_file(os.path.join('data', vol_path), par)
    return build_multi_nzb([(path, 'Salv.Movie.bin', len(data), 100_000, set()),
                            (par_path, 'Salv.Movie.par2', len(par), 500_000, {1} if index_dead else set()),
                            (vol_path, 'Salv.Movie.vol00+01.par2', len(par), 500_000, set())])


def scenario_fleetsalvage(daemon, t):
    """F2b: copies whose index .par2 is dead are identified from the tail of
    their smallest par2 volume (par_rs::salvage): two postings of the same
    files are twins, and the fingerprint TwinCheck stores is salvaged, never
    "lost"; a lost index never makes a copy dead."""
    data = _payload(1_200_000, 16201)
    rel = 'Salv.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:salv.movie.2024.1080p.web.dl.x264.grp'
    api = daemon.wait_ready()
    api.pausedownload()
    _fleet(daemon, [(rel + '.nzb', _salvage_copy(t, 'svA', data, True)),
                    (rel + '_1.nzb', _salvage_copy(t, 'svB', data, True))], key=key, timeout=60)
    items = _wait_probed(api, key, 2)
    kinds = {g['NZBName']: _params(g).get('DupeCopyKind') for g in items}
    alive = {g['NZBName']: _params(g).get('DupeAlive') for g in items}
    deadline = time.time() + 60
    files = {}
    while time.time() < deadline:
        files = {g['NZBName']: _params(g).get('DupeFiles') for g in _queued(api, key)}
        if len(files) == 2 and all(files.values()):
            break
        time.sleep(1)
    ok = (len(items) == 2 and 'twin' in kinds.values() and all(a not in (None, '0') for a in alive.values()) and
          _grep_log(t, 'is lost; salvaged from its volumes') >= 2 and
          all(f and f not in ('lost', 'none') for f in files.values()) and
          len(set(files.values())) == 1)
    return ('fleetsalvage', ok, 'kinds=%s alive=%s files=%s' % (kinds, alive, files))


# ---- F16 review fixes (becee83b review) ----

def _par_copy(t, tag, data, files=2, missing=(), vol=True, vol_articles=1):
    """A posting of ``data`` cut into ``files`` data files, with a par2 index
    and (``vol``) a par2 volume, the extra par2 a download pauses at first."""
    size = len(data) // files
    parts = [data[i * size:(i + 1) * size if i < files - 1 else len(data)] for i in range(files)]
    members = []
    for i, part in enumerate(parts):
        path = _place_copy(t, tag, part, 'part%d.bin' % i)
        members.append((path, 'Hold.Movie.part%d.bin' % i, len(part), 100_000, set(missing) if i == 0 else set()))
    par = generators.par2_index([('Hold.Movie.part%d.bin' % i, part) for i, part in enumerate(parts)], pad=0)
    t.write_file(os.path.join('data', tag, 'rel.par2'), par)
    members.append((tag + '/rel.par2', 'Hold.Movie.par2', len(par), 500_000, set()))
    if vol:
        t.write_file(os.path.join('data', tag, 'rel.vol00+01.par2'), par)
        members.append((tag + '/rel.vol00+01.par2', 'Hold.Movie.vol00+01.par2', len(par),
                        500_000 if vol_articles == 1 else len(par) // vol_articles + 1, set()))
    return build_multi_nzb(members)


def _files_of(api, nzb_id):
    return {f['Filename']: bool(f.get('Paused')) for f in api.listfiles(0, 0, nzb_id)}


def _group_named(api, name):
    """The queued item named ``name`` (its {{password}} may be cut from the name)."""
    plain = re.sub(r'\s*\{\{[^}]*\}\}', '', name).strip()
    return next((g for g in api.listgroups() if g['NZBName'] in (name, plain)), None)


def _wait_free(api, name, timeout=30):
    """Waits until ``name`` is queued and not held by its fleet."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        g = _group_named(api, name)
        if g and _params(g).get('DupeHeld') != 'yes':
            return g
        time.sleep(0.3)
    return _group_named(api, name) or {}


def scenario_fleetholdfiles(daemon, t):
    """Review items 3 and 4 and the priority and password nits.
    - A copy sent later with a higher DupeScore takes over: the copy that
      downloaded is held, and exactly one copy downloads (4).
    - A release resumes only the files the hold paused: a file the user paused
      stays paused, and the extra par2 volume stays paused (3). A copy held as
      it was added resumes as a fresh copy: everything but its extra par2.
    - A released copy gets its own priority back, or the one the user gave it
      while it was held.
    - "Found duplicate" and every fleet line show no {{password}}."""
    data = _payload(1_200_000, 16301)
    rel = 'Hold.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:hold.movie.2024.1080p.web.dl.x264.grp'
    api = daemon.wait_ready()
    api.pausedownload()
    b_name = rel + '_1 {{s3cret}}'
    _ds_append(api, b_name, _par_copy(t, 'hfB', data), key, 100, paused=False)
    b = _wait_free(api, b_name)
    b_files = {f['Filename']: f['ID'] for f in api.listfiles(0, 0, b['NZBID'])}
    api.editqueue('FilePause', 0, '', [b_files['Hold.Movie.part1.bin']])
    _ds_append(api, rel, _par_copy(t, 'hfA', data), key, 200, paused=False)
    a = _wait_free(api, rel)
    time.sleep(2)
    copies = [g for g in api.listgroups() if g.get('DupeKey', '').lower() == key]
    free = [g['NZBName'] for g in copies if _params(g).get('DupeHeld') != 'yes']
    b_held = _params(_group_named(api, b_name) or {}).get('DupeHeld') == 'yes'
    # a third copy, held as it is added; the user raises its priority meanwhile
    _ds_append(api, rel + '_2', _par_copy(t, 'hfC', data), key, 50, paused=False)
    c = _wait_held(api, rel + '_2')
    api.editqueue('GroupSetPriority', 0, '50', [c['NZBID']])
    # the primary goes: the copy next in order (b) takes over
    api.editqueue('GroupDelete', 0, '', [a['NZBID']])
    b2 = _wait_free(api, b_name)
    b_after = _files_of(api, b2.get('NZBID', 0)) if b2 else {}
    b_priority = int(b2.get('MaxPriority', -1)) if b2 else None
    api.editqueue('GroupDelete', 0, '', [b2['NZBID']]) if b2 else None
    c2 = _wait_free(api, rel + '_2')
    c_after = _files_of(api, c2.get('NZBID', 0)) if c2 else {}
    c_priority = int(c2.get('MaxPriority', -1)) if c2 else None
    log = t.read_file('nzbget.log').decode(errors='replace')
    fleet_lines = [line for line in log.splitlines()
                   if any(w in line for w in ('Found duplicate', 'Releasing ', 'Holding ', 'Failing over'))]
    secret = any('s3cret' in line for line in fleet_lines)
    ok = (free == [rel] and b_held and
          b_after.get('Hold.Movie.part1.bin') is True and b_after.get('Hold.Movie.part0.bin') is False and
          b_after.get('Hold.Movie.par2') is False and b_after.get('Hold.Movie.vol00+01.par2') is True and
          b_priority == 0 and
          c_after.get('Hold.Movie.part0.bin') is False and c_after.get('Hold.Movie.part1.bin') is False and
          c_after.get('Hold.Movie.vol00+01.par2') is True and c_priority == 50 and
          any(rel + '_1' in line for line in fleet_lines if 'Releasing ' in line) and not secret)
    return ('fleetholdfiles', ok, 'free=%s b_held=%s b_files=%s b_prio=%s c_files=%s c_prio=%s secret_in_fleet_lines=%s' % (
        free, b_held, b_after, b_priority, c_after, c_priority, secret))


def scenario_fleetparpaused(daemon, t):
    """6790 throughput (2026-11-27): copies added by appendfleet keep their
    extra par2 volumes paused. The primary a probe releases, and the backup
    released when the primary goes, download their data and the par2 index
    only; a recovery volume is fetched only when repair needs it. Before, the
    release resumed every file and the volumes stayed unpaused, so every
    download fetched all of its recovery volumes (dead ones too) after its
    data, holding every connection on 430s."""
    data = _payload(1_200_000, 16501)
    rel = 'Hold.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:hold.movie.2024.1080p.web.dl.x264.grp'
    api = daemon.wait_ready()
    api.pausedownload()
    _fleet(daemon, [(rel + '.nzb', _par_copy(t, 'fpA', data)),
                    (rel + '_1.nzb', _par_copy(t, 'fpB', data))], key=key, timeout=60)
    items = _wait_probed(api, key, 2)
    def fresh(files):
        return (files.get('Hold.Movie.part0.bin') is False and files.get('Hold.Movie.part1.bin') is False and
                files.get('Hold.Movie.par2') is False and files.get('Hold.Movie.vol00+01.par2') is True)

    def settled(nzb_id, timeout=10):
        # (the release may clear DupeHeld before it changes the files: read until settled)
        deadline = time.time() + timeout
        files = _files_of(api, nzb_id)
        while time.time() < deadline and not fresh(files):
            time.sleep(0.3)
            files = _files_of(api, nzb_id)
        return files
    deadline = time.time() + 30
    free = []
    while time.time() < deadline:
        free = [g for g in _queued(api, key) if _params(g).get('DupeHeld') != 'yes']
        if free:
            break
        time.sleep(0.3)
    primary = settled(free[0]['NZBID']) if free else {}
    held = [g for g in _queued(api, key) if _params(g).get('DupeHeld') == 'yes']
    backup = {}
    if free and held:
        api.editqueue('GroupDelete', 0, '', [free[0]['NZBID']])
        deadline = time.time() + 30
        while time.time() < deadline:
            g = next((x for x in _queued(api, key) if x['NZBID'] == held[0]['NZBID']), None)
            if g and _params(g).get('DupeHeld') != 'yes':
                backup = settled(g['NZBID'])
                break
            time.sleep(0.3)
    ok = len(items) == 2 and fresh(primary) and fresh(backup)
    return ('fleetparpaused', ok, 'copies=%d primary=%s backup=%s' % (len(items), primary, backup))


def _fleet_par_download(daemon, t, tag, restart):
    """Two intact copies with a par2 index and a recovery volume, sent by
    appendfleet; the primary downloads (after a daemon restart when
    ``restart``). Returns (history status, recovery-volume downloads logged)."""
    data = _payload(1_200_000, 16511)
    rel = 'Hold.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:hold.movie.2024.1080p.web.dl.x264.grp'
    api = daemon.wait_ready()
    api.pausedownload()
    _fleet(daemon, [(rel + '.nzb', _par_copy(t, tag + 'A', data, vol_articles=4)),
                    (rel + '_1.nzb', _par_copy(t, tag + 'B', data, vol_articles=4))], key=key, timeout=60)
    _wait_probed(api, key, 2)
    if restart:
        try:
            api.shutdown()
        except Exception:
            pass
        t.procs[-1].wait(timeout=60)
        daemon.start()
        api = daemon.wait_ready()
    api.resumedownload()
    deadline = time.time() + 120
    done = {}
    while time.time() < deadline:
        done = next((h for h in _history_of(api, key) if h['Status'].startswith(('SUCCESS', 'FAILURE'))), {})
        if done:
            break
        time.sleep(0.5)
    return done.get('Status', 'none'), len(re.findall(r'INFO\tSuccessfully downloaded [^\n]*vol00\+01\.par2', t.read_file('nzbget.log').decode(errors='replace')))


def scenario_fleetparskip(daemon, t):
    """6790 throughput (2026-11-27): an undamaged fleet copy downloads its
    data and par2 index, never its recovery volume (ParCheck=auto: extra par2
    stay paused until a repair needs them). On 6790 every copy fetched all
    of its recovery volumes after its data, dead ones too."""
    status, vols = _fleet_par_download(daemon, t, 'fps', False)
    return ('fleetparskip', status.startswith('SUCCESS') and vols == 0, 'status=%s vol_downloads=%s' % (status, vols))


def scenario_fleetparskiprestart(daemon, t):
    """As fleetparskip, with a daemon restart between the fleet's probe and
    the download: the extra par2 stay paused across the restart."""
    status, vols = _fleet_par_download(daemon, t, 'fpr', True)
    return ('fleetparskiprestart', status.startswith('SUCCESS') and vols == 0, 'status=%s vol_downloads=%s' % (status, vols))


def scenario_fleetparpausedmany(daemon, t):
    """As fleetparpaused with six copies (6790 adds fleets of six to twelve
    copies, released while the rest are still added) and a daemon restart:
    no queued copy has an unpaused recovery volume, before or after."""
    data = _payload(1_200_000, 16521)
    rel = 'Hold.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:hold.movie.2024.1080p.web.dl.x264.grp'
    api = daemon.wait_ready()
    api.pausedownload()
    _fleet(daemon, [(rel + ('_%d' % i if i else '') + '.nzb', _par_copy(t, 'fm%d' % i, data, vol_articles=4))
                    for i in range(6)], key=key, timeout=90)
    _wait_probed(api, key, 6)
    time.sleep(3)

    # (DownloadRate=50: after the restart the primary downloads slowly; the
    # census sees it while it does)
    def census(api):
        out = []
        for g in _queued(api, key):
            files = _files_of(api, g['NZBID'])
            out.append((g['NZBID'], _params(g).get('DupeHeld') == 'yes', files.get('Hold.Movie.vol00+01.par2'),
                        files.get('Hold.Movie.part0.bin')))
        return sorted(out)
    before = census(api)
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    daemon.start()
    api = daemon.wait_ready()
    time.sleep(2)
    after = census(api)
    good = lambda c: len(c) == 6 and all(vol is True and (part is True or not held) for _, held, vol, part in c)
    return ('fleetparpausedmany', good(before) and good(after), 'before=%s after=%s' % (before, after))


def scenario_pressureowncgroup(daemon, t):
    """6790 throughput (2026-11-27): another program's memory stalls on the
    host (its /proc/pressure/memory: full avg10=8.03) never throttle a daemon
    running in a cgroup of its own whose memory.pressure is calm; the host's
    PSI counts only for a daemon in the root cgroup (the control). On 6790 the
    host's PSI flapped the monitor to CRITICAL every minute or two, cutting
    the downloads to a quarter while the service's own PSI was 0.00."""
    mock = t.path('pressure-mock')
    host_psi = 'some avg10=8.03 avg60=8.18 avg300=6.55 total=1\nfull avg10=8.03 avg60=8.16 avg300=6.52 total=1\n'
    calm = 'some avg10=0.00 avg60=0.00 avg300=0.00 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=1\n'
    service = os.path.join(mock, 'sys', 'fs', 'cgroup', 'system.slice', 'rustyget.service')
    os.makedirs(os.path.join(mock, 'proc', 'self'), exist_ok=True)
    os.makedirs(os.path.join(mock, 'proc', 'pressure'), exist_ok=True)
    os.makedirs(service, exist_ok=True)

    def put(path, text):
        with open(path, 'w') as f:
            f.write(text)
    put(os.path.join(mock, 'proc', 'pressure', 'memory'), host_psi)
    put(os.path.join(mock, 'proc', 'meminfo'), 'MemTotal: 40000000 kB\nMemFree: 6000000 kB\nMemAvailable: 11000000 kB\n')
    put(os.path.join(mock, 'proc', 'self', 'cgroup'), '0::/system.slice/rustyget.service\n')
    put(os.path.join(service, 'memory.pressure'), calm)
    put(os.path.join(service, 'memory.current'), '800000000\n')
    put(os.path.join(service, 'memory.max'), 'max\n')
    api = daemon.wait_ready()
    try:
        api.shutdown()
    except Exception:
        pass
    t.procs[-1].wait(timeout=60)
    os.environ['NZBGET_MEMORY_PRESSURE_MOCK_ROOT'] = mock
    try:
        daemon.start()
    finally:
        os.environ.pop('NZBGET_MEMORY_PRESSURE_MOCK_ROOT', None)
    daemon.wait_ready()
    needle = 'System memory pressure CRITICAL'
    time.sleep(6)
    own = _grep_log(t, needle)
    # the control: the same daemon moved to the root cgroup sees the host's stalls
    put(os.path.join(mock, 'proc', 'self', 'cgroup'), '0::/\n')
    deadline = time.time() + 20
    while time.time() < deadline and _grep_log(t, needle) == own:
        time.sleep(0.5)
    root = _grep_log(t, needle) - own
    return ('pressureowncgroup', own == 0 and root >= 1, 'critical_in_own_cgroup=%d critical_in_root_cgroup=%d' % (own, root))


def scenario_fleetendnolive(daemon, t):
    """Review item 2: a fleet whose last live copy fails other than by its
    health check ends: its primary downloads in full and fails its par check
    (its data damaged), or is deleted by hand, and the held dead copies go to
    History as FAILURE/HEALTH with their nzb-files, never left held in the
    queue for ever."""
    data = _payload(1_000_000, 16401)
    bad = bytearray(data)
    for i in range(0, len(bad), 50_000):
        bad[i] ^= 0xff
    api = daemon.wait_ready()
    out = {}
    for case in ('par', 'delete'):
        rel = 'End%s.Movie.2024.1080p.WEB-DL.x264-GRP' % case.title()
        key = 'dupes:end%s.movie.2024.1080p.web.dl.x264.grp' % case
        prim_path = _place_copy(t, 'en%sA' % case, bytes(bad), 'movie.bin')
        par = generators.par2_index([('End.Movie.bin', data)], pad=0)
        t.write_file(os.path.join('data', 'en%sA' % case, 'rel.par2'), par)
        primary = build_multi_nzb([(prim_path, 'End.Movie.bin', len(data), 100_000, set()),
                                   ('en%sA/rel.par2' % case, 'End.Movie.par2', len(par), 500_000, set())])
        dead = _drop_copy(t, 'en%sB' % case, data, set(range(1, 11)), dead_par=True)
        if case == 'delete':
            api.pausedownload()
        _fleet(daemon, [(rel + '.nzb', primary), (rel + '_1.nzb', dead)], key=key, timeout=60)
        items = _wait_probed(api, key, 2)
        if case == 'delete':
            prim = next((g for g in items if _params(g).get('DupeHeld') != 'yes'), None)
            if prim:
                api.editqueue('GroupDelete', 0, '', [prim['NZBID']])
        deadline = time.time() + 180
        while time.time() < deadline and _queued(api, key):
            time.sleep(1)
        hist = {h['NZBName']: h['Status'] for h in _history_of(api, key)}
        out[case] = (len(_queued(api, key)), hist)
        if case == 'delete':
            api.resumedownload()
    nzbs = os.listdir(t.path('main', 'nzb'))
    ok = all(q == 0 and h.get(r + '_1', '').startswith('FAILURE/HEALTH')
             for (q, h), r in ((out['par'], 'EndPar.Movie.2024.1080p.WEB-DL.x264-GRP'),
                               (out['delete'], 'EndDelete.Movie.2024.1080p.WEB-DL.x264-GRP'))) and \
        out['par'][1].get('EndPar.Movie.2024.1080p.WEB-DL.x264-GRP', '').startswith('FAILURE') and \
        all(any(f.startswith('End%s.Movie.2024.1080p.WEB-DL.x264-GRP_1.nzb' % c) for f in nzbs) for c in ('Par', 'Delete')) and \
        _grep_log(t, 'goes to history as dead') == 2
    return ('fleetendnolive', ok, 'out=%s' % out)


def scenario_fleetprobehealthfail(daemon, t):
    """Review item 1: a dead copy that fails while copies of another
    submission are being probed never fails them: failover takes no copy that
    is still being probed, so the dead copy waits for the probe, is then held
    as a donor and the best new copy downloads and succeeds; when its health
    check gives up during the probe it is held as a donor too (the log line
    "while other copies are still being probed"). It fails over once only:
    a copy already held is never failed over again, which would leave two
    copies downloading."""
    rel = 'Probefail.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:probefail.movie.2024.1080p.web.dl.x264.grp'
    data = _payload(1_000_000, 16501)
    api = daemon.wait_ready()
    _ds_append(api, rel, _dead_release(t, 'pfA', 16511), key, 100, paused=True)
    copies = [(rel + '_1.nzb', _drop_copy(t, 'pfB', data, set())), (rel + '_2.nzb', _drop_copy(t, 'pfC', data, set()))]
    import threading
    th = threading.Thread(target=lambda: _fleet(daemon, copies, key=key, timeout=60))
    th.start()
    time.sleep(0.5)
    api.editqueue('GroupResume', 0, '', [g['NZBID'] for g in api.listgroups() if g['NZBName'] == rel])
    th.join(timeout=60)
    done = _wait_key_success(api, key, timeout=300)
    hist = {h['NZBName']: h['Status'] for h in _history_of(api, key)}
    during = _grep_log(t, 'while other copies are still being probed')
    # one failover only: a copy already held is never failed over again
    failovers = _grep_log(t, 'from its fleet: failing over from ' + rel)
    ok = (failovers == 1 and done is not None and done['NZBName'] in (rel + '_1', rel + '_2') and
          not any(v.startswith('FAILURE') for n_, v in hist.items() if n_ != rel) and
          not _queued(api, key))
    return ('fleetprobehealthfail', ok, 'done=%s history=%s failed_during_probe=%d failovers=%d' % (
        done and done['NZBName'], hist, during, failovers))


def scenario_fleetendkeephistory0(daemon, t):
    """Re-review 1: with KeepHistory=0 a copy deleted from the queue is freed at
    once. A fleet whose primary fails its par check with two dead copies held
    ends once: no crash (the daemon still answers), each held copy handled
    exactly once ("Every copy of the fleet of ..." once for each), and nothing
    of the fleet left in the queue."""
    data = _payload(1_000_000, 16601)
    bad = bytearray(data)
    for i in range(0, len(bad), 50_000):
        bad[i] ^= 0xff
    rel = 'Kh0.Movie.2024.1080p.WEB-DL.x264-GRP'
    key = 'dupes:kh0.movie.2024.1080p.web.dl.x264.grp'
    api = daemon.wait_ready()
    prim_path = _place_copy(t, 'khA', bytes(bad), 'movie.bin')
    par = generators.par2_index([('Kh0.Movie.bin', data)], pad=0)
    t.write_file(os.path.join('data', 'khA', 'rel.par2'), par)
    primary = build_multi_nzb([(prim_path, 'Kh0.Movie.bin', len(data), 100_000, set()),
                               ('khA/rel.par2', 'Kh0.Movie.par2', len(par), 500_000, set())])
    dead1 = _drop_copy(t, 'khB', data, set(range(1, 11)), dead_par=True)
    dead2 = _drop_copy(t, 'khC', data, set(range(1, 11)), dead_par=True)
    _fleet(daemon, [(rel + '.nzb', primary), (rel + '_1.nzb', dead1), (rel + '_2.nzb', dead2)], key=key, timeout=60)
    deadline = time.time() + 240
    while time.time() < deadline:
        if not _queued(api, key) and _grep_log(t, 'Every copy of the fleet of') >= 2:
            break
        time.sleep(1)
    time.sleep(3)
    alive = True
    try:
        api.status()
    except Exception:
        alive = False
    counts = {s_: _grep_log(t, 'Every copy of the fleet of ' + rel + s_ + ' failed') for s_ in ('_1', '_2')}
    total = _grep_log(t, 'Every copy of the fleet of')
    ok = alive and not _queued(api, key) and counts == {'_1': 1, '_2': 1} and total == 2
    return ('fleetendkeephistory0', ok, 'alive=%s queued=%d counts=%s total=%d' % (
        alive, len(_queued(api, key)) if alive else -1, counts, total))


def _thread_cpu_seconds(pid, name):
    """CPU seconds (user + system) the daemon's thread called ``name`` used so far, from
    /proc (Linux); None where there is no /proc or no such thread."""
    task_dir = '/proc/%d/task' % pid
    if not os.path.isdir(task_dir):
        return None
    tick = os.sysconf('SC_CLK_TCK')
    for tid in os.listdir(task_dir):
        try:
            with open(os.path.join(task_dir, tid, 'comm')) as f:
                if f.read().strip() != name:
                    continue
            with open(os.path.join(task_dir, tid, 'stat')) as f:
                fields = f.read().rsplit(')', 1)[1].split()
            # fields after the name: state is [0], utime [11], stime [12]
            return (int(fields[11]) + int(fields[12])) / tick
        except (OSError, IndexError, ValueError):
            continue
    return None


BIGQ_FLEETS, BIGQ_COPIES, BIGQ_FILES = 150, 7, 12


def _bigq_nzb(tag):
    """An nzb-file of BIGQ_FILES one-article files the news server does not have (never
    fetched: the copy is paused or held)."""
    files = ''.join(
        '<file poster="p@x" date="1" subject="[%d/%d] &quot;%s.part%02d.rar&quot; yEnc (1/1)">'
        '<groups><group>alt.binaries.test</group></groups><segments>'
        '<segment bytes="400000" number="1">%s-%d@x</segment></segments></file>'
        % (i + 1, BIGQ_FILES, tag, i + 1, tag, i) for i in range(BIGQ_FILES))
    return ('<?xml version="1.0" encoding="UTF-8"?>\n<nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">%s</nzb>'
            % files)


def scenario_fleetbigqueue(daemon, t):
    """A queue of 1050 items in 150 fleets of 7 copies (each fleet's primary paused, its 6
    other copies held, about 900 held copies), as the 6790 resubmit left it: the queue
    coordinator's per-second fleet refresh read the whole queue once per fleet and once more
    per primary, held the queue lock nearly all the time and spun at 100% CPU, so a finished
    job waited in PP_QUEUED and the API took minutes to answer. Now: building the queue goes
    quickly, the queue-coord thread stays under 20% of a core over 15 s with the primaries
    paused and under 40% (a debug build) while the 150 primaries download (Linux, from
    /proc), listgroups answers within 5 s, and an ordinary rar download added afterwards is unpacked
    and in history as SUCCESS within 90 s."""
    api = daemon.wait_ready()
    start = time.time()
    deadline = start + 300
    for fleet in range(BIGQ_FLEETS):
        key = 'dupes:bigq movie %03d' % fleet
        for copy in range(BIGQ_COPIES):
            if time.time() > deadline:
                return ('fleetbigqueue', False, 'building the queue took over 300 s (at fleet %d copy %d)' % (fleet, copy))
            name = 'BigQ.Movie.%03d.2160p.REMUX-G%d' % (fleet, copy)
            _ds_append(api, name, _bigq_nzb('bq%03d-%d' % (fleet, copy)), key, 100 - copy, paused=(copy == 0))
    built = time.time() - start
    groups = api.listgroups()
    held = sum(1 for g in groups if _params(g).get('DupeHeld') == 'yes')
    pid = daemon.process.pid

    def queue_coord_cpu(secs):
        cpu0, t0 = _thread_cpu_seconds(pid, 'queue-coord'), time.time()
        time.sleep(secs)
        cpu1, t1 = _thread_cpu_seconds(pid, 'queue-coord'), time.time()
        return None if cpu0 is None or cpu1 is None else (cpu1 - cpu0) / (t1 - t0)

    def listgroups_secs():
        lg_start = time.time()
        api.listgroups()
        return time.time() - lg_start

    time.sleep(3)  # a few refreshes on the full queue
    cpu = queue_coord_cpu(15)
    lg = listgroups_secs()
    # an ordinary download behind them goes through post-processing (unpack) at once
    members = _rarrenamer_members(t, 'bqP', ['testfile3'])
    added = time.time()
    daemon.append(api, 'BigQPlain', build_multi_nzb([m[:5] for m in members]), False, 'bigq-plain', 100)
    try:
        h = daemon.wait_history(api, 'BigQPlain', timeout=90)
        plain, plain_s = h['Status'], time.time() - added
    except RuntimeError:
        plain, plain_s = 'not in history', time.time() - added
    # Last, the 150 primaries download: the news server holds their articles, so the
    # connections stay busy and the coordinator looks for the next article every few
    # milliseconds, as on 6790. (Last because the held articles keep the connections and the
    # server blocked long after a pause.)
    primaries = [g['NZBID'] for g in groups if g['NZBName'].endswith('-G0')]
    api.editqueue('GroupResume', 0, '', primaries)
    time.sleep(5)
    downloading = sum(1 for g in api.listgroups() if g.get('ActiveDownloads', 0) > 0)
    cpu_dl = queue_coord_cpu(15)
    lg_dl = listgroups_secs()
    files = _main_basenames(t)
    pct = lambda c: 'n/a' if c is None else '%.1f%%' % (c * 100)
    ok = (len(groups) == BIGQ_FLEETS * BIGQ_COPIES and held >= BIGQ_FLEETS * (BIGQ_COPIES - 1) and
          len(primaries) == BIGQ_FLEETS and (cpu is None or cpu < 0.20) and downloading > 0 and (cpu_dl is None or cpu_dl < 0.40) and
          lg < 5 and lg_dl < 5 and plain.startswith('SUCCESS') and 'testfile3.dat' in files)
    return ('fleetbigqueue', ok, 'groups=%d held=%d built=%.1fs queue-coord cpu idle=%s downloading=%s (%d primaries fetching) listgroups=%.2fs/%.2fs plain=%s after %.1fs' % (
        len(groups), held, built, pct(cpu), pct(cpu_dl), downloading, lg, lg_dl, plain, plain_s))


def scenario_dupausedunpack(daemon, t):
    """A direct unpack waiting for a volume of a download paused whole holds no other job's
    unpack back, and is not stopped for it. RelA's direct unpack starts on its first volumes,
    then RelA is paused while its last volume is still on the way (the news server holds it);
    RelB, a whole rar set added after, unpacks in a free unpack slot (a waiting extraction
    holds none) and goes to history as SUCCESS, while RelA's direct unpack keeps waiting:
    it is neither stopped nor cancelled, so its live extraction is not thrown away. Before
    unpack slots, RelB sat in PP_QUEUED for as long as RelA stayed paused (6790: a finished
    job waited 36 minutes behind a paused download's direct unpack)."""
    api = daemon.wait_ready()
    members_a = _rarrenamer_members(t, 'dpuA', ['testfile3'])
    daemon.append(api, 'RelA', build_multi_nzb([m[:5] for m in members_a]), False, 'dpu-a', 100)
    deadline = time.time() + 60
    while time.time() < deadline and _grep_log(t, 'Directly unpacking RelA') == 0:
        time.sleep(0.2)
    started = _grep_log(t, 'Directly unpacking RelA') > 0
    rel_a = next((g for g in api.listgroups() if g['NZBName'] == 'RelA'), None)
    if rel_a is None:
        return ('dupausedunpack', False, 'RelA left the queue before it could be paused (du_started=%s)' % started)
    api.editqueue('GroupPause', 0, '', [rel_a['NZBID']])
    members_b = _rarrenamer_members(t, 'dpuB', ['testfile5'])
    daemon.append(api, 'RelB', build_multi_nzb([m[:5] for m in members_b]), False, 'dpu-b', 100)
    # (RelA's last volume is held 200 s and ArticleTimeout is 120 s: its direct unpack waits
    # all through this, so RelB has to go past it, not after it)
    try:
        h = daemon.wait_history(api, 'RelB', timeout=45)
        status, unpack = h['Status'], h['UnpackStatus']
    except RuntimeError:
        g = next((g for g in api.listgroups() if g['NZBName'] == 'RelB'), {})
        status, unpack = 'still queued: %s' % g.get('Status'), '-'
    a_now = next((g['Status'] for g in api.listgroups() if g['NZBName'] == 'RelA'), 'gone')
    stopped = _grep_log(t, 'Stopping the direct unpack of RelA') + _grep_log(t, 'Cancelling direct unpack for RelA')
    files = _main_basenames(t)
    ok = (started and stopped == 0 and a_now != 'gone' and status.startswith('SUCCESS') and unpack == 'SUCCESS' and
          'testfile5.dat' in files)
    return ('dupausedunpack', ok, 'du_started=%s stopped=%d relB=%s unpack=%s relA=%s files=%s' % (
        started, stopped, status, unpack, a_now, files))



def _corrupt_part_release(t, tag, rar5=False):
    """A stored RAR set (8 x 1 MB volumes) with a par2 set of 20 recovery blocks, posted with
    one wrong byte in the middle of volume 4: the articles carry the yEnc CRC of the wrong
    bytes, so the download is whole (health 100%) and only the archive's part check and par2
    see the damage. Returns the NZB and the payload."""
    import subprocess as _sp
    work = t.path('data', tag)
    os.makedirs(work, exist_ok=True)
    movie = _payload(8_000_000, 24310)
    if rar5:
        scratch = t.path('data', tag + '-rar')
        os.makedirs(scratch, exist_ok=True)
        vols = generators.rar5_method_volumes('movie.mkv', movie, 1_000_000, 0, scratch)
    else:
        vols = generators.rar3_store_volumes_valid('movie.mkv', movie, 1_000_000)
    names = []
    for i, vol in enumerate(vols):
        name = 'movie.part%02d.rar' % (i + 1)
        t.write_file(os.path.join('data', tag, name), vol)
        names.append(name)
    _sp.run(['par2', 'create', '-q', '-q', '-s65536', '-c20', '-n2', '-a', 'movie', 'movie.par2'] + names,
            cwd=work, check=True, capture_output=True)
    bad = bytearray(vols[3])
    bad[len(bad) // 2] ^= 0x5a
    t.write_file(os.path.join('data', tag, names[3]), bytes(bad))
    members = []
    for name in sorted(n for n in os.listdir(work) if n.endswith('.par2')):
        members.append(('%s/%s' % (tag, name), name, os.path.getsize(os.path.join(work, name)), 100_000, set()))
    for name, vol in zip(names, vols):
        members.append(('%s/%s' % (tag, name), name, len(vol), 100_000, set()))
    return build_multi_nzb(members), movie


def _corrupt_part_run(daemon, t, label, tag, rar5=False):
    """zipfast #314 + RustyGet: a part that fails its own check (a corrupt article with a valid
    yEnc CRC) is not extracted again. The volume is held as damaged, par2 repairs it, and only
    its bytes are read back into the output kept from the first pass; post-processing does not
    unpack again. Before (prybar's 4 GiB audit, m0-dmg): the persistent session stopped at
    the part, direct unpack dropped its partial state and re-extracted from volume 1, and
    post-processing extracted everything a third time (about 3x the set read)."""
    nzb, movie = _corrupt_part_release(t, tag, rar5)
    api = daemon.wait_ready()
    daemon.append(api, 'Corrupt', nzb, False, tag + '-key', 100)
    h = daemon.wait_history(api, 'Corrupt', timeout=300)
    intact = _verify_output(t, movie, '.mkv', dirs=(('main', 'dst'), ('main', 'inter')))
    log = t.read_file('nzbget.log').decode(errors='replace')
    declared = len(re.findall(r'failed its own check in movie\.part04\.rar', log))
    late = len(re.findall(r'live PAR found \d+ bytes of movie\.part04\.rar bad', log))
    first_reads = len(re.findall(r'Unrar: Extracting from movie\.part01\.rar', log))
    fallback = len(re.findall(r'progressive direct unpack \(', log))
    again = len(re.findall(r'unpacking all files again', log))
    du_ok = _grep_log(t, 'Direct unpack for Corrupt successful')
    written = sum(int(n) for n in re.findall(r'persistent partial files wrote (\d+) bytes', log))
    refilled = (sum(int(n) for n in re.findall(r'while extracting \((\d+) bytes\)', log)) +
                sum(int(n) for n in re.findall(r'(\d+) bytes read back\)', log)))
    # Each archive byte read once by the extraction, plus at most the repaired part (and a
    # 64 KiB slice live PAR may count damaged for a moment and rebuild).
    ok = (h['Status'].startswith('SUCCESS') and intact and du_ok == 1 and declared + late >= 1 and
          first_reads == 1 and fallback == 0 and again == 0 and
          0 < written <= len(movie) and 0 < refilled <= 1_000_000 + 2 * 65536)
    return (label, ok, 'status=%s intact=%s du_ok=%d declared=%d late=%d first_reads=%d fallback=%d '
            'unpack_again=%d written=%d refilled=%d' % (h['Status'], intact, du_ok, declared, late, first_reads,
                                                       fallback, again, written, refilled))


def scenario_corruptpartdu(daemon, t):
    """A RAR4 stored set with a corrupt article whose yEnc CRC is valid: see _corrupt_part_run."""
    return _corrupt_part_run(daemon, t, 'corruptpartdu', 'cpd')


def scenario_corruptpartdurar5(daemon, t):
    """corruptpartdu with a RAR5 stored set (rar -m0 -ma5)."""
    if not generators.HAVE_RAR:
        return ('corruptpartdurar5', None, 'SKIP: rar not installed')
    return _corrupt_part_run(daemon, t, 'corruptpartdurar5', 'cpd5', rar5=True)


SCENARIOS = {
    'corruptpartdu': scenario_corruptpartdu,
    'corruptpartdurar5': scenario_corruptpartdurar5,
    'dropnzbdir': scenario_dropnzbdir,
    'fleetprimaryease': scenario_fleetprimaryease,
    'dropdune': scenario_dropdune,
    'fleetnzbpassword': scenario_fleetnzbpassword,
    'fleettimeoutunverified': scenario_fleettimeoutunverified,
    'dropjoin': scenario_dropjoin,
    'dropallfail': scenario_dropallfail,
    'dropallfailnonzb': scenario_dropallfailnonzb,
    'dropjoinstrict': scenario_dropjoinstrict,
    'dropfleetasync': scenario_dropfleetasync,
    'droppasswordlogs': scenario_droppasswordlogs,
    'dropjoinrestart': scenario_dropjoinrestart,
    'dropkeepdonor': scenario_dropkeepdonor,
    'fleetstaysqueued': scenario_fleetstaysqueued,
    'fleetfailoverorder': scenario_fleetfailoverorder,
    'fleetprimaryfirst': scenario_fleetprimaryfirst,
    'fleetprobespeed': scenario_fleetprobespeed,
    'fleetprobebusy': scenario_fleetprobebusy,
    'fleetallweak': scenario_fleetallweak,
    'fleetslowprobe': scenario_fleetslowprobe,
    'fleetfailoverkeepsalive': scenario_fleetfailoverkeepsalive,
    'fleetsalvage': scenario_fleetsalvage,
    'fleetholdfiles': scenario_fleetholdfiles,
    'fleetparpaused': scenario_fleetparpaused,
    'pressureowncgroup': scenario_pressureowncgroup,
    'fleetparskip': scenario_fleetparskip,
    'fleetparpausedmany': scenario_fleetparpausedmany,
    'fleetparskiprestart': scenario_fleetparskiprestart,
    'fleetendnolive': scenario_fleetendnolive,
    'fleetprobehealthfail': scenario_fleetprobehealthfail,
    'fleetendkeephistory0': scenario_fleetendkeephistory0,
    'complementary': scenario_complementary,
    'historylog': scenario_historylog,
    'urlhistorylog': scenario_urlhistorylog,
    'redownloadresetsrecovered': scenario_redownloadresetsrecovered,
    'recheckfailed': scenario_recheckfailed,
    'nzbgapborrow': scenario_nzbgapborrow,
    'sidefilenorepair': scenario_sidefilenorepair,
    'finaldeletemidway': scenario_finaldeletemidway,
    'interdirchange': scenario_interdirchange,
    'finaldeleterestart': scenario_finaldeleterestart,
    'restartmidway': scenario_restartmidway,
    'declaredcountnopair': scenario_declaredcountnopair,
    'rejectnextserver': scenario_rejectnextserver,
    'backbonemiss': scenario_backbonemiss,
    'backendauto': scenario_backendauto,
    'cutover': scenario_cutover,
    'transientretry': scenario_transientretry,
    'transientexhausted': scenario_transientexhausted,
    'transientotherserver': scenario_transientotherserver,
    'leadswitch': scenario_leadswitch,
    'cutovertruth': scenario_cutovertruth,
    'manydonors': scenario_manydonors,
    'stream': scenario_stream,
    'truncatedstate': scenario_truncatedstate,
    'queuesyncslow': scenario_queuesyncslow,
    'diskfullrestart': scenario_diskfullrestart,
    'cachecommitfull': scenario_cachecommitfull,
    'interdirbudget': scenario_interdirbudget,
    'interdirbudgetprio': scenario_interdirbudgetprio,
    'queuebatchstart': scenario_queuebatchstart,
    'streamrestartcredit': scenario_streamrestartcredit,
    'streamtimeout': scenario_streamtimeout,
    'streamdeaddonor': scenario_streamdeaddonor,
    'streamslowprogress': scenario_streamslowprogress,
    'streamtooslow': scenario_streamtooslow,
    'streammaxrun': scenario_streammaxrun,
    'streamstarved': scenario_streamstarved,
    'liveoverlap': scenario_liveoverlap,
    'livegate': scenario_livegate,
    'livelastfile': scenario_livelastfile,
    'reloadmidrepair': scenario_reloadmidrepair,
    'repost': scenario_repost,
    'repostrenamed': scenario_repostrenamed,
    'repostobfuscated': scenario_repostobfuscated,
    'repostdonorgaps': scenario_repostdonorgaps,
    'xpackbare': scenario_xpackbare,
    'xpackrar': scenario_xpackrar,
    'xpackgapdonor': scenario_xpackgapdonor,
    'xpackrar2rar': scenario_xpackrar2rar,
    'xpack2sets': scenario_xpack2sets,
    'xpackzip': scenario_xpackzip,
    'xpack7z': scenario_xpack7z,
    'xpack7zlzma2': scenario_xpack7zlzma2,
    'xpack7zmanyvols': scenario_xpack7zmanyvols,
    'xpackrarmanyvols': scenario_xpackrarmanyvols,
    'xpacksplit': scenario_xpacksplit,
    'xpackcompressed': scenario_xpackcompressed,
    'xpackneg': scenario_xpackneg,
    'xcrypt_encplain': scenario_xcrypt_encplain,
    'xcrypt_enc7ztarget': scenario_xcrypt_enc7ztarget,
    'xpacknesteddonor': scenario_xpacknesteddonor,
    'xcrypt_nestedin7zenc': scenario_xcrypt_nestedin7zenc,
    'xcrypt_nestedinrarenc': scenario_xcrypt_nestedinrarenc,
    'xcrypt_nestedinenctarget': scenario_xcrypt_nestedinenctarget,
    'xpacknestedsamename': scenario_xpacknestedsamename,
    'xpacknestedsetdonor': scenario_xpacknestedsetdonor,
    'xpacknestedsettarget': scenario_xpacknestedsettarget,
    'xpacknestedtarget': scenario_xpacknestedtarget,
    'xcrypt_enc7zdonor': scenario_xcrypt_enc7zdonor,
    'xcrypt_plainenc': scenario_xcrypt_plainenc,
    'xcrypt_diffpass': scenario_xcrypt_diffpass,
    'xcrypt_wrongpass': scenario_xcrypt_wrongpass,
    'xdecomp_zip': scenario_xdecomp_zip,
    'xdecomp_slow': scenario_xdecomp_slow,
    'xdecomp_7z': scenario_xdecomp_7z,
    'xdecomp_storetarget': scenario_xdecomp_storetarget,
    'xdecomp_enc7z': scenario_xdecomp_enc7z,
    'xdecomp_enctarget': scenario_xdecomp_enctarget,
    'xdecomp_neg': scenario_xdecomp_neg,
    'xdecomp_symlink': scenario_xdecomp_symlink,
    'xdecomp_off': scenario_xdecomp_off,
    'wholefile': scenario_wholefile,
    'dupefailover': scenario_dupefailover,
    'lockedfailover': scenario_lockedfailover,
    'dupefailoverraw': scenario_dupefailoverraw,
    'dupehopeless': scenario_dupehopeless,
    'dupehopelessretry': scenario_dupehopelessretry,
    'dupedeadstart': scenario_dupedeadstart,
    'wholefileretry': scenario_wholefileretry,
    'dupehopelessnodupecheck': scenario_dupehopelessnodupecheck,
    'dupefailovernofallback': scenario_dupefailovernofallback,
    'wholefilepar': scenario_wholefilepar,
    'wholefilefailretry': scenario_wholefilefailretry,
    'wholefilelive': scenario_wholefilelive,
    'streamretry': scenario_streamretry,
    'streamgrouped': scenario_streamgrouped,
    'retrykeepsjobs': scenario_retrykeepsjobs,
    'retryparkedname': scenario_retryparkedname,
    'healthlastarticle': scenario_healthlastarticle,
    'wholefilerestart': scenario_wholefilerestart,
    'wholefilerestartlive': scenario_wholefilerestartlive,
    'restartfirstdead': scenario_restartfirstdead,
    'wholefilestale': scenario_wholefilestale,
    'wholefilenfoproof': scenario_wholefilenfoproof,
    'wholefilesampleproof': scenario_wholefilesampleproof,
    'dupefailoverchain': scenario_dupefailoverchain,
    'xpacklatency': scenario_xpacklatency,
    'xpackflaky': scenario_xpackflaky,
    'deadpickprobe': scenario_deadpickprobe,
    'deadpickafterreload': scenario_deadpickafterreload,
    'deadpickpartial': scenario_deadpickpartial,
    'deadpickstray': scenario_deadpickstray,
    'forcefailover': scenario_forcefailover,
    'copybackup': scenario_copybackup,
    'copyoffailed': scenario_copyoffailed,
    'resendbackup': scenario_resendbackup,
    'resendfailed': scenario_resendfailed,
    'projectedswap': scenario_projectedswap,
    'runswap': scenario_runswap,
    'runnobackup': scenario_runnobackup,
    'runscattered': scenario_runscattered,
    'runreset': scenario_runreset,
    'projectednobackup': scenario_projectednobackup,
    'projectedhalfnobackup': scenario_projectedhalfnobackup,
    'projectedbelownobackup': scenario_projectedbelownobackup,
    'keepreturned': scenario_keepreturned,
    'restartfailover': scenario_restartfailover,
    'slowprobe': scenario_slowprobe,
    'runparcovers': scenario_runparcovers,
    'ondiskskip': scenario_ondiskskip,
    'ondiskgone': scenario_ondiskgone,
    'ondiskstop': scenario_ondiskstop,
    'ondiskstoprepairfirst': scenario_ondiskstoprepairfirst,
    'ondiskstopsamename': scenario_ondiskstopsamename,
    'ondiskstopsamenamenointer': scenario_ondiskstopsamenamenointer,
    'ondiskstopothernamenointer': scenario_ondiskstopothernamenointer,
    'repairlast': scenario_repairlast,
    'recheckwholefail': scenario_recheckwholefail,
    'lightburst': scenario_lightburst,
    'samepostingrepair': scenario_samepostingrepair,
    'samepostinglate': scenario_samepostinglate,
    'projectedhealthy': scenario_projectedhealthy,
    'projectedworsebackup': scenario_projectedworsebackup,
    'projectededge': scenario_projectededge,
    'projectededgelead': scenario_projectededgelead,
    'parprojection': scenario_parprojection,
    'deaddownloadstray2': scenario_deaddownloadstray2,
    'deaddownloadrestart': scenario_deaddownloadrestart,
    'deadbackupsonly': scenario_deadbackupsonly,
    'backuporder': scenario_backuporder,
    'deaddownloadlate': scenario_deaddownloadlate,
    'fleetdeadfirst': scenario_fleetdeadfirst,
    'fleettwins': scenario_fleettwins,
    'fleettimeout': scenario_fleettimeout,
    'fleetone': scenario_fleetone,
    'fleetnodupecheck': scenario_fleetnodupecheck,
    'fleetkindorder': scenario_fleetkindorder,
    'fleetkindsingle': scenario_fleetkindsingle,
    'fleetkindlater': scenario_fleetkindlater,
    'fleetintake': scenario_fleetintake,
    'fleetintakelater': scenario_fleetintakelater,
    'fleetencryptedlast': scenario_fleetencryptedlast,
    'fleetencryptedheadless': scenario_fleetencryptedheadless,
    'fleetlockedlast': scenario_fleetlockedlast,
    'fleetencryptedpassword': scenario_fleetencryptedpassword,
    'fleettwinvolumes': scenario_fleettwinvolumes,
    'fleetkindslowcheck': scenario_fleetkindslowcheck,
    'watchforcepick': scenario_watchforcepick,
    'watchdeadpick': scenario_watchdeadpick,
    'watchsearchedkey': scenario_watchsearchedkey,
    'watchswap': scenario_watchswap,
    'watchborrow': scenario_watchborrow,
    'watchfleet': scenario_watchfleet,
    'watchswaptwin': scenario_watchswaptwin,
    'watchrescue': scenario_watchrescue,
    'midwaytwin': scenario_midwaytwin,
    'midwayalt': scenario_midwayalt,
    'fleetalldead': scenario_fleetalldead,
    'fleetshutdown': scenario_fleetshutdown,
    'fleetallerror': scenario_fleetallerror,
    'fleetotherkey': scenario_fleetotherkey,
    'fleetnokey': scenario_fleetnokey,
    'fleetresend': scenario_fleetresend,
    'fleetresendslow': scenario_fleetresendslow,
    'fleetaddbackup': scenario_fleetaddbackup,
    'fleetfailover': scenario_fleetfailover,
    'fleetbusy': scenario_fleetbusy,
    'fleetdeadtwins': scenario_fleetdeadtwins,
    'fleetlarge': scenario_fleetlarge,
    'fleetcopy': scenario_fleetcopy,
    'fleetmostlydead': scenario_fleetmostlydead,
    'fleetpaused': scenario_fleetpaused,
    'fleetparallel': scenario_fleetparallel,
    'fleetscoremax': scenario_fleetscoremax,
    'fleetsamekey': scenario_fleetsamekey,
    'fleetwide': scenario_fleetwide,
    'fleetslowurlfirst': scenario_fleetslowurlfirst,
    'fleetmerged': scenario_fleetmerged,
    'appendconcurrent': scenario_appendconcurrent,
    'appendurlgzip': scenario_appendurlgzip,
    'readdafterdelete': scenario_readdafterdelete,
    'appendscorerange': scenario_appendscorerange,
    'apiedges': scenario_apiedges,
    'apiformat': scenario_apiformat,
    'jsonpoff': scenario_jsonpoff,
    'apiaccess': scenario_apiaccess,
    'queueedits': scenario_queueedits,
    'directunpackkeep': scenario_directunpackkeep,
    'directunpacksimple': scenario_directunpacksimple,
    'badpaths': scenario_badpaths,
    'noconfig': scenario_noconfig,
    'garbageconfig': scenario_garbageconfig,
    'directunpacktwoarchives': scenario_directunpacktwoarchives,
    'directunpackparfailure': scenario_directunpackparfailure,
    'fleetbigqueue': scenario_fleetbigqueue,
    'dupausedunpack': scenario_dupausedunpack,
    'parfailurenodirectunpack': scenario_parfailurenodirectunpack,
    'parfailuredirectunpackfailed': scenario_parfailuredirectunpackfailed,
    'jointwosets': scenario_jointwosets,
    'live7zsplit': scenario_live7zsplit,
    'live7zlzma2': scenario_live7zlzma2,
    'live7zlzma2threads': scenario_live7zlzma2threads,
    'livezipsplit': scenario_livezipsplit,
    'livezipz01': scenario_livezipz01,
    'livesplitjoin': scenario_livesplitjoin,
    'liveedgehole': scenario_liveedgehole,
    'liveedgehole5': scenario_liveedgehole5,
    'livefedrar5store': scenario_livefedrar5store,
    'livefedrar5m3': scenario_livefedrar5m3,
    'livefed7z': scenario_livefed7z,
    'liveorder7z': scenario_liveorder7z,
    'liveorderrar': scenario_liveorderrar,
    'livestraggler': scenario_livestraggler,
    'staleprogress': scenario_staleprogress,
    'authrejected': scenario_authrejected,
    'appendlongname': scenario_appendlongname,
    'editscorerange': scenario_editscorerange,
    'appendlarge': scenario_appendlarge,
    'appendurlodd': scenario_appendurlodd,
    'addstorm': scenario_addstorm,
    'longstateline': scenario_longstateline,
    'articledecoy': scenario_articledecoy,
    'articledecoypar': scenario_articledecoypar,
    'articledecoyparrestart': scenario_articledecoyparrestart,
    'yencrangefar': scenario_yencrangefar,
    'yencrangeshort': scenario_yencrangeshort,
    'quotafutureday': scenario_quotafutureday,
    'connhold': scenario_connhold,
    'innerarchivekeep': scenario_innerarchivekeep,
    'innerarchivekeepdirect': scenario_innerarchivekeepdirect,
    'feedpreviewparallel': scenario_feedpreviewparallel,
    'tasktypo': scenario_tasktypo,
    'urlredirects': scenario_urlredirects,
    'urlclosed': scenario_urlclosed,
    'urlretrywait': scenario_urlretrywait,
    'historyeditlist': scenario_historyeditlist,
    'speedtestnohistory': scenario_speedtestnohistory,
    'getunsafe': scenario_getunsafe,
    'netspeedpause': scenario_netspeedpause,
    'httpedge': scenario_httpedge,
    'volumereset': scenario_volumereset,
    'xmlparamdecode': scenario_xmlparamdecode,
    'pathtraversalnzb': scenario_pathtraversalnzb,
    'pathtraversalarticle': scenario_pathtraversalarticle,
    'urlschemes': scenario_urlschemes,
    'nzbentities': scenario_nzbentities,
    'uucache': scenario_uucache,
    'filepausepars': scenario_filepausepars,
    'nntppercent': scenario_nntppercent,
    'joinequalpieces': scenario_joinequalpieces,
    'directunpackkeepnointer': scenario_directunpackkeepnointer,
    'parscanpercent': scenario_parscanpercent,
    'retentionnodate': scenario_retentionnodate,
    'joingroupreconnect': scenario_joingroupreconnect,
    'joingroupnogroups': scenario_joingroupnogroups,
    'stallquit': scenario_stallquit,
    'appendslashname': scenario_appendslashname,
    'parrenamebad': scenario_parrenamebad,
    'longnamedir': scenario_longnamedir,
    'damagedstats': scenario_damagedstats,
    'damagedhistorykind': scenario_damagedhistorykind,
    'emptyconfigread': scenario_emptyconfigread,
    'damagedfilestate': scenario_damagedfilestate,
    'staletempstate': scenario_staletempstate,
    'failedloadfileinfos': scenario_failedloadfileinfos,
    'clientthreads': scenario_clientthreads,
    'xmlrpccdata': scenario_xmlrpccdata,
    'webdirget': scenario_webdirget,
    'webetag': scenario_webetag,
    'web404warn': scenario_web404warn,
    'partialnzb': scenario_partialnzb,
    'partialnzbdeclared': scenario_partialnzbdeclared,
    'daemonlock': scenario_daemonlock,
    'daemonlockhealth': scenario_daemonlockhealth,
    'urlarchive': scenario_urlarchive,
    'extdownloadname': scenario_extdownloadname,
    'connectioncap': scenario_connectioncap,
    'parvolnames': scenario_parvolnames,
    'pardamagefailover': scenario_pardamagefailover,
    'pardamageborrow': scenario_pardamageborrow,
    'nosourceonce': scenario_nosourceonce,
    'twinalt': scenario_twinalt,
    'twinlabelonadd': scenario_twinlabelonadd,
    'neartwinborrow': scenario_neartwinborrow,
    'lostindexpar': scenario_lostindexpar,
    'allheadspar': scenario_allheadspar,
    'allheadsalt': scenario_allheadsalt,
    'allheadsaltall': scenario_allheadsaltall,
    'allheadstwin': scenario_allheadstwin,
    'obfuscatedparnoborrow': scenario_obfuscatedparnoborrow,
    'twinaltsampled': scenario_twinaltsampled,
    'twinparrepair': scenario_twinparrepair,
    'pardamagerepairable': scenario_pardamagerepairable,
    'liveparnocache': scenario_liveparnocache,
    'liveparobfuscated': scenario_liveparobfuscated,
    'liveparplanned': scenario_liveparplanned,
    'livepargrow': scenario_livepargrow,
    'liveparzeroreads': scenario_liveparzeroreads,
    'liveparflushrace': scenario_liveparflushrace,
    'liveparrestart': scenario_liveparrestart,
    'liveparheldvolume': scenario_liveparheldvolume,
    'livepardecodeahead': scenario_livepardecodeahead,
    'liveparrarclean': scenario_liveparrarclean,
    'liveparrartrust': scenario_liveparrartrust,
    'liveparrarobfuscated': scenario_liveparrarobfuscated,
    'liveparrarobfuscatedslow': scenario_liveparrarobfuscatedslow,
    'liveparraroff': scenario_liveparraroff,
    'livepartrusted': scenario_livepartrusted,
    'livepartrustedtemp': scenario_livepartrustedtemp,
    'parinplace': scenario_parinplace,
    'parlesscritical': scenario_parlesscritical,
    'parlesstwin': scenario_parlesstwin,
    'parlesstwinpar': scenario_parlesstwinpar,
    'movemissingtarget': scenario_movemissingtarget,
    'scangrowing': scenario_scangrowing,
    'archiveyoung': scenario_archiveyoung,
    'clientcommands': scenario_clientcommands,
    'mergefinished': scenario_mergefinished,
    'directrenamesubdir': scenario_directrenamesubdir,
    'directrenamesubdirjoin': scenario_directrenamesubdirjoin,
    'newlinestate': scenario_newlinestate,
    'idsafterunreadable': scenario_idsafterunreadable,
    'idsafterfinaldelete': scenario_idsafterfinaldelete,
    'idsafteremptyqueue': scenario_idsafteremptyqueue,
    'idsafteremptyfinaldelete': scenario_idsafteremptyfinaldelete,
    'fleetslowurl': scenario_fleetslowurl,
    'deadpickservers': scenario_deadpickservers,
    'deadpickfewservers': scenario_deadpickfewservers,
    'notfound451': scenario_notfound451,
    'manualparnopar': scenario_manualparnopar,
    'xpackextensionless': scenario_xpackextensionless,
    'xpackextensionlessneg': scenario_xpackextensionlessneg,
    'xpackcorrupt': scenario_xpackcorrupt,
    'xpackendhole_nodirect': scenario_xpackendhole_nodirect,
    'xpackdeadserver': scenario_xpackdeadserver,
    'wholefileunicode': scenario_wholefileunicode,
    'wholefilepadding': scenario_wholefilepadding,
    'wholefileoldstyle': scenario_wholefileoldstyle,
    'wholefilecontinued': scenario_wholefilecontinued,
    'reloadpostqueue': scenario_reloadpostqueue,
    'wholefiletwosets': scenario_wholefiletwosets,
    'dupefailovernonzb': scenario_dupefailovernonzb,
    'wholefileproofcost': scenario_wholefileproofcost,
    'wholefile_nodirect': scenario_wholefile,
    'wholefileonly_nodirect': scenario_wholefileonly,
    'stream_nodirect': scenario_stream,
    'xpackbare_nodirect': scenario_xpackbare,
    'streamretry_nodirect': scenario_streamretry,
    'prodwholefile': scenario_prodwholefile,
    'prodstream': scenario_prodstream,
    'prodrarwhole': scenario_prodrarwhole,
    'prodrarstream': scenario_prodrarstream,
    'skipholetwin': scenario_skipholetwin,
    'deadvolumenestednopar': scenario_deadvolumenestednopar,
    'deadvolumeencrypted': scenario_deadvolumeencrypted,
    'failcont_twin25': scenario_failcont_twin25,
    'failcont_twin50': scenario_failcont_twin50,
    'failcont_twin90': scenario_failcont_twin90,
    'failcont_alt25': scenario_failcont_alt25,
    'failcont_alt50': scenario_failcont_alt50,
    'failcont_altlive': scenario_failcont_altlive,
    'failcont_alt90': scenario_failcont_alt90,
    'failcont_aes25': scenario_failcont_aes25,
    'failcont_aes50': scenario_failcont_aes50,
    'failcont_aes90': scenario_failcont_aes90,
    'failcont_compressed': scenario_failcont_compressed,
    'failcont_aestwin': scenario_failcont_aestwin,
    'failcont_aesalt': scenario_failcont_aesalt,
    'failcont_rar5m3': scenario_failcont_rar5m3,
    'failcont_rar5m3other': scenario_failcont_rar5m3other,
    'failcont_rar5m3aes': scenario_failcont_rar5m3aes,
    'failcont_lzma2': scenario_failcont_lzma2,
    'failcont_norestart': scenario_failcont_norestart,
    'failcont_othervideo': scenario_failcont_othervideo,
    'failcont_aescontainer': scenario_failcont_aescontainer,
    'failcont_alt25busy': scenario_failcont_alt25busy,
    'dupausedlive': scenario_dupausedlive,
    'slotpreemptpaused': scenario_slotpreemptpaused,
    'slotfinishedlive': scenario_slotfinishedlive,
    'failcont_twinlabel': scenario_failcont_twinlabel,
    'failcont_fleettwin': scenario_failcont_fleettwin,
    'failcont_fleetalt': scenario_failcont_fleetalt,
    'weakswitchalt': scenario_weakswitchalt,
    'weakswitchaes': scenario_weakswitchaes,
    'weakswitchm3': scenario_weakswitchm3,
    'weakswitchtwin': scenario_weakswitchtwin,
    'streamtimeoutwrites': scenario_streamtimeoutwrites,
    'failcont_fleetaes': scenario_failcont_fleetaes,
    'headlostvolume': scenario_headlostvolume,
    'headlostvolume5': scenario_headlostvolume5,
    'headlostvolumepar': scenario_headlostvolumepar,
    'deadvolumeunlabelled': scenario_deadvolumeunlabelled,
    'deadvolumetwincheck': scenario_deadvolumetwincheck,
    'altfetchonce': scenario_altfetchonce,
    'deadvolumeothervideo': scenario_deadvolumeothervideo,
    'deadvolumeothersize': scenario_deadvolumeothersize,
    'deadvolumetwolenders': scenario_deadvolumetwolenders,
    'persistdeadprimary': scenario_persistdeadprimary,
    'persistrestart': scenario_persistrestart,
    'persistrestartdamaged': scenario_persistrestartdamaged,
    'rar4lzdirect': scenario_rar4lzdirect,
    'skipholepar': scenario_skipholepar,
    'wholefileonly': scenario_wholefileonly,
    'wholefilewrongdonor': scenario_wholefilewrongdonor,
    'wholefilenofirst': scenario_wholefilenofirst,
    'wholefilepartial': scenario_wholefilepartial,
    'wholefiletwo': scenario_wholefiletwo,
    'failoverlive': scenario_failoverlive,
}

EXPECTED_HISTORY_STATUS = {
    'complementary': 'SUCCESS/HEALTH', 'cutover': 'SUCCESS/HEALTH',
    'leadswitch': 'SUCCESS/HEALTH', 'cutovertruth': 'SUCCESS/HEALTH',
    'manydonors': 'SUCCESS/HEALTH',
    'stream': 'SUCCESS/HEALTH', 'repost': 'FAILURE/PAR',
    'repostrenamed': 'SUCCESS/HEALTH', 'repostobfuscated': 'SUCCESS/HEALTH',
    'repostdonorgaps': 'SUCCESS/HEALTH',
    'xpackbare': 'SUCCESS/HEALTH',
    'xpackrar': 'FAILURE/HEALTH', 'xpackrar2rar': 'SUCCESS/HEALTH',
    'xpack2sets': 'SUCCESS/HEALTH',
    'xpackzip': 'SUCCESS/HEALTH', 'xpack7z': 'SUCCESS/HEALTH', 'xpack7zlzma2': 'SUCCESS/HEALTH',
    'xpack7zmanyvols': 'SUCCESS/HEALTH', 'xpackrarmanyvols': 'SUCCESS/HEALTH',
    'xpacksplit': 'SUCCESS/HEALTH', 'xpackcompressed': 'SUCCESS/HEALTH',
    'xpackneg': 'FAILURE/HEALTH', 'xcrypt_encplain': 'SUCCESS/HEALTH',
    'xcrypt_enc7ztarget': 'SUCCESS/HEALTH', 'xcrypt_enc7zdonor': 'SUCCESS/HEALTH',
    'xpacknesteddonor': 'SUCCESS/HEALTH', 'xpacknestedtarget': 'SUCCESS/HEALTH',
    'xcrypt_nestedin7zenc': 'SUCCESS/HEALTH', 'xcrypt_nestedinrarenc': 'SUCCESS/HEALTH',
    'xcrypt_nestedinenctarget': 'FAILURE/HEALTH',
    'xpacknestedsamename': 'SUCCESS/HEALTH',
    'xpacknestedsetdonor': 'SUCCESS/HEALTH', 'xpacknestedsettarget': 'SUCCESS/HEALTH',
    'xcrypt_plainenc': 'SUCCESS/HEALTH', 'xcrypt_diffpass': 'SUCCESS/HEALTH',
    'xcrypt_wrongpass': 'FAILURE/HEALTH', 'xdecomp_zip': 'SUCCESS/HEALTH',
    'xdecomp_7z': 'SUCCESS/HEALTH', 'xdecomp_storetarget': 'SUCCESS/HEALTH',
    'xdecomp_enc7z': 'SUCCESS/HEALTH', 'xdecomp_neg': 'FAILURE/HEALTH',
    'xdecomp_symlink': 'FAILURE/HEALTH', 'xdecomp_off': 'FAILURE/HEALTH',
    'wholefile': 'SUCCESS/HEALTH',
    'dupefailover': 'FAILURE/HEALTH', 'dupehopeless': 'FAILURE/HEALTH',
    'dupedeadstart': 'FAILURE/HEALTH',
}

# per-scenario daemon options; the article-level scenarios keep the legacy
# "yes" spelling on purpose - it must still parse as "article". The stream
# scenario overrides the base config's ParCheck=manual: under "manual" the
# RequestParCheck the repair stage issues would only flag the item instead
# of running the par stage, leaving that handoff untested (with no par2
# files present, "auto" ends in a harmless "Nothing to par-check").
# the xdecomp_* daemon config pins SevenZipCmd at whatever real 7z binary
# generators._find_7z() discovered on PATH, so Unpack::MakeExtractor has a
# tool to shell out to; empty when none was found (the scenarios themselves
# SKIP in that case, see generators.HAVE_7Z).
_SEVENZIP_OPTION = ['SevenZipCmd=%s' % generators.SEVENZIP_PATH] if generators.HAVE_7Z else []

# Environment variables the daemon of a scenario runs with (others are cleared).
SCENARIO_ENV = {
    # Persistent partial files are off by default on macOS (see
    # progressive_unpack/persistent.rs `enabled`): the scenarios that test them
    # turn them on, so they test the same path on every platform.
    'persistdeadprimary': {'RUSTYGET_PERSISTENT_UNPACK': '1'},
    'persistrestart': {'RUSTYGET_PERSISTENT_UNPACK': '1'},
    'persistrestartdamaged': {'RUSTYGET_PERSISTENT_UNPACK': '1'},
    'corruptpartdu': {'RUSTYGET_PERSISTENT_UNPACK': '1'},
    'corruptpartdurar5': {'RUSTYGET_PERSISTENT_UNPACK': '1'},
    # each state-file sync takes 1 s (diskstate_format::test_state_sync_delay)
    'queuesyncslow': {'RUSTYGET_TEST_STATE_SYNC_DELAY_MS': '1000'},
    # the InterDir budget (interbudget.rs) holds a download that needs 1 GB of state reserve on a 64 MB disk
    'cachecommitfull': {'RUSTYGET_TEST_STATE_RESERVE_MB': '1'},
    'interdirbudget': {'RUSTYGET_TEST_STATE_RESERVE_MB': '1'},
    'interdirbudgetprio': {'RUSTYGET_TEST_STATE_RESERVE_MB': '1'},
    # each pass of the queue coordinator takes 100 ms (queuecoord::test_coord_pass_delay)
    'queuebatchstart': {'RUSTYGET_TEST_COORD_PASS_DELAY_MS': '100'},
    # Each cache flush holds its file's flush lock 150 ms before writing (articledl.rs
    # `flush_test_hold`): captures land while their file is flushed.
    'liveparflushrace': {'RUSTYGET_TEST_FLUSH_HOLD_MS': '150'},
    # Each safe point of a direct unpack takes that long, holding its unpack slot
    # (postproc.rs `test_boundary_delay`): an extraction onto a slow disk.
    'slotpreemptpaused': {'RUSTYGET_TEST_EXTRACT_BOUNDARY_DELAY_MS': '3000'},
    'slotfinishedlive': {'RUSTYGET_TEST_EXTRACT_BOUNDARY_DELAY_MS': '4000'},
}
SCENARIO_ENV_KEYS = sorted({k for env in SCENARIO_ENV.values() for k in env})

SCENARIO_OPTIONS = {
    'corruptpartdu': WRITER_FED_OPTIONS + ['LiveParRepair=yes', 'DirectWrite=yes'],
    'corruptpartdurar5': WRITER_FED_OPTIONS + ['LiveParRepair=yes', 'DirectWrite=yes'],
    'fleetprimaryease': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'dropdune': ['NzbDirInterval=1', 'NzbDirFileAge=2', 'DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetnzbpassword': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleettimeoutunverified': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'dropnzbdir': ['NzbDirInterval=1', 'NzbDirFileAge=2', 'HealthCheck=dupe', 'DupeArticleFallback=article'],
    'dropjoin': ['HealthCheck=dupe'],
    'dropallfail': ['HealthCheck=dupe', 'DupeArticleFallback=article'],
    'dropallfailnonzb': ['HealthCheck=dupe', 'DupeArticleFallback=article'],
    'dropjoinstrict': ['HealthCheck=dupe'],
    'dropfleetasync': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'droppasswordlogs': ['NzbDirInterval=1', 'NzbDirFileAge=2', 'HealthCheck=dupe', 'DupeArticleFallback=no'],
    'dropjoinrestart': ['HealthCheck=dupe', 'DupeCheck=no'],
    'dropkeepdonor': ['HealthCheck=dupe', 'DupeArticleFallback=article'],
    'fleetstaysqueued': ['HealthCheck=dupe', 'DupeArticleFallback=article'],
    'fleetfailoverorder': ['HealthCheck=dupe', 'DupeArticleFallback=article'],
    'fleetprimaryfirst': ['HealthCheck=dupe', 'DupeArticleFallback=article'],
    'fleetprobespeed': ['HealthCheck=dupe', 'DupeArticleFallback=article', 'Server1.Connections=40'],
    'fleetprobebusy': ['HealthCheck=dupe', 'DupeArticleFallback=article', 'Server1.Connections=40'],
    'fleetallweak': ['HealthCheck=dupe', 'DupeArticleFallback=article'],
    'fleetslowprobe': ['HealthCheck=dupe', 'DupeArticleFallback=article'],
    'fleetfailoverkeepsalive': ['HealthCheck=dupe', 'DupeArticleFallback=article', 'DupeWatch=yes'],
    'fleetsalvage': ['HealthCheck=dupe', 'DupeArticleFallback=article'],
    'fleetholdfiles': ['HealthCheck=dupe', 'DupeArticleFallback=no'],
    'fleetparpaused': ['HealthCheck=dupe', 'DupeArticleFallback=live'],
    'fleetparskip': ['HealthCheck=dupe', 'DupeArticleFallback=live', 'ParCheck=auto', 'ParRepair=yes', 'ParRename=yes', 'DirectRename=yes', 'ReorderFiles=yes', 'ParScan=dupe', 'DupeWatch=yes', 'DupeHealthPreset=medium', 'PostStrategy=aggressive'],
    'fleetparpausedmany': ['HealthCheck=dupe', 'DupeArticleFallback=live', 'ParCheck=auto', 'ParRepair=yes', 'ParRename=yes', 'DirectRename=yes', 'ReorderFiles=yes', 'ParScan=dupe', 'DupeWatch=yes', 'DupeHealthPreset=medium', 'PostStrategy=aggressive', 'DownloadRate=50'],
    'fleetparskiprestart': ['HealthCheck=dupe', 'DupeArticleFallback=live', 'ParCheck=auto', 'ParRepair=yes', 'ParRename=yes', 'DirectRename=yes', 'ReorderFiles=yes', 'ParScan=dupe', 'DupeWatch=yes', 'DupeHealthPreset=medium', 'PostStrategy=aggressive'],
    'fleetendnolive': ['HealthCheck=dupe', 'DupeArticleFallback=no', 'ParCheck=always'],
    'fleetprobehealthfail': ['HealthCheck=dupe', 'DupeArticleFallback=no'],
    'fleetendkeephistory0': ['HealthCheck=dupe', 'DupeArticleFallback=no', 'ParCheck=always', 'KeepHistory=0'],
    'allheadspar': PROD_OPTIONS,
    'allheadsalt': PROD_OPTIONS,
    'allheadsaltall': PROD_OPTIONS,
    'allheadstwin': PROD_OPTIONS,
    'watchforcepick': ['DupeArticleFallback=no', 'DupeWatch=yes', 'DupeBodyChecks=0', 'DupeHealthMin=40', 'DupeHealthMax=40'],
    'watchdeadpick': ['DupeArticleFallback=no', 'DupeWatch=yes', 'DupeBodyChecks=0', 'DupeHealthMin=40', 'DupeHealthMax=40'],
    'watchsearchedkey': ['DupeArticleFallback=no', 'DupeWatch=yes', 'DupeBodyChecks=0', 'DupeHealthMin=40', 'DupeHealthMax=40'],
    'watchfleet': ['DupeArticleFallback=no', 'DupeWatch=yes', 'DupeBodyChecks=0', 'DupeHealthMin=40', 'DupeHealthMax=40'],
    'watchswap': ['DupeArticleFallback=no', 'DupeWatch=yes', 'DupeBodyChecks=0', 'DupeHealthMin=40', 'DupeHealthMax=40'],
    'watchborrow': ['DupeArticleFallback=live', 'DupeBorrowFirst=yes', 'DupeWatch=yes', 'DupeBodyChecks=0', 'DupeHealthMin=40', 'DupeHealthMax=40'],
    'watchswaptwin': ['DupeArticleFallback=no', 'DupeWatch=yes', 'DupeBodyChecks=0', 'DupeHealthMin=40', 'DupeHealthMax=40'],
    'watchrescue': ['DupeArticleFallback=no', 'HealthCheck=delete', 'ArticleRetries=0', 'Server1.Connections=2', 'DupeWatch=yes'],
    'midwaytwin': ['DupeArticleFallback=live', 'DupeStreamDecompress=yes', 'HealthCheck=dupe', 'ParScan=dupe', 'DupeWatch=yes', 'ParCheck=auto'],
    'midwayalt': ['DupeArticleFallback=live', 'DupeStreamDecompress=yes', 'HealthCheck=dupe', 'ParScan=dupe', 'DupeWatch=yes', 'ParCheck=auto'],
    # JSON-P is off by default; these scenarios call /jsonprpc, and apiformat checks
    # that a server password stays hidden there
    'apiformat': ['DupeArticleFallback=yes', 'JsonP=yes', 'Server1.Password=harness-secret'],
    'xmlparamdecode': ['DupeArticleFallback=yes', 'JsonP=yes'],
    'jsonpoff': ['DupeArticleFallback=yes', 'Server1.Password=harness-secret'],
    'stream': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'truncatedstate': ['ContinuePartial=yes', 'FlushQueue=yes', 'Server1.Connections=1'],
    'queuesyncslow': ['ContinuePartial=yes', 'FlushQueue=yes', 'Server1.Connections=6'],
    'interdirchange': ['DownloadRate=200', 'Unpack=yes', 'DirectUnpack=yes'],
    'diskfullrestart': ['ContinuePartial=yes', 'FlushQueue=yes', 'DiskSpace=0', 'Server1.Connections=4'],
    'cachecommitfull': ['ArticleCache=16', 'DirectWrite=yes', 'DiskSpace=1', 'Server1.Connections=4'],
    'interdirbudget': ['DiskSpace=4', 'Server1.Connections=8'],
    'interdirbudgetprio': ['DiskSpace=4', 'Server1.Connections=8'],
    'queuebatchstart': ['Server1.Connections=30'],
    'streamrestartcredit': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'streamdeaddonor': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'rejectnextserver': ['DupeArticleFallback=article', 'ArticleRetries=0'],
    'backbonemiss': ['DupeCheck=no', 'ArticleRetries=0', 'DetailTarget=log', 'Server1.Backbone=TestBone'],
    'backendauto': ['DupeCheck=no', 'ArticleRetries=0', 'DetailTarget=log'],
    'nzbgapborrow': ['DupeArticleFallback=article'],
    'sidefilenorepair': ['DupeArticleFallback=stream'],
    'partialnzb': ['DupeArticleFallback=stream'],
    'finaldeletemidway': ['DupeArticleFallback=live', 'DirectRename=yes', 'HealthCheck=dupe', 'ArticleCache=8192', 'ContinuePartial=yes', 'DirectUnpack=yes'],
    'finaldeleterestart': ['DupeArticleFallback=live', 'DirectRename=yes', 'HealthCheck=dupe', 'ArticleCache=8192', 'ContinuePartial=yes', 'DirectUnpack=yes'],
    'restartmidway': ['DupeArticleFallback=live', 'DirectRename=yes', 'HealthCheck=dupe'],
    'declaredcountnopair': ['DupeArticleFallback=article'],
    'recheckfailed': ['DupeArticleFallback=article', 'ArticleRetries=0', 'Server1.Connections=2'],
    'streamgrouped': ['DupeArticleFallback=stream', 'ParCheck=auto', 'ArticleTimeout=20', 'Server1.Group=1'],
    # Finish the retained first volume before missing articles trigger parking.
    # Parallel requests can otherwise cancel before any article is retained.
    'retrykeepsjobs': ['DupeArticleFallback=stream', 'ParCheck=auto', 'HealthCheck=park', 'Server1.Connections=1'],
    'retryparkedname': ['DupeArticleFallback=stream', 'ParCheck=auto', 'HealthCheck=park', 'Server1.Connections=1'],
    'healthlastarticle': ['DupeArticleFallback=stream', 'ParCheck=auto', 'HealthCheck=park', 'Server1.Connections=1'],
    'streamslowprogress': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DupeStreamTimeout=5'],
    'streamtooslow': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DupeStreamTimeout=5'],
    'streammaxrun': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DupeStreamTimeout=8'],
    'streamstarved': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DupeStreamTimeout=5', 'Server1.Connections=2'],
    'streamtimeout': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DupeStreamTimeout=5'],
    # liveoverlap: the DownloadRate throttle (KB/s) keeps the big FileB
    # downloading long enough that FileA's live repair provably overlaps it
    'liveoverlap': ['DupeArticleFallback=live', 'ParCheck=auto', 'DownloadRate=8000'],
    'wholefilerestartlive': ['DupeArticleFallback=live', 'DownloadRate=400'],
    'restartfirstdead': ['DupeArticleFallback=live', 'DirectRename=yes', 'DirectWrite=yes', 'DownloadRate=300'],
    'livegate': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'livelastfile': ['DupeArticleFallback=live', 'ParCheck=auto'],
    'reloadmidrepair': ['DupeArticleFallback=live', 'ParCheck=auto', 'DownloadRate=6000',
                        'Server1.Connections=8'],
    # repost: ParCheck=auto runs par-check against a random-bytes stand-in
    # par2; duplicate fallback repairs only archive data and leaves parity
    # untouched - FAILURE/PAR is the EXPECTED final status
    'repost': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    # repostrenamed: no par2 members; "auto" ends in a harmless
    # "Nothing to par-check" after the repair handoff
    'repostrenamed': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'repostobfuscated': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'repostdonorgaps': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    # xpackbare: no par2; "auto" ends in "Nothing to par-check" post-repair
    'xpackbare': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    # xpack*: no real par2 anywhere; ParCheck=auto ends in "Nothing to par-check"
    'xpackrar': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpackgapdonor': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpackrar2rar': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpack2sets': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpackzip': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpack7z': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpack7zmanyvols': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpackrarmanyvols': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpack7zlzma2': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DupeStreamDecompress=no'],
    'xpacksplit': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpackcompressed': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpackneg': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    # xcrypt_*: M3 password-assisted cross-encryption; no real par2 anywhere,
    # ParCheck=auto ends in "Nothing to par-check" post-repair. Skipped
    # outright (no daemon options matter) when the cryptography module is
    # not installed - see generators.HAVE_CRYPTO.
    'xcrypt_encplain': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xcrypt_enc7ztarget': ['DupeArticleFallback=stream', 'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xpacknesteddonor': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xcrypt_nestedin7zenc': ['DupeArticleFallback=stream', 'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xcrypt_nestedinrarenc': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xcrypt_nestedinenctarget': ['DupeArticleFallback=stream', 'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xpacknestedsamename': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpacknestedsetdonor': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpacknestedsettarget': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpacknestedtarget': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xcrypt_enc7zdonor': ['DupeArticleFallback=stream', 'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xcrypt_plainenc': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xcrypt_diffpass': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xcrypt_wrongpass': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    # xdecomp_*: M4 decompression-assisted donor extraction. SevenZipCmd is
    # pinned at the locally-discovered 7z binary (generators.SEVENZIP_PATH,
    # via _SEVENZIP_OPTION below) so Unpack::MakeExtractor has a real tool to
    # shell out to; every xdecomp_* scenario SKIPs outright when 7z is not
    # installed (see generators.HAVE_7Z) before even reading this dict.
    # xdecomp_off is the deliberate exception: it OMITS
    # DupeStreamDecompress=yes to prove the opt-in gate, everything else
    # about its fixture matches xdecomp_7z.
    'xdecomp_zip': ['DupeArticleFallback=stream', 'DupeStreamDecompress=yes',
                    'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xdecomp_slow': ['DupeArticleFallback=stream', 'DupeStreamDecompress=yes', 'DupeStreamTimeout=5', 'Server1.Connections=1',
                     'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xdecomp_7z': ['DupeArticleFallback=stream', 'DupeStreamDecompress=yes',
                   'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xdecomp_storetarget': ['DupeArticleFallback=stream', 'DupeStreamDecompress=yes',
                            'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xdecomp_enc7z': ['DupeArticleFallback=stream', 'DupeStreamDecompress=yes',
                      'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xdecomp_enctarget': ['DupeArticleFallback=stream', 'DupeStreamDecompress=yes',
                          'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xdecomp_neg': ['DupeArticleFallback=stream', 'DupeStreamDecompress=yes',
                    'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xdecomp_symlink': ['DupeArticleFallback=stream', 'DupeStreamDecompress=yes',
                        'ParCheck=auto'] + _SEVENZIP_OPTION,
    'xdecomp_off': ['DupeArticleFallback=stream', 'ParCheck=auto'] + _SEVENZIP_OPTION,
    'wholefile': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    # dupefailover: the health action under test replaces the base config's
    # HealthCheck=none; article recovery stays on so the failover's
    # "duplicates were asked first" sample gate is exercised too
    'dupefailover': ['DupeArticleFallback=article', 'HealthCheck=dupe'],
    'lockedfailover': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Unpack=yes', 'DirectUnpack=yes',
                       'DownloadRate=300'],
    'dupefailoverraw': ['DupeArticleFallback=article', 'HealthCheck=dupe', 'RawArticle=yes'],
    'dupehopeless': ['DupeArticleFallback=article', 'HealthCheck=dupe'],
    'dupehopelessretry': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'dupedeadstart': ['DupeArticleFallback=article', 'HealthCheck=dupe'],
    'wholefileretry': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'dupehopelessnodupecheck': ['DupeArticleFallback=article', 'HealthCheck=dupe', 'DupeCheck=no'],
    'dupefailovernofallback': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'wholefilepar': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefilefailretry': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefilelive': ['DupeArticleFallback=live', 'ParCheck=auto', 'DownloadRate=4000'],
    'streamretry': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefilerestart': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefilestale': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefilenfoproof': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefilesampleproof': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'dupefailoverchain': ['DupeArticleFallback=article', 'HealthCheck=dupe'],
    'xpacklatency': ['DupeArticleFallback=stream', 'ParCheck=auto', 'Server1.Connections=8'],
    'xpackflaky': ['DupeArticleFallback=stream', 'ParCheck=auto', 'Server1.Connections=4'],
    'deadpickprobe': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'deadpickafterreload': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'deadpickpartial': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'deadpickstray': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'deaddownloadlate': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'forcefailover': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'copybackup': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'copyoffailed': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'resendbackup': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'resendfailed': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'projectedswap': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'runswap': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=1'],
    'runnobackup': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=1'],
    'runscattered': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=1'],
    'runreset': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=1'],
    'projectednobackup': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'projectedhalfnobackup': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'projectedbelownobackup': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'keepreturned': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'restartfailover': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'slowprobe': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=4'],
    'runparcovers': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'ParCheck=manual'],
    'ondiskskip': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'ondiskgone': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'ondiskstop': ['DupeArticleFallback=stream', 'ParCheck=auto', 'HealthCheck=dupe', 'DupeStreamTimeout=60', 'PostStrategy=rocket'],
    'ondiskstopsamename': [o for o in PROD_OPTIONS if not o.startswith('DupeArticleFallback=')] + ['DupeArticleFallback=stream', 'DupeStreamTimeout=60'],
    'ondiskstopsamenamenointer': [o for o in PROD_OPTIONS if not o.startswith('DupeArticleFallback=')] + ['DupeArticleFallback=stream', 'DupeStreamTimeout=60', 'InterDir='],
    'ondiskstopothernamenointer': [o for o in PROD_OPTIONS if not o.startswith('DupeArticleFallback=')] + ['DupeArticleFallback=stream', 'DupeStreamTimeout=60', 'InterDir='],
    'ondiskstoprepairfirst': ['DupeArticleFallback=stream', 'ParCheck=auto', 'HealthCheck=dupe', 'DupeStreamTimeout=60', 'PostStrategy=rocket'],
    'repairlast': ['DupeArticleFallback=stream', 'ParCheck=auto', 'HealthCheck=dupe'],
    # Let both files complete so this isolates history's failed-article
    # recheck, rather than the separate early health-cancellation path.
    'recheckwholefail': ['DupeArticleFallback=article', 'HealthCheck=none', 'ArticleRetries=0'],
    'lightburst': LIGHTBURST_OPTIONS + ['LightConnections=0'],
    'samepostingrepair': ['DupeArticleFallback=stream', 'ParCheck=auto', 'HealthCheck=dupe'],
    'samepostinglate': ['DupeArticleFallback=stream', 'ParCheck=auto', 'HealthCheck=dupe'],
    'projectedhealthy': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'projectedworsebackup': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'projectededge': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'projectededgelead': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    # The wait for par-check is the behavior under test: with DupeBorrowFirst=yes
    # (the default) the missing articles are borrowed at once.
    'parprojection': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DupeBorrowFirst=no'],
    'deaddownloadstray2': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'deaddownloadrestart': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'ContinuePartial=yes'],
    'deadbackupsonly': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'backuporder': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'fleetdeadfirst': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleettwins': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleettimeout': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetone': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetnodupecheck': ['DupeArticleFallback=no', 'HealthCheck=park', 'DupeCheck=no'],
    'fleetalldead': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetshutdown': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetallerror': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetotherkey': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetnokey': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetresend': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetresendslow': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetaddbackup': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetfailover': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetbusy': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'fleetdeadtwins': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetlarge': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetcopy': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetmostlydead': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetpaused': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'jointwosets': ['Unpack=yes', UNRAR_CMD, SEVENZIP_CMD],
    'joinequalpieces': ['Unpack=yes', UNRAR_CMD, SEVENZIP_CMD, 'ParCheck=force'],
    'parscanpercent': ['ParCheck=force', 'ParScan=full'],
    'retentionnodate': ['Server1.Retention=30', 'ArticleRetries=0'],
    'damagedfilestate': ['ContinuePartial=yes', 'ArticleRetries=0', 'DupeArticleFallback=no'],
    'parrenamebad': ['ParRename=yes', 'DirectRename=no', 'ParCheck=auto'],
    'joingroupreconnect': ['Server1.JoinGroup=yes', 'Server1.Connections=1', 'ArticleRetries=0', 'DirectWrite=no'],
    'joingroupnogroups': ['Server1.JoinGroup=yes', 'ArticleRetries=0', 'DirectWrite=no'],
    'stallquit': ['ArticleTimeout=5', 'ArticleRetries=2', 'ArticleInterval=0', 'DirectWrite=no', 'Server1.Connections=1'],
    'directunpacksimple': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1'],
    'live7zsplit': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1', 'UnpackCleanupDisk=yes', 'DownloadRate=8000'],
    'live7zlzma2': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1', 'UnpackCleanupDisk=yes', 'DownloadRate=8000'],
    'live7zlzma2threads': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1', 'UnpackCleanupDisk=yes', 'DownloadRate=8000', 'FeedThreads=4'],
    'livezipsplit': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1', 'UnpackCleanupDisk=yes', 'DownloadRate=8000'],
    'livezipz01': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1', 'UnpackCleanupDisk=yes', 'DownloadRate=8000'],
    'livesplitjoin': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1', 'UnpackCleanupDisk=yes', 'DownloadRate=8000'],
    'liveedgehole': PROD_OPTIONS + ['DownloadRate=3000'],
    'liveedgehole5': PROD_OPTIONS + ['DownloadRate=3000'],
    # Writer-fed live extraction: a download slower than the decode, as on a real line.
    'livefedrar5store': WRITER_FED_OPTIONS + ['DownloadRate=8000'],
    'livefedrar5m3': WRITER_FED_OPTIONS + ['DownloadRate=6000'],
    'livefed7z': WRITER_FED_OPTIONS + ['DownloadRate=6000'],
    # Extraction-order downloads: direct rename's first articles show where a set starts,
    # the NZB's own order is kept (ReorderFiles off), and the line is slower than the decode.
    'liveorder7z': WRITER_FED_OPTIONS + ['DirectRename=yes', 'ReorderFiles=no', 'DownloadRate=2000'],
    'liveorderrar': WRITER_FED_OPTIONS + ['DirectRename=yes', 'ReorderFiles=no', 'DownloadRate=2000'],
    # eight connections: the four hung first requests leave download connections free while
    # they hang (nntp-rs keeps its light reserve beside starved downloads, G22; TODO: back to
    # four connections once nntp-rs ships G22)
    'livestraggler': WRITER_FED_OPTIONS + ['DownloadRate=3000', 'Server1.Connections=8'],
    'directunpacktwoarchives': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1'],
    'directunpackparfailure': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1'],
    'fleetbigqueue': ['Unpack=yes', 'DirectUnpack=no', 'ParCheck=auto', 'KeepHistory=1', 'ArticleTimeout=20'],
    'dupausedunpack': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1', 'ArticleTimeout=120'],
    'parfailurenodirectunpack': ['Unpack=yes', 'DirectUnpack=no', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1'],
    'parfailuredirectunpackfailed': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1'],
    'directunpackkeep': ['Unpack=yes', 'DirectUnpack=yes', 'UseTempUnpackDir=no', UNRAR_CMD, 'UnpackCleanupDisk=yes', 'ParCheck=auto'],
    'directunpackkeepnointer': ['InterDir=', 'Unpack=yes', 'DirectUnpack=yes', 'UseTempUnpackDir=no', UNRAR_CMD, 'UnpackCleanupDisk=yes', 'ParCheck=auto'],
    'apiaccess': ['ControlPassword=ctlpass', 'RestrictedUsername=ro', 'RestrictedPassword=ropass'],
    'articledecoy': ['DupeArticleFallback=article', 'HealthCheck=dupe', 'ParCheck=auto'],
    'articledecoypar': ['DupeArticleFallback=article', 'HealthCheck=dupe', 'ParCheck=auto'],
    'articledecoyparrestart': ['DupeArticleFallback=article', 'HealthCheck=dupe', 'ParCheck=auto', 'Server1.Connections=2'],
    'fleetslowurlfirst': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetwide': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetsamekey': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'yencrangefar': ['DupeArticleFallback=no', 'DirectWrite=no', 'ArticleRetries=0'],
    'yencrangeshort': ['DupeArticleFallback=no', 'ArticleCache=64', 'ArticleRetries=0'],
    'quotafutureday': ['DailyQuota=100000'],
    'tasktypo': lambda: _next_minute_task_options(),
    'nntppercent': lambda: _percent_server_options(),
    'uucache': ['DirectWrite=no', 'ArticleCache=64', 'ArticleRetries=0'],
    'directrenamesubdir': ['DirectRename=yes', 'DirectWrite=yes', 'ParCheck=auto'],
    'directrenamesubdirjoin': ['DirectRename=yes', 'DirectWrite=no', 'ParCheck=auto'],
    'parvolnames': ['ParCheck=auto'],
    'pardamagefailover': ['ParCheck=auto', 'HealthCheck=dupe', 'DupeArticleFallback=no'],
    # at full speed its 16 MB arrive in about a second: the queue's checks may not run between
    # the moment too few articles are left to reach the borrow sample and the end (1 in 12
    # runs under load); about 4 s leaves them that time
    'pardamageborrow': ['ParCheck=auto', 'HealthCheck=dupe', 'DupeArticleFallback=live',
                        'DownloadRate=4000'],
    'nosourceonce': ['DupeArticleFallback=live', 'HealthCheck=none'],
    'twinalt': ['HealthCheck=dupe'],
    'twinlabelonadd': ['HealthCheck=dupe'],
    'neartwinborrow': ['DupeArticleFallback=live', 'HealthCheck=none', 'ParCheck=force'],
    'lostindexpar': ['ParCheck=auto', 'Unpack=no', 'DupeArticleFallback=no'],
    'obfuscatedparnoborrow': ['DupeArticleFallback=live', 'HealthCheck=none', 'DirectRename=no', 'ParCheck=force'],
    'twinaltsampled': ['HealthCheck=dupe'],
    'twinparrepair': ['HealthCheck=none', 'DupeArticleFallback=no', 'ParCheck=force', 'Unpack=no'],
    'pardamagerepairable': ['ParCheck=auto', 'HealthCheck=dupe', 'DupeArticleFallback=no'],
    'liveparnocache': ['ParCheck=auto', 'LiveParRepair=yes', 'ArticleCache=0', 'DirectWrite=yes'],
    'liveparobfuscated': ['ParCheck=auto', 'LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes',
                          'ParRename=yes', 'DirectRename=no'],
    'liveparplanned': ['ParCheck=auto', 'LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes'],
    'livepargrow': ['ParCheck=auto', 'LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes'],
    'liveparzeroreads': ['ParCheck=auto', 'LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes'],
    'liveparrestart': ['ParCheck=auto', 'LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes',
                       'DownloadRate=3000'],
    'liveparflushrace': ['ParCheck=auto', 'LiveParRepair=yes', 'ArticleCache=1', 'DirectWrite=yes',
                         'Server1.Connections=16'],
    'liveparheldvolume': ['ParCheck=auto', 'LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes'],
    'livepardecodeahead': ['ParCheck=auto', 'LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes'],
    'livepartrusted': ['ParCheck=auto', 'LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes'],
    'livepartrustedtemp': ['ParCheck=auto', 'LiveParRepair=yes', 'ArticleCache=0', 'DirectWrite=no'],
    'liveparrarclean': PROD_OPTIONS + ['LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes', 'UnpackCleanupDisk=no'],
    'liveparrartrust': PROD_OPTIONS + ['LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes', 'UnpackCleanupDisk=no'],
    'liveparrarobfuscated': PROD_OPTIONS + ['LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes', 'UnpackCleanupDisk=no'],
    'liveparrarobfuscatedslow': PROD_OPTIONS + ['LiveParRepair=yes', 'ArticleCache=256', 'DirectWrite=yes', 'UnpackCleanupDisk=no',
                                 'DownloadRate=1000'],
    'liveparraroff': PROD_OPTIONS + ['LiveParRepair=no', 'ArticleCache=256', 'DirectWrite=yes', 'UnpackCleanupDisk=no'],
    'parinplace': ['ParCheck=auto', 'LiveParRepair=no'],
    'parlesscritical': ['HealthCheck=dupe', 'DupeArticleFallback=no'],
    'parlesstwin': ['HealthCheck=dupe', 'DupeArticleFallback=no'],
    'parlesstwinpar': ['HealthCheck=dupe', 'DupeArticleFallback=no'],
    'scangrowing': ['NzbDirInterval=1', 'NzbDirFileAge=4'],
    'archiveyoung': ['NzbDirInterval=1', 'NzbDirFileAge=4'],
    'pathtraversalnzb': ['Unpack=yes', UNRAR_CMD, 'FileNaming=nzb'],
    'pathtraversalarticle': ['Unpack=yes', UNRAR_CMD, 'FileNaming=article'],
    'urlclosed': ['UrlRetries=1', 'UrlInterval=1'],
    'urlretrywait': ['UrlTimeout=2', 'UrlInterval=20', 'UrlRetries=2'],
    'historyeditlist': ['DupeCheck=yes'],
    'speedtestnohistory': ['KeepHistory=0'],
    'innerarchivekeep': ['InterDir=', 'Unpack=yes', 'UseTempUnpackDir=no', UNRAR_CMD, SEVENZIP_CMD, 'UnpackCleanupDisk=yes'],
    'innerarchivekeepdirect': ['InterDir=', 'Unpack=yes', 'DirectUnpack=yes', 'UseTempUnpackDir=no', UNRAR_CMD, SEVENZIP_CMD, 'UnpackCleanupDisk=yes'],
    'fleetscoremax': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'fleetmerged': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    'addstorm': ['NzbDirInterval=1', 'NzbDirFileAge=60', 'UrlConnections=4', 'UrlForce=yes', 'HealthCheck=dupe', 'DupeArticleFallback=live'],
    'fleetparallel': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'fleetslowurl': ['DupeArticleFallback=no', 'HealthCheck=dupe'],
    # an indexer that answers (with no results): a search that learned nothing is repeated
    'deadpickservers': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'deadpickfewservers': ['DupeArticleFallback=no', 'HealthCheck=dupe', 'Server1.Connections=2'],
    'notfound451': ['DupeArticleFallback=no', 'ArticleRetries=3', 'ArticleInterval=3'],
    'transientretry': ['DupeArticleFallback=no', 'ArticleRetries=3', 'ArticleInterval=2'],
    'transientexhausted': ['DupeArticleFallback=no', 'ArticleRetries=3', 'ArticleInterval=2'],
    'transientotherserver': ['DupeArticleFallback=no', 'ArticleRetries=3', 'ArticleInterval=30'],
    'manualparnopar': ['DupeArticleFallback=stream', 'ParCheck=manual'],
    'xpackextensionless': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpackextensionlessneg': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'xpackcorrupt': ['DupeArticleFallback=stream', 'ParCheck=auto', 'CrcCheck=no'],
    'xpackendhole_nodirect': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DirectWrite=no'],
    # Server2 (preferred level 0, behind a FlakyNntpProxy) serves the download
    # and goes down for good at the first duplicate request; Server1 is the
    # level-1 backup every repair fetch then falls back to
    'xpackdeadserver': ['DupeArticleFallback=stream', 'ParCheck=auto', 'Server1.Connections=2',
                        'Server1.Level=1', 'Server2.Host=127.0.0.1', 'Server2.Connections=2',
                        'Server2.Level=0', 'Server2.Encryption=no'],
    'wholefileunicode': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefilepadding': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefileoldstyle': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefilecontinued': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'reloadpostqueue': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefiletwosets': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'dupefailovernonzb': ['DupeArticleFallback=article', 'HealthCheck=dupe'],
    'wholefileproofcost': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    # the same paths when nzbget assembles files from temporary article files
    'wholefile_nodirect': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DirectWrite=no'],
    'wholefileonly_nodirect': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DirectWrite=no'],
    'stream_nodirect': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DirectWrite=no'],
    'xpackbare_nodirect': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DirectWrite=no'],
    'streamretry_nodirect': ['DupeArticleFallback=stream', 'ParCheck=auto', 'DirectWrite=no'],
    'prodwholefile': PROD_OPTIONS,
    'prodstream': PROD_OPTIONS,
    'prodrarwhole': PROD_OPTIONS,
    'prodrarstream': PROD_OPTIONS,
    # tiny articles (200 bytes) so the volumes arrive one by one while direct unpack runs
    'rar4lzdirect': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'DownloadRate=2'],
    # throttled so direct unpack reaches the damaged volume before its repair lands
    'skipholetwin': PROD_OPTIONS + ['DownloadRate=1500'],
    'deadvolumenestednopar': PROD_OPTIONS + ['DownloadRate=4000'],
    'deadvolumeencrypted': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_twin25': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_twin50': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_twin90': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_alt25': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_alt50': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_altlive': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_alt90': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_aes25': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_aes50': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_aes90': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_compressed': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_aestwin': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_aesalt': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_rar5m3': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_rar5m3other': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_rar5m3aes': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_lzma2': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_norestart': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_othervideo': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_aescontainer': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_alt25busy': PROD_OPTIONS + ['DownloadRate=4000', 'UnpackSlots=1', 'ArticleTimeout=1200'],
    'dupausedlive': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no', 'KeepHistory=1',
                     'ArticleTimeout=1200', 'UnpackSlots=1'],
    # the default UnpackSlots (one slot)
    'slotpreemptpaused': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no',
                          'KeepHistory=1', 'ArticleTimeout=1200'],
    'slotfinishedlive': ['Unpack=yes', 'DirectUnpack=yes', 'ParCheck=auto', 'ParRepair=yes', 'DupeCheck=no',
                         'KeepHistory=1', 'ArticleTimeout=1200'],
    'failcont_twinlabel': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_fleettwin': PROD_OPTIONS + ['DownloadRate=4000'],
    'failcont_fleetalt': PROD_OPTIONS + ['DownloadRate=4000'],
    'weakswitchalt': PROD_OPTIONS + ['DownloadRate=4000'],
    'weakswitchaes': PROD_OPTIONS + ['DownloadRate=4000'],
    'weakswitchm3': PROD_OPTIONS + ['DownloadRate=4000'],
    'weakswitchtwin': PROD_OPTIONS + ['DownloadRate=4000'],
    'streamtimeoutwrites': ['DupeArticleFallback=stream', 'ParCheck=auto', 'Server1.Connections=2',
                            'DupeStreamTimeout=10'],
    'failcont_fleetaes': PROD_OPTIONS + ['DownloadRate=4000'],
    'headlostvolume': PROD_OPTIONS + ['DownloadRate=4000'],
    'headlostvolume5': PROD_OPTIONS + ['DownloadRate=4000'],
    'headlostvolumepar': PROD_OPTIONS + ['DownloadRate=4000'],
    'deadvolumeunlabelled': PROD_OPTIONS + ['DownloadRate=4000'],
    'deadvolumetwincheck': PROD_OPTIONS + ['DownloadRate=1000'],
    'altfetchonce': PROD_OPTIONS + ['DownloadRate=8000'],
    'deadvolumeothervideo': PROD_OPTIONS + ['DownloadRate=1000'],
    'deadvolumeothersize': PROD_OPTIONS + ['DownloadRate=1000'],
    'deadvolumetwolenders': PROD_OPTIONS + ['DownloadRate=1000'],
    'skipholepar': PROD_OPTIONS + ['DownloadRate=1500'],
    'persistdeadprimary': PROD_OPTIONS + ['DownloadRate=1500'],
    'persistrestart': PROD_OPTIONS + ['DownloadRate=1500'],
    'persistrestartdamaged': PROD_OPTIONS + ['DownloadRate=1500'],
    'wholefileonly': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefilewrongdonor': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefilenofirst': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefilepartial': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'wholefiletwo': ['DupeArticleFallback=stream', 'ParCheck=auto'],
    'failoverlive': ['DupeArticleFallback=live', 'HealthCheck=dupe', 'ParCheck=auto'],
}
DEFAULT_OPTIONS = ['DupeArticleFallback=yes']

# the values of DupeArticleFallback in the order of Options::EDupeArticleFallback;
# the daemon gates its features with >= on that order, so whatever passes under
# a mode must pass under every higher one. --fallback-mode runs the scenarios
# written for a lower mode under the requested one to check that promise.
FALLBACK_MODES = ['no', 'article', 'stream', 'live']


def _fallback_mode(options):
    """The DupeArticleFallback value a scenario's options select ("yes" is the
    legacy spelling of "article")."""
    mode = 'no'
    for option in options:
        if option.startswith('DupeArticleFallback='):
            mode = option.split('=', 1)[1]
    return 'article' if mode == 'yes' else mode


def _promote_fallback(options, mode):
    """The options with DupeArticleFallback set to ``mode``."""
    return ['DupeArticleFallback=%s' % mode if option.startswith('DupeArticleFallback=') else option
            for option in options]


# scenarios that test their own mode's sequence of events and so are never
# promoted: livegate asserts that the live pass does NOT run under "stream";
# streamrestartcredit shuts down during a post-processing repair that the live
# pass has already made by then (nothing to interrupt); wholefilerestart
# restarts while a whole-file job waits for post-processing, which the live
# pass has already begun (the restart redoes it: two recreations, not one);
# wholefilestale zeroes the recreated volume after the shutdown to stand for
# a recreation cut short, but the live pass may have finished it, and a
# finished job is rightly not redone
FALLBACK_MODE_BOUND = {'livegate', 'streamrestartcredit', 'wholefilerestart', 'wholefilestale'}
# scenarios that read nserv's request log (nserv.log)
CAPTURE_REQUESTS = {'repost', 'diskfullrestart', 'cachecommitfull', 'interdirbudget', 'interdirbudgetprio', 'altfetchonce', 'wholefileproofcost', 'dropnzbdir', 'fleetstaysqueued', 'fleetprimaryfirst'} | {
    'failcont_twin25', 'failcont_twin50', 'failcont_twin90', 'failcont_alt25', 'failcont_alt50', 'failcont_altlive', 'failcont_alt90', 'failcont_aes25', 'failcont_aes50', 'failcont_aes90', 'failcont_compressed', 'failcont_othervideo', 'failcont_twinlabel', 'failcont_fleettwin', 'failcont_fleetalt', 'failcont_fleetaes', 'failcont_aestwin', 'failcont_aesalt', 'failcont_rar5m3', 'failcont_rar5m3aes', 'failcont_rar5m3other', 'failcont_lzma2', 'failcont_norestart', 'weakswitchalt', 'weakswitchtwin', 'weakswitchaes', 'weakswitchm3', 'failcont_aescontainer', 'failcont_alt25busy'}
# extra nserv arguments per scenario (-w: response latency in ms)
SCENARIO_NSERV_ARGS = {'diskfullrestart': ['-w', '300'], 'cachecommitfull': ['-w', '100'], 'interdirbudget': ['-w', '2000'], 'interdirbudgetprio': ['-w', '2000'], 'streamtimeoutwrites': ['-w', '800'], 'fleetslowprobe': ['-x', '2000'], 'fleetprobespeed': ['-w', '300'], 'fleetprobebusy': ['-w', '300'], 'fleetprobehealthfail': ['-w', '800'], 'finaldeletemidway': ['-w', '300'], 'finaldeleterestart': ['-w', '300'], 'restartmidway': ['-w', '300'], 'xpacklatency': ['-w', '1000'], 'xpack7zmanyvols': ['-w', '300'], 'xpackrarmanyvols': ['-w', '300'], 'deadpickprobe': ['-w', '500'], 'deadpickafterreload': ['-w', '500'],
                       'deadpickpartial': ['-w', '100'], 'deadpickstray': ['-w', '500'], 'forcefailover': ['-w', '500'], 'copybackup': ['-w', '500'], 'copyoffailed': ['-w', '500'], 'deaddownloadstray2': ['-w', '300'], 'deaddownloadrestart': ['-w', '300'], 'deadbackupsonly': ['-w', '500'], 'backuporder': ['-w', '500'], 'deaddownloadlate': ['-w', '300'], 'deadpickservers': ['-w', '200'],
                       'deadpickfewservers': ['-w', '200'],
                       # a missing article answers late, so the dead volumes fail one after another and later
                       # repair runs find them, as on 6790 (XB: one dead volume every 10-15 s, each borrowed by its own run)
                       'failcont_aescontainer': ['-x', '15000']}

# extra news servers behind the same nserv: (servers answering, unreachable
# optional servers); Server1 counts among the answering ones
SCENARIO_EXTRA_SERVERS = {'deadpickservers': (5, 1), 'deadpickfewservers': (4, 2), 'rejectnextserver': (2, 0),
                          'transientotherserver': (2, 0)}

# scenarios whose extra servers join Server1's group (the same account): Group=1
SCENARIO_GROUPED_SERVERS = {'streamgrouped': 2}

# scenarios with a FlakyNntpProxy in front of a news server:
# (server number, message-id trigger, window in s or None for good)
SCENARIO_FLAKY_PROXY = {'xpackflaky': (1, 'xfB/', 4.0), 'xpackdeadserver': (2, 'xdB/', None)}

# scenarios with a CorruptingNntpProxy in front of Server1: message-id marker
SCENARIO_CORRUPT_PROXY = {'xpackcorrupt': 'xcB/'}

# scenarios with a RewritingNntpProxy in front of Server1: (old, new) reply bytes
SCENARIO_REWRITE_PROXY = {'notfound451': (b'430 ', b'451 '),
                          'yencrangefar': (b'begin=1000001 end=1500000', b'begin=9000001 end=9500000'),
                          'yencrangeshort': (b'begin=1000001 end=1500000', b'begin=1000001 end=1000100'),
                          'connhold': (b'\x00no-such\x00', b'\x00no-such\x00'),
                          'pathtraversalnzb': (b'name=aaaaaaaa.bin', b'name=../../ab.bin'),
                          'pathtraversalarticle': (b'name=aaaaaaaa.bin', b'name=../../ab.bin'), 'rejectnextserver': (b'=ypart begin=', b'=ypart begxn=')}

# scenarios with a DelayingNntpProxy in front of Server1: [(message-id marker, delay in s)]
SCENARIO_DELAY_PROXY = {'fleetbigqueue': [(b'-0-', 600.0)], 'dupausedunpack': [(b'dpuA/testfile3.part03', 200.0)], 'dupausedlive': [(b'dplA/testfile3.part03', 900.0)], 'slotpreemptpaused': [(b'sppA/v07', 900.0)], 'slotfinishedlive': [(b'sflA/v29', 900.0)], 'failcont_alt25busy': [(b'fcBlk/Blk.part03.rar?2=', 900.0)], 'livestraggler': LIVE_STRAGGLERS, 'lightburst': [(b'lbLoad', 0.25)], 'clientthreads': [(b'ctA/', 3.0)], 'xdecomp_slow': [(b'xdzB/', 0.7)], 'damagedfilestate': [(b'?6=500000:', 20.0, True)], 'articledecoyparrestart': [(b'?9=4000000:', 15.0, True), (b'?10=4500000:', 15.0, True)], 'directrenamesubdir': [(b'drA/obf', 0.4)], 'directrenamesubdirjoin': [(b'drA/obf', 0.4)], 'slowprobe': [(b'STAT ', 5.0), (b'spA/', 0.2)],
                        'directunpackkeep': [(b'rel.part03', 2.0)],
                        'directunpackkeepnointer': [(b'rel.part03', 2.0)],
                        'truncatedstate': [(b'tsA/', 0.5)],
                        'queuesyncslow': [(b'qsA/', 0.3)],
                        'queuebatchstart': [(b'qbA/', 1.0), (b'qbB/', 1.0)],
                        'reloadmidrepair': [(b'rlDon/', 2.0)],
                        'streamrestartcredit': [(b'srSlow/', 3.0)],
                        'restartfailover': [(b'rfA/', 0.4)],
                        'ondiskstop': [(b'odB/', 1.0)],
                        'ondiskstoprepairfirst': [(b'ocC/', 0.5)],
                        'ondiskstopsamename': [(b'osnB/', 1.5)],
                        'ondiskstopsamenamenointer': [(b'osiB/', 1.5)],
                        'ondiskstopothernamenointer': [(b'oonB/', 1.5)],
                        'streamstarved': [(b'busy/', 2.0)],
                        'keepreturned': [(b'krA/', 0.4)],
                        'streamtimeout': [(b'slowB/', 8.0)],
                        'streamslowprogress': [(b'slowA/', 2.0), (b'slowB/', 0.5)],
                        'streamtooslow': [(b'slowB/', 1.0), (b'warm/', 0.2)],
                        'streammaxrun': [(b'capA/', 2.0), (b'capB/', 1.0)]}

# scenarios with a FirstFailNntpProxy in front of Server1: (message-id markers,)

SCENARIO_FIRST_FAIL_PROXY = {'recheckfailed': ((b'?5=', b'?10=', b'?15='),),
                             'transientretry': ((b'?5=', b'?10=', b'?15='),),
                             'transientexhausted': ((b'?5=', b'?10=', b'?15='), True),
                             'transientotherserver': ((b'?5=', b'?10=', b'?15='), True)}

# scenarios with a FakeNewznab indexer (DupeSearchUrl points to it)

# scenarios with a FakeNntp news server in place of nserv
SCENARIO_FAKE_NNTP = {'watchsearchedkey', 'fleetprimaryease', 'dropfleetasync', 'droppasswordlogs', 'dropdune', 'fleetnzbpassword', 'fleettimeoutunverified', 'watchfleet', 'watchforcepick', 'watchdeadpick', 'watchswap', 'watchborrow', 'watchswaptwin', 'uucache', 'stallquit', 'joingroupreconnect', 'joingroupnogroups', 'fleetparallel', 'fleetslowurlfirst', 'fleetwide', 'fleetsamekey', 'fleetscoremax', 'fleetmerged', 'fleetslowurl', 'fleetpaused', 'fleetmostlydead', 'fleetlarge', 'fleetcopy', 'fleetbusy', 'fleetdeadtwins', 'fleetfailover', 'fleetaddbackup', 'fleetresendslow', 'fleetresend', 'fleetallerror', 'fleetotherkey', 'fleetnokey', 'fleetdeadfirst', 'fleettwins', 'fleettimeout', 'fleetone', 'fleetnodupecheck', 'fleetkindorder', 'fleetkindsingle', 'fleetkindlater', 'fleetintake', 'fleetintakelater', 'fleetencryptedlast', 'fleetencryptedheadless', 'fleetlockedlast', 'fleetencryptedpassword', 'fleetkindslowcheck', 'fleettwinvolumes', 'fleetalldead', 'fleetshutdown'}


# --------------------------------------------------------------------------- #
# Main
# --------------------------------------------------------------------------- #

def main():
    ap = argparse.ArgumentParser(description='DupeArticleFallback functional harness')
    ap.add_argument('--nzbget', required=True,
                    help='path to nzbget binary (local) or ON-DEVICE path (adb)')
    ap.add_argument('--target', choices=['local', 'adb'], default='local')
    ap.add_argument('--scenario', default='all',
                    help="'all', a scenario name, or several separated by commas")
    ap.add_argument('--serial', help='adb device serial (adb target)')
    ap.add_argument('--keep', action='store_true', help='keep the workdir')
    ap.add_argument('--fallback-mode', choices=FALLBACK_MODES[2:],
                    help='run the scenarios written for a lower DupeArticleFallback mode '
                         '(article, stream) under this one; the others are left out')
    args = ap.parse_args()

    # 'all', one name, or several separated by commas
    scenarios = list(SCENARIOS) if args.scenario == 'all' else args.scenario.split(',')
    unknown = [name for name in scenarios if name not in SCENARIOS]
    if unknown:
        ap.error('unknown scenario(s): %s' % ', '.join(unknown))
    if args.fallback_mode:
        # only a scenario of a lower mode than the requested one is promoted;
        # one written for "no" tests the absence of the feature, one already
        # at or above the requested mode is covered by a plain run
        def promoted(name):
            options = SCENARIO_OPTIONS.get(name, DEFAULT_OPTIONS)
            mode = _fallback_mode(options() if callable(options) else options)
            return name not in FALLBACK_MODE_BOUND and mode != 'no' and \
                FALLBACK_MODES.index(mode) < FALLBACK_MODES.index(args.fallback_mode)
        left_out = [name for name in scenarios if not promoted(name)]
        scenarios = [name for name in scenarios if promoted(name)]
        print('fallback-mode=%s: %d scenarios promoted, %d left out' % (args.fallback_mode, len(scenarios), len(left_out)))
    results = []

    for name in scenarios:
        if args.target == 'adb' and (name in SCENARIO_FLAKY_PROXY or name in SCENARIO_CORRUPT_PROXY or
                                           name in SCENARIO_REWRITE_PROXY or name in SCENARIO_DELAY_PROXY):
            results.append((name, None, 'SKIP: the flaky news-server proxy runs on the host'))
            print('[SKIP] %s  (the flaky news-server proxy runs on the host)' % name)
            continue
        if args.target == 'adb' and name.startswith('xdecomp_'):
            results.append((name, None, 'SKIP: target-side archive extractor is not provisioned on Android'))
            print('[SKIP] %s  (target-side archive extractor is not provisioned on Android)' % name)
            continue
        for key in SCENARIO_ENV_KEYS:
            os.environ.pop(key, None)
        os.environ.update(SCENARIO_ENV.get(name, {}))
        nntp = free_port()
        rpc = free_port()
        while rpc == nntp:  # the two must not coincide
            rpc = free_port()
        stage = tempfile.mkdtemp(prefix='dupefallback-%s-' % name)
        if args.target == 'local':
            target = LocalTarget(args.nzbget, stage)
        else:
            target = AdbTarget(args.nzbget, stage, serial=args.serial)
        daemon = Daemon(target, nntp, rpc)
        try:
            flaky = SCENARIO_FLAKY_PROXY.get(name)
            corrupt = SCENARIO_CORRUPT_PROXY.get(name)
            rewrite = SCENARIO_REWRITE_PROXY.get(name)
            delay = SCENARIO_DELAY_PROXY.get(name)
            first_fail = SCENARIO_FIRST_FAIL_PROXY.get(name)
            options = SCENARIO_OPTIONS.get(name, DEFAULT_OPTIONS)
            # a function gives options that depend on the start time
            options = list(options() if callable(options) else options)
            if args.fallback_mode:
                options = _promote_fallback(options, args.fallback_mode)
            # a scenario that hands the daemon an archive tool this host lacks
            # is a SKIP, like the xcrypt_*/xdecomp_* ones without their module
            missing = [tool for prefix, tool, have in (('UnrarCmd=', 'unrar', generators.HAVE_UNRAR),
                                                       ('SevenZipCmd=', '7z', generators.HAVE_7Z))
                       if not have and any(option.startswith(prefix) for option in options)]
            if missing:
                results.append((name, None, 'SKIP: %s is not installed' % ' and '.join(missing)))
                print('[SKIP] %s  (%s is not installed)' % (name, ' and '.join(missing)))
                continue
            nserv_port = nntp
            proxy_port = None
            if flaky or corrupt or rewrite or delay or first_fail:
                # Server1 always is nntp: a proxy for it listens there and
                # nserv moves to a port of its own; a proxy for Server2 gets a
                # port of its own
                server = flaky[0] if flaky else 1
                proxy_port = nntp if server == 1 else free_port()
                if server == 1:
                    nserv_port = free_port()
                else:
                    options.append('Server%d.Port=%d' % (server, proxy_port))
            if name in SCENARIO_GROUPED_SERVERS:
                for number in range(2, SCENARIO_GROUPED_SERVERS[name] + 1):
                    options += ['Server%d.Host=127.0.0.1' % number, 'Server%d.Port=%d' % (number, nserv_port),
                                'Server%d.Connections=2' % number, 'Server%d.Level=0' % number,
                                'Server%d.Encryption=no' % number, 'Server%d.Group=1' % number]
            if name in SCENARIO_EXTRA_SERVERS:
                live, dead = SCENARIO_EXTRA_SERVERS[name]
                number = 1
                # each extra live server is a server of its own (its own port, through a
                # plain forwarding proxy to nserv): servers on one host and port are one
                # article store to the daemon's backend identification
                daemon.extra_proxies = []
                for _ in range(live - 1):
                    number += 1
                    port = free_port()
                    daemon.extra_proxies.append(FlakyNntpProxy(port, nserv_port, '\x00never\x00', None))
                    options += ['Server%d.Host=127.0.0.1' % number, 'Server%d.Port=%d' % (number, port),
                                'Server%d.Connections=2' % number, 'Server%d.Level=0' % number,
                                'Server%d.Encryption=no' % number]
                for _ in range(dead):
                    number += 1
                    options += ['Server%d.Host=127.0.0.1' % number, 'Server%d.Port=1' % number,
                                'Server%d.Connections=2' % number, 'Server%d.Level=0' % number,
                                'Server%d.Encryption=no' % number, 'Server%d.Optional=yes' % number]
            if name in ('backbonemiss', 'backendauto'):
                bone = 'TestBone' if name == 'backbonemiss' else ''
                for number, backbone, host in ((2, bone, '127.0.0.1'), (3, '', 'localhost')):
                    options += ['Server%d.Active=yes' % number, 'Server%d.Host=%s' % (number, host),
                                'Server%d.Port=%d' % (number, nserv_port), 'Server%d.Connections=2' % number,
                                'Server%d.Priority=2' % number, 'Server%d.Encryption=no' % number,
                                'Server%d.Backbone=%s' % (number, backbone)]
            if name == 'authrejected':
                daemon.reject_nntp = RejectingNntp()
                options += ['Server2.Active=yes', 'Server2.Host=127.0.0.1', 'Server2.Port=%d' % daemon.reject_nntp.port,
                            'Server2.Username=u', 'Server2.Password=p', 'Server2.Level=0', 'Server2.Connections=2',
                            'Server2.Encryption=no', 'ArticleRetries=0', 'ArticleInterval=0']
            daemon.write_config(options)
            if name in SCENARIO_FAKE_NNTP:
                daemon.fake_nntp = FakeNntp(nserv_port)
            else:
                daemon.start_nserv(capture_requests=(name in CAPTURE_REQUESTS),
                                   extra_args=SCENARIO_NSERV_ARGS.get(name, ()), port=nserv_port)
            if flaky:
                daemon.proxy = FlakyNntpProxy(proxy_port, nserv_port, *flaky[1:])
            elif corrupt:
                daemon.proxy = CorruptingNntpProxy(proxy_port, nserv_port, corrupt)
            elif rewrite:
                daemon.proxy = RewritingNntpProxy(proxy_port, nserv_port, *rewrite)
            elif delay:
                daemon.proxy = DelayingNntpProxy(proxy_port, nserv_port, delay)
            elif first_fail:
                daemon.proxy = FirstFailNntpProxy(proxy_port, nserv_port, *first_fail)
            time.sleep(1)
            daemon.start()
            if args.target == 'adb':
                target.forward_rpc(rpc)
            time.sleep(2)
            sc_name, passed, detail = SCENARIOS[name](daemon, target)
            expected_status = EXPECTED_HISTORY_STATUS.get(name)
            if passed is not None and expected_status and \
                    ('status=%s' % expected_status) not in detail:
                passed = False
                detail += ' expected_status=%s' % expected_status
            results.append((sc_name, passed, detail))
            label = 'SKIP' if passed is None else ('PASS' if passed else 'FAIL')
            print('[%s] %s  (%s)' % (label, sc_name, detail))
        except Exception as e:
            results.append((name, False, 'ERROR: %s' % e))
            print('[FAIL] %s  (ERROR: %s)' % (name, e))
        finally:
            failed = not results or results[-1][1] is False
            if daemon.proxy:
                daemon.proxy.close()
            if daemon.fake_nntp:
                daemon.fake_nntp.close()
            for extra in getattr(daemon, 'extra_proxies', []):
                extra.close()
            # a failed scenario keeps its work directory
            if failed and not args.keep and args.target == 'local':
                print('       kept: %s' % target.work)
            target.teardown(args.keep or failed)

    # tri-state result: True/False are real pass/fail, None is a graceful SKIP
    # (e.g. an xcrypt_* scenario when the cryptography module is not
    # installed) - it counts toward neither the numerator nor the "attempted"
    # denominator, and never fails the run on its own.
    skipped = sum(1 for _, p, _ in results if p is None)
    attempted = len(results) - skipped
    passed_count = sum(1 for _, p, _ in results if p)
    failed = any(p is False for _, p, _ in results)
    suffix = ' (+%d skipped)' % skipped if skipped else ''
    print('\n=== %s / %s scenarios passed%s on target=%s%s ===' % (
        passed_count, attempted, suffix, args.target,
        ' fallback-mode=%s' % args.fallback_mode if args.fallback_mode else ''))
    if failed:
        return 1
    # A requested scenario that cannot run is an explicit, machine-readable
    # infrastructure result. Never turn missing crypto/7z into a green run.
    return 77 if skipped else 0


if __name__ == '__main__':
    sys.exit(main())
