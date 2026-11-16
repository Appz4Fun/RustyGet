#!/usr/bin/env python3
"""Offline streaming PAR2 checks through the real queue, cache, repair and unpack.

All subprocesses and providers belong to a unique scratch fixture. Successful
and failed artifacts are retained. These are correctness checks, not the 100GB
resource/performance campaign. No production configuration or providers are used.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import struct
import subprocess
import tempfile
import threading
import time
import zlib

import generators
import harness
from par_first_test import LoggedDaemon, inspect_parity, verify_named_outputs

CASES = ('ram_part40', 'manual', 'repair_off', 'live_off')
LATE_CASES = ('late_par_hole_patch', 'late_par_hole_manual',
              'late_par_hole_repair_off', 'late_par_hole_corrupt',
              'late_par_hole_healthy', 'late_par_hole_zero_pool')


class RecoveryBarrierProxy(harness.RewritingNntpProxy):
    """Hold recovery requests until the fixture observes extraction past damage."""

    def __init__(self, listen_port, upstream_port):
        self.release = threading.Event()
        self.requested = threading.Event()
        self.serve_done = threading.Event()
        self.lock = threading.Lock()
        self.workers = []
        self.sockets = []
        self.events = []
        self.timed_out = False
        self.held_once = False
        super().__init__(listen_port, upstream_port, b'', b'')

    def _serve(self):
        try:
            while True:
                try:
                    client, _ = self.srv.accept()
                    upstream = harness.socket.create_connection(('127.0.0.1', self.upstream_port))
                except OSError:
                    return
                with self.lock:
                    self.sockets.extend((client, upstream))
                    for args in ((client, upstream, False), (upstream, client, True)):
                        worker = threading.Thread(target=self._pipe, args=args, daemon=True)
                        self.workers.append(worker)
                        worker.start()
        finally:
            self.serve_done.set()

    def _pipe(self, src, dst, rewrite):
        pending = b''
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                if rewrite:
                    dst.sendall(data)
                    continue
                pending += data
                while b'\r\n' in pending:
                    line, pending = pending.split(b'\r\n', 1)
                    lower = line.lower()
                    if lower.startswith((b'body ', b'article ')) and b'.vol' in lower:
                        with self.lock:
                            self.events.append({'event': 'recovery_requested',
                                                'monotonic_ns': time.monotonic_ns()})
                            hold = not self.held_once
                            self.held_once = True
                        if hold:
                            self.requested.set()
                        if hold and not self.release.wait(120):
                            self.timed_out = True
                            return
                    dst.sendall(line + b'\r\n')
        except OSError:
            pass
        finally:
            for sock in (src, dst):
                try:
                    sock.shutdown(harness.socket.SHUT_RDWR)
                except OSError:
                    pass
                sock.close()

    def close(self):
        self.release.set()
        try:
            self.srv.shutdown(harness.socket.SHUT_RDWR)
        except OSError:
            pass
        super().close()
        self.serve_done.wait(10)
        with self.lock:
            sockets, workers = list(self.sockets), list(self.workers)
        for sock in sockets:
            try:
                sock.shutdown(harness.socket.SHUT_RDWR)
            except OSError:
                pass
        for worker in workers:
            worker.join(timeout=10)
        if not self.serve_done.is_set() or any(worker.is_alive() for worker in workers):
            raise RuntimeError('owned proxy threads did not terminate')


def digest_regular(path, limit=256 * 1024 * 1024):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        before = os.fstat(fd)
        assert stat.S_ISREG(before.st_mode) and before.st_size <= limit
        digest = hashlib.sha256()
        remaining = before.st_size
        while remaining:
            data = os.read(fd, min(1024 * 1024, remaining))
            assert data, 'short hash input'
            digest.update(data)
            remaining -= len(data)
        assert not os.read(fd, 1), 'hash input grew'
        after = os.fstat(fd)
        assert (before.st_size, before.st_mtime_ns) == (after.st_size, after.st_mtime_ns)
        return digest.hexdigest()
    finally:
        os.close(fd)


def metrics(target):
    try:
        log = target.read_file('nzbget.log').decode(errors='replace')
    except FileNotFoundError:
        return []
    return live_metric_reports(log, strict=False)



LIVE_METRIC_KEYS = (
    'captured_articles captured_bytes capture_refusals cache_misses '
    'committed_replay_bytes ram_bytes_verified source_bytes_read backfill_bytes_read '
    'recovery_bytes_read target_verification_bytes_read tracked_exponents ready_recovery '
    'pending_folds verified_slices total_slices writer_wait_ns operation_wait_ns '
    'repair_attempts repair_completed_calls target_bytes_written payload_wait_ns '
    'slot_backfill_ns matrix_ns decode_ns target_write_ns target_sync_ns refold_ns '
    'target_verification_ns repair_call_ns'
).split()


def live_metric_reports(log, strict=True):
    """Decode complete per-generation packets; strict mode requires a disposal footer.

    Non-strict mode is only for startup polling of a still-running daemon. It may
    return earlier complete records while a later packet is pending, never final
    qualification or substituted zero counters. Parse final logs after exit zero.
    """
    def require(condition, message):
        if not condition:
            raise ValueError('live metric report: ' + message)

    def uint(value, maximum=(1 << 64) - 1):
        require(type(value) is int and 0 <= value <= maximum, 'unsigned integer')
        return value

    def nzb_id(value):
        require(type(value) is int and -(1 << 31) <= value < (1 << 31), 'NZB identity')
        return value

    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, 'duplicate JSON member')
            result[key] = value
        return result

    def decode_json(text):
        data = json.loads(text, object_pairs_hook=unique_object)
        require(isinstance(data, dict), 'record object')
        return data

    groups = {}
    for line in log.splitlines(keepends=True):
        marker = 'Live PAR frame '
        if marker not in line:
            continue
        if not line.endswith('\n'):
            require(not strict, 'unterminated frame')
            continue
        frame = decode_json(line.split(marker, 1)[1])
        require(type(frame.get('version')) is int and frame['version'] == 1, 'version')
        identity = (nzb_id(frame.get('nzb_id')), uint(frame.get('generation')), uint(frame.get('report_id')))
        require(identity[1] > 0 and identity[2] > 0, 'positive generation/report identity')
        parts = uint(frame.get('parts'), 64)
        index = uint(frame.get('index'))
        require(parts > 0 and index < parts, 'part geometry')
        crc = uint(frame.get('crc32'), (1 << 32) - 1)
        hex_data = frame.get('hex')
        require(type(hex_data) is str and 0 < len(hex_data) <= 512 and len(hex_data) % 2 == 0
                and re.fullmatch('[0-9a-f]+', hex_data) is not None, 'hex payload')
        data = bytes.fromhex(hex_data)
        require(index == parts - 1 or len(data) == 256, 'short nonfinal part')
        group = groups.setdefault(identity, dict(parts=parts, crc=crc, rows={}))
        require(group['parts'] == parts and group['crc'] == crc and index not in group['rows'],
                'mixed or duplicate parts')
        group['rows'][index] = data
    records, cohorts = [], {}
    for identity, group in sorted(groups.items()):
        if len(group['rows']) != group['parts']:
            require(not strict, 'incomplete packet')
            continue
        raw = b''.join(group['rows'][index] for index in range(group['parts']))
        require(zlib.crc32(raw) == group['crc'], 'CRC mismatch')
        prefix = b'Live PAR metrics '
        require(raw.startswith(prefix), 'message kind')
        sample = decode_json(raw[len(prefix):].decode('utf-8'))
        require(nzb_id(sample.get('nzb_id')) == identity[0]
                and uint(sample.get('generation')) == identity[1], 'inner identity')
        require(sample.get('event') in ('started', 'repair_complete', 'disposed'), 'event')
        for key in LIVE_METRIC_KEYS:
            uint(sample.get(key))
        require(type(sample.get('count_ready_observed')) is bool, 'readiness flag')
        handoff = sample.get('count_ready_to_handoff_ns')
        if handoff is not None:
            uint(handoff)
            require(sample['count_ready_observed'], 'handoff without readiness')
        require('count_ready_to_handoff_ns' in sample, 'missing handoff field')
        for key, expected in [('decode_includes_writes', True),
                              ('refold_includes_queued_completion', False), ('logical_io', True)]:
            require(sample.get(key) is expected, key)
        records.append(sample)
        cohorts.setdefault(identity[:2], []).append((identity[2], sample['event']))
    if strict:
        for reports in cohorts.values():
            ids, events = zip(*reports)
            require(all(report_id == index for index, report_id in enumerate(ids, 1)),
                    'missing report sequence')
            require(events.count('started') == 1 and events.count('disposed') == 1
                    and events[-1] == 'disposed', 'incomplete cohort lifetime')
    return records


def callback_report(log, expected_min):
    """Validate the complete framed, quiescent callback report or fail closed."""
    def require(condition, message):
        if not condition:
            raise ValueError('callback report: ' + message)

    def uint(value, limit=(1 << 64) - 1):
        require(type(value) is int and 0 <= value <= limit, 'invalid unsigned integer')
        return value

    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, 'duplicate JSON member')
            result[key] = value
        return result

    def records(marker):
        result = []
        for line in log.splitlines():
            if marker in line:
                data = json.loads(line.split(marker, 1)[1], object_pairs_hook=unique_object)
                require(isinstance(data, dict), 'record is not an object')
                result.append(data)
        return result

    summaries = records('ArticleCompleted metrics ')
    bins = records('ArticleCompleted bin ')
    require(len(summaries) == 1, 'missing or duplicate summary')
    summary = summaries[0]
    require(type(summary.get('version')) is int and summary['version'] == 1, 'version')
    for key, value in [('boundary', 'queuecoord.article_completed'),
                       ('scope', 'coordinator_lifetime'),
                       ('histogram_method', 'loglinear128_ns_v1'),
                       ('transport', 'log_records_v1')]:
        require(summary.get(key) == value, key)
    require(summary.get('valid') is True and summary.get('quiescent') is True,
            'invalid or nonquiescent report')
    report_id = uint(summary.get('report_id'))
    require(report_id > 0, 'report identity')
    callbacks = uint(summary.get('callbacks'))
    require(callbacks >= uint(expected_min) and callbacks > 0, 'insufficient callback coverage')
    rows = uint(summary.get('histogram_rows'), 7424)
    require(rows > 0 and len(bins) == rows, 'incomplete histogram')
    checksum = uint(summary.get('histogram_crc32'), (1 << 32) - 1)
    indexed = {}
    for row in bins:
        require(type(row.get('version')) is int and row['version'] == 1, 'bin version')
        require(uint(row.get('report_id')) == report_id, 'mixed report identity')
        require(uint(row.get('rows')) == rows, 'mixed row count')
        index = uint(row.get('index'))
        require(index < rows and index not in indexed, 'invalid or duplicate row index')
        lower, upper, count = (uint(row.get(key)) for key in ('lower_ns', 'upper_ns', 'count'))
        require(lower <= upper and count > 0, 'invalid bin')
        shift = max(0, lower.bit_length() - 1 - 7)
        width = 1 << shift
        require(lower % width == 0 and upper == lower + width - 1, 'bin geometry')
        indexed[index] = (lower, upper, count)
    crc = zlib.crc32(struct.pack('<QQQ', report_id, callbacks, rows))
    total, previous, bounds, histogram = 0, -1, None, []
    rank = (callbacks * 99 + 99) // 100
    for index in range(rows):
        lower, upper, count = indexed[index]
        require(lower > previous, 'overlapping or unordered bins')
        previous = upper
        total += count
        histogram.append([lower, upper, count])
        crc = zlib.crc32(struct.pack('<QQQQ', index, lower, upper, count), crc)
        if total >= rank and bounds is None:
            bounds = (lower, upper)
    require(crc == checksum, 'histogram CRC mismatch')
    require(total == callbacks and bounds is not None, 'inconsistent callback total')
    reported_bounds = (uint(summary.get('p99_lower_ns')), uint(summary.get('p99_upper_ns')))
    require(bounds == reported_bounds, 'inconsistent p99 bounds')
    return dict(summary, histogram=histogram)


def wait_until(predicate, description, timeout=30):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError('timeout: ' + description)


def inspect_source_geometry(paths, golden, slice_size):
    main = None
    descriptions = {}
    for path in paths:
        data = path.read_bytes()
        offset = 0
        while offset < len(data):
            assert data[offset:offset + 8] == b'PAR2\0PKT'
            length = struct.unpack_from('<Q', data, offset + 8)[0]
            assert length >= 64 and length % 4 == 0 and offset + length <= len(data)
            packet = data[offset:offset + length]
            assert hashlib.md5(packet[32:]).digest() == packet[16:32]
            kind = packet[48:64]
            if kind == b'PAR 2.0\0Main\0\0\0\0':
                assert length >= 76 and (length - 76) % 16 == 0
                block_size, count = struct.unpack_from('<QI', packet, 64)
                ids = tuple(packet[i:i + 16] for i in range(76, length, 16))
                value = (block_size, count, ids)
                assert main is None or main == value
                main = value
            elif kind == b'PAR 2.0\0FileDesc':
                assert length >= 120
                file_id = packet[64:80]
                value = (struct.unpack_from('<Q', packet, 112)[0], packet[120:].rstrip(b'\0'))
                assert file_id not in descriptions or descriptions[file_id] == value
                descriptions[file_id] = value
            offset += length
    assert main is not None
    block_size, count, ids = main
    assert block_size == slice_size and count == len(golden) == len(ids) == len(set(ids))
    observed = {}
    for file_id in ids:
        length, name = descriptions[file_id]
        name = name.decode('ascii')
        assert name in golden and length == len(golden[name]) and name not in observed
        observed[name] = length
    slices = sum((length + block_size - 1) // block_size for length in observed.values())
    assert slices == 100 and all(length <= block_size for length in observed.values())
    return {'source_files': count, 'source_slices': slices, 'source_lengths': observed}


def make_fixture(target, par2, case):
    root = Path(target.makedirs('data', 'streaming-primary'))
    assert shutil.disk_usage(root).free >= 256 * 1024 * 1024
    late = case in LATE_CASES
    volume_size = 262144 if late else 65536
    slice_size = 524288 if late else 131072
    missing_article = 4 if late else 2
    payload = harness._payload(100 * volume_size, 20261114)
    volumes = generators.rar3_store_volumes_valid('payload.bin', payload, volume_size)
    assert len(volumes) == 100
    golden = {'rel.part%03d.rar' % (i + 1): data for i, data in enumerate(volumes)}
    for name, data in golden.items():
        (root / name).write_bytes(data)
    create = [par2, 'create', '-q', '-q', '-s' + str(slice_size), '-c1', '-n1',
              'rel.par2', *golden]
    with open(target.path('par2-create.log'), 'xb') as output:
        subprocess.run(create, cwd=root, stdout=output, stderr=subprocess.STDOUT,
                       timeout=60, check=True)
    parity = sorted(root.glob('*.par2'))
    assert len(parity) == 2, [p.name for p in parity]
    assert inspect_parity(parity) == (slice_size, 1)
    geometry = inspect_source_geometry(parity, golden, slice_size)
    with open(target.path('par2-verify.log'), 'xb') as output:
        subprocess.run([par2, 'verify', '-q', '-q', 'rel.par2'], cwd=root,
                       stdout=output, stderr=subprocess.STDOUT, timeout=60, check=True)
    original_parity_sha256 = {p.name: digest_regular(p) for p in parity}
    if late:
        damaged_size = len(golden['rel.part040.rar'])
        assert 98304 >= 65536 and 131072 <= damaged_size - 4096
    if case == 'late_par_hole_corrupt':
        recovery = next(p for p in parity if '.vol' in p.name.lower())
        data = bytearray(recovery.read_bytes())
        offset = 0
        changed = 0
        while offset < len(data):
            assert data[offset:offset + 8] == b'PAR2\0PKT'
            length = struct.unpack_from('<Q', data, offset + 8)[0]
            assert length >= 64 and offset + length <= len(data)
            if data[offset + 48:offset + 64] == b'PAR 2.0\0RecvSlic':
                assert length > 68
                data[offset + 68] ^= 1
                changed += 1
            offset += length
        assert changed == 1
        recovery.write_bytes(data)
    members = []
    for name, content in golden.items():
        members.append(('streaming-primary/' + name, name, len(content), 32768,
                        {missing_article} if name == 'rel.part040.rar'
                        and case not in ('late_par_hole_healthy', 'late_par_hole_zero_pool') else set()))
    for path in parity:
        members.append(('streaming-primary/' + path.name, path.name,
                        path.stat().st_size, 65536, set()))
    fixture = {'payload_sha256': hashlib.sha256(payload).hexdigest(),
               'source_sha256': {name: hashlib.sha256(data).hexdigest()
                                 for name, data in golden.items()},
               'parity_sha256': {p.name: digest_regular(p) for p in parity},
               'original_parity_sha256': original_parity_sha256,
               'par2_binary_sha256': digest_regular(par2), 'par2_create': create,
               'source_geometry': geometry, 'slice_size': slice_size, 'recovery_slices': 1,
               'damaged_volume': 'rel.part040.rar', 'missing_article': missing_article,
               'late_hole_range': [98304, 131072] if late else None,
               'corrupt_recovery': case == 'late_par_hole_corrupt'}
    Path(target.path('fixture.json')).write_text(json.dumps(fixture, indent=2) + '\n')
    return payload, golden, members


def run_case(binary, par2, case):
    work = tempfile.mkdtemp(prefix='rustyget-streaming-par-' + case + '-')
    target = harness.LocalTarget(binary, work)
    result = {'case': case, 'work': work, 'passed': False,
              'binary_sha256': digest_regular(binary)}
    daemon = None
    proxy = None
    try:
        payload, golden, members = make_fixture(target, par2, case)
        ports = set()
        while len(ports) < 3:
            ports.add(harness.free_port())
        nserv_port, proxy_port, rpc_port = sorted(ports)
        daemon = LoggedDaemon(target, nserv_port, rpc_port)
        daemon.start_nserv()
        # Make the last dependency arrive after extraction reaches part40.
        barrier_case = case in ('late_par_hole_patch', 'late_par_hole_corrupt')
        proxy = (RecoveryBarrierProxy(proxy_port, nserv_port) if barrier_case else
                 harness.DelayingNntpProxy(proxy_port, nserv_port,
                                           [(b'rel.part100.rar', 1.0)]))
        daemon.nntp_port = proxy_port
        manual = case in ('manual', 'late_par_hole_manual')
        repair_off = case in ('repair_off', 'late_par_hole_repair_off')
        options = ['ParCheck=' + ('manual' if manual else 'auto'),
                   'ParRepair=' + ('no' if repair_off else 'yes'),
                   'ParQuick=yes', 'ParCleanupQueue=no', 'ArticleCache=16',
                   'Server1.Connections=3', 'DirectWrite=yes', 'DirectRename=no',
                   'InterDir=', 'Unpack=yes', 'DirectUnpack=yes',
                   'UnpackCleanupDisk=no', 'DupeArticleFallback=no']
        # The live session runs only with LiveParRepair=yes (off by default);
        # 'live_off' checks the default: repair right after the download.
        options.append('LiveParRepair=' + ('no' if case == 'live_off' else 'yes'))
        if case == 'late_par_hole_zero_pool':
            options += ['ParPreset=custom', 'ParBuffer=0']
        daemon.write_config(options)
        daemon.start()
        api = daemon.wait_ready()
        name = 'StreamingPar-' + case
        nzb_id = daemon.append(api, name, harness.build_multi_nzb(members),
                               True, 'streaming-par-' + case, 100)
        assert nzb_id > 0
        files = wait_until(lambda: api.listfiles(0, 0, nzb_id), 'queued fixture files')
        assert len(files) == 102, len(files)
        index_id = next(f['ID'] for f in files if f['Filename'] == 'rel.par2')
        sources = [f['ID'] for f in files if f['Filename'].endswith('.rar')]
        assert len(sources) == 100
        assert api.editqueue('FileResume', '', [index_id])
        if case in ('ram_part40', 'late_par_hole_patch', 'late_par_hole_corrupt', 'late_par_hole_healthy'):
            wait_until(lambda: any(m['event'] == 'started' and m['nzb_id'] == nzb_id
                                   for m in metrics(target)), 'bounded PAR startup before source bytes')
        else:
            wait_until(lambda: all(f['ID'] != index_id for f in api.listfiles(0, 0, nzb_id)),
                       'explicitly resumed metadata index completion')
        assert api.editqueue('FileResume', '', sources)
        if barrier_case:
            wait_until(proxy.requested.is_set, 'automatic recovery request', timeout=90)
            def saw_partial():
                try:
                    log = target.read_file('nzbget.log').decode(errors='replace')
                except FileNotFoundError:
                    return False
                return 'extracted past damage' in log
            wait_until(saw_partial, 'progressive extraction past damage while recovery is held', timeout=120)
            assert not proxy.release.is_set() and not proxy.timed_out
            result['partial_observed_ns'] = time.monotonic_ns()
            assert not any(os.path.basename(p) == 'payload.bin'
                           for p in target.find_files('main', 'dst')), 'partial output published early'
            result['barrier_release_ns'] = time.monotonic_ns()
            proxy.release.set()
        history = daemon.wait_history(api, name, timeout=120)
        assert api.shutdown(), 'graceful daemon shutdown was refused'
        result['graceful_daemon_exit'] = daemon.process.wait(timeout=30)
        assert result['graceful_daemon_exit'] == 0, result
        log = target.read_file('nzbget.log').decode(errors='replace')
        events = live_metric_reports(log)
        time.sleep(0.25)  # nserv's final request-log flush
        requests = re.findall(r'Received: (?:BODY|ARTICLE) [^\r\n]*',
                              target.read_file('nserv.log').decode(errors='replace'))
        assert requests, 'empty provider capture cannot prove fetch policy'
        recovery_requests = [line for line in requests if '.vol' in line.lower()]
        result.update(status=history.get('Status'), par_status=history.get('ParStatus'),
                      metrics=events, recovery_requests=recovery_requests)
        if case == 'late_par_hole_patch':
            result['barrier_events'] = proxy.events
            assert proxy.events and proxy.events[0]['monotonic_ns'] < result['partial_observed_ns'] < result['barrier_release_ns']
            assert not proxy.timed_out
            assert any(event['event'] == 'repair_complete' for event in events), events
            assert history.get('Status', '').startswith('SUCCESS'), history
            integrity = verify_named_outputs(target, dict(golden, **{'payload.bin': payload}))
            result['integrity'] = integrity
            assert all(integrity.values()), integrity
            assert recovery_requests, 'no recovery slice was fetched'
            partial_at = log.find('extracted past damage')
            repaired_at = log.find('1 paused volume(s) released')
            patched_at = log.find('Unrar: repaired payload.bin from the repaired volumes')
            # The helper logs release after unlocking; unpack can patch and log
            # concurrently, so these two lines have no guaranteed mutual order.
            assert 0 <= partial_at < repaired_at and partial_at < patched_at, \
                'live repair and verified progressive patch did not follow the partial extraction'
        elif case in ('late_par_hole_healthy', 'late_par_hole_zero_pool'):
            assert history.get('Status', '').startswith('SUCCESS'), history
            integrity = verify_named_outputs(target, dict(golden, **{'payload.bin': payload}))
            result['integrity'] = integrity
            assert all(integrity.values()), integrity
            assert not recovery_requests, 'undamaged download fetched recovery'
            if case == 'late_par_hole_zero_pool':
                assert not events, 'zero PAR pool created a live session'
        elif case == 'late_par_hole_corrupt':
            result['barrier_events'] = proxy.events
            assert not proxy.timed_out and recovery_requests
            assert not any(event['event'] == 'repair_complete' for event in events), events
            assert '1 paused volume(s) released' not in log
            assert not history.get('Status', '').startswith('SUCCESS'), history
            assert not any(os.path.basename(p) == 'payload.bin'
                           for p in target.find_files('main', 'dst')), 'corrupt parity produced a complete output'
        elif case == 'live_off':
            # No live session: no Live PAR frames, no live hold release; the
            # recovery volume is still fetched and the ordinary repair heals
            # part040 right after the download.
            assert not events, 'LiveParRepair=no created a live session'
            assert 'paused volume(s) released' not in log, 'live repair ran with LiveParRepair=no'
            assert history.get('Status', '').startswith('SUCCESS'), history
            integrity = verify_named_outputs(target, dict(golden, **{'payload.bin': payload}))
            result['integrity'] = integrity
            assert all(integrity.values()), integrity
            assert recovery_requests, 'recovery slice never fetched'
            assert 'Repairing ' + name in log, 'ordinary repair did not run'
        elif case == 'ram_part40':
            complete = [event for event in events if event['event'] == 'repair_complete']
            assert complete, 'no guarded live repair publication'
            event = complete[-1]
            assert history.get('Status', '').startswith('SUCCESS'), history
            integrity = verify_named_outputs(target, dict(golden, **{'payload.bin': payload}))
            result['integrity'] = integrity
            assert all(integrity.values()), integrity
            assert event['captured_articles'] > 0 and event['captured_bytes'] > 0
            assert event['committed_replay_bytes'] == 0, event
            assert event['source_bytes_read'] == 0 and event['backfill_bytes_read'] == 0, event
            assert event['target_verification_bytes_read'] == len(golden['rel.part040.rar']), event
            assert event['target_bytes_written'] == len(golden['rel.part040.rar']), event
            assert event['verified_slices'] == 100, event
            assert event['count_ready_to_handoff_ns'] is not None, event
            assert event['matrix_ns'] > 0 and event['decode_ns'] > 0, event
            assert event['target_write_ns'] > 0 and event['target_verification_ns'] > 0, event
            assert '1 paused volume(s) released' in log, 'unpack hold was not released'
            # The ordinary checker emits "Verifying file" even for its zero-read
            # CRC path. Require its actual source verdicts, not that message.
            checks = [json.loads(line.split('PAR verification metrics ', 1)[1])
                      for line in log.splitlines() if 'PAR verification metrics {' in line]
            result['normal_par_checks'] = checks
            assert checks and all(check == {'source_files': 100,
                                           'quick_verified_files': 100,
                                           'repair_needed': False} for check in checks), checks
            assert 'Repairing ' + name not in log, 'normal repair repeated the live repair'
            assert recovery_requests, 'recovery slice never fetched'
        else:
            assert not events, 'manual/repair-off created a live session'
            if manual:
                # Preserve incumbent manual postprocessing: it downloads the
                # remaining PAR files for the user's later manual check.
                manual = log.find('Downloading all remaining files for manual par-check for ' + name)
                first_recovery = log.find('Downloading ' + name + '/rel.vol')
                assert recovery_requests and 0 <= manual < first_recovery, \
                    'manual recovery was fetched before the ordinary manual stage'
                assert history.get('ParStatus') == 'MANUAL', history
            else:
                # ParRepair=no still permits the ordinary checker to establish
                # repairability; it must not fetch through a live session.
                checking = log.find('Checking pars for ' + name)
                first_recovery = log.find('Downloading ' + name + '/rel.vol')
                assert recovery_requests and 0 <= checking < first_recovery, \
                    'repair-off recovery was fetched before the ordinary checker'
                assert history.get('Status') == 'WARNING/REPAIRABLE', history
                assert history.get('ParStatus') == 'REPAIR_POSSIBLE', history
                assert 'Repair possible for ' + name + '/rel' in log, \
                    'ordinary checker did not establish repairability'
            assert 'paused volume(s) released' not in log, 'disabled repair released a live hold'
            assert not history.get('Status', '').startswith('SUCCESS'), history
            damaged = [path for path in target.find_files('main', 'dst')
                       if os.path.basename(path) == 'rel.part040.rar']
            assert len(damaged) == 1, 'disabled repair lost or duplicated the damaged source'
            assert target.read_file(damaged[0]) != golden['rel.part040.rar'], \
                'disabled repair healed the damaged source'
        if os.environ.get('RUSTYGET_BENCH_CALLBACK_METRICS') == '1':
            expected_min = sum((size + segment - 1) // segment
                               for _, _, size, segment, _ in members)
            sample = callback_report(log, expected_min)
            result['callback_metrics'] = sample
            result['callbacks_expected_min'] = expected_min
        result['passed'] = True
    except Exception as error:
        result['error'] = repr(error)
    finally:
        if proxy:
            try:
                proxy.close()
            except Exception as error:
                result['passed'] = False
                result['proxy_cleanup_error'] = repr(error)
        target.teardown(True)
        result['native_children'] = []
        for process in target.procs:
            try:
                code = process.wait(timeout=10)
                result['native_children'].append({'pid': process.pid, 'exit_code': code})
            except subprocess.TimeoutExpired:
                result['passed'] = False
                result['cleanup_error'] = 'owned native child did not terminate'
        Path(target.path('result.json')).write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, sort_keys=True), flush=True)
        print('Artifacts: ' + work, flush=True)
    return result['passed']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nzbget', required=True)
    parser.add_argument('--par2', default=shutil.which('par2'))
    parser.add_argument('--case', choices=CASES + LATE_CASES + ('all',), default='ram_part40')
    args = parser.parse_args()
    assert args.par2, 'independent par2 reference tool is required'
    binary, par2 = str(Path(args.nzbget).resolve()), str(Path(args.par2).resolve())
    cases = CASES if args.case == 'all' else (args.case,)
    outcomes = [run_case(binary, par2, case) for case in cases]
    return 0 if all(outcomes) else 1


if __name__ == '__main__':
    raise SystemExit(main())
