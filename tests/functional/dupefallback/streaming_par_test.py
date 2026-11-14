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
import subprocess
import tempfile
import time

import generators
import harness
from par_first_test import LoggedDaemon, inspect_parity, verify_named_outputs

CASES = ('ram_part40', 'manual', 'repair_off')


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
    return [json.loads(line.split('Live PAR metrics ', 1)[1])
            for line in log.splitlines() if 'Live PAR metrics {' in line]


def wait_until(predicate, description, timeout=30):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError('timeout: ' + description)


def make_fixture(target, par2):
    root = Path(target.makedirs('data', 'streaming-primary'))
    assert shutil.disk_usage(root).free >= 256 * 1024 * 1024
    payload = harness._payload(100 * 65536, 20261114)
    volumes = generators.rar3_store_volumes_valid('payload.bin', payload, 65536)
    assert len(volumes) == 100
    golden = {'rel.part%03d.rar' % (i + 1): data for i, data in enumerate(volumes)}
    for name, data in golden.items():
        (root / name).write_bytes(data)
    create = [par2, 'create', '-q', '-q', '-s131072', '-c1', '-n1',
              'rel.par2', *golden]
    with open(target.path('par2-create.log'), 'xb') as output:
        subprocess.run(create, cwd=root, stdout=output, stderr=subprocess.STDOUT,
                       timeout=60, check=True)
    parity = sorted(root.glob('*.par2'))
    assert len(parity) == 2, [p.name for p in parity]
    assert inspect_parity(parity) == (131072, 1)
    with open(target.path('par2-verify.log'), 'xb') as output:
        subprocess.run([par2, 'verify', '-q', '-q', 'rel.par2'], cwd=root,
                       stdout=output, stderr=subprocess.STDOUT, timeout=60, check=True)
    members = []
    for name, content in golden.items():
        members.append(('streaming-primary/' + name, name, len(content), 32768,
                        {2} if name == 'rel.part040.rar' else set()))
    for path in parity:
        members.append(('streaming-primary/' + path.name, path.name,
                        path.stat().st_size, 65536, set()))
    fixture = {'payload_sha256': hashlib.sha256(payload).hexdigest(),
               'source_sha256': {name: hashlib.sha256(data).hexdigest()
                                 for name, data in golden.items()},
               'parity_sha256': {p.name: digest_regular(p) for p in parity},
               'par2_binary_sha256': digest_regular(par2), 'par2_create': create,
               'source_slices': 100, 'slice_size': 131072, 'recovery_slices': 1,
               'damaged_volume': 'rel.part040.rar', 'missing_article': 2}
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
        payload, golden, members = make_fixture(target, par2)
        ports = set()
        while len(ports) < 3:
            ports.add(harness.free_port())
        nserv_port, proxy_port, rpc_port = sorted(ports)
        daemon = LoggedDaemon(target, nserv_port, rpc_port)
        daemon.start_nserv()
        # Make the last dependency arrive after extraction reaches part40.
        proxy = harness.DelayingNntpProxy(proxy_port, nserv_port,
                                         [(b'rel.part100.rar', 1.0)])
        daemon.nntp_port = proxy_port
        options = ['ParCheck=' + ('manual' if case == 'manual' else 'auto'),
                   'ParRepair=' + ('no' if case == 'repair_off' else 'yes'),
                   'ParQuick=yes', 'ParCleanupQueue=no', 'ArticleCache=16',
                   'Server1.Connections=3', 'DirectWrite=yes', 'DirectRename=no',
                   'InterDir=', 'Unpack=yes', 'DirectUnpack=yes',
                   'UnpackCleanupDisk=no', 'DupeArticleFallback=no']
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
        if case == 'ram_part40':
            wait_until(lambda: any(m['event'] == 'started' and m['nzb_id'] == nzb_id
                                   for m in metrics(target)), 'bounded PAR startup before source bytes')
        else:
            wait_until(lambda: all(f['ID'] != index_id for f in api.listfiles(0, 0, nzb_id)),
                       'explicitly resumed metadata index completion')
        assert api.editqueue('FileResume', '', sources)
        history = daemon.wait_history(api, name, timeout=120)
        events = metrics(target)
        log = target.read_file('nzbget.log').decode(errors='replace')
        time.sleep(0.25)  # nserv's final request-log flush
        requests = re.findall(r'Received: (?:BODY|ARTICLE) [^\r\n]*',
                              target.read_file('nserv.log').decode(errors='replace'))
        assert requests, 'empty provider capture cannot prove fetch policy'
        recovery_requests = [line for line in requests if '.vol' in line.lower()]
        result.update(status=history.get('Status'), par_status=history.get('ParStatus'),
                      metrics=events, recovery_requests=recovery_requests)
        if case == 'ram_part40':
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
            assert not recovery_requests, 'disabled repair fetched a recovery volume'
            assert 'paused volume(s) released' not in log, 'disabled repair released a live hold'
            assert not history.get('Status', '').startswith('SUCCESS'), history
        result['passed'] = True
    except Exception as error:
        result['error'] = repr(error)
    finally:
        if proxy:
            proxy.close()
        target.teardown(True)
        Path(target.path('result.json')).write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, sort_keys=True), flush=True)
        print('Artifacts: ' + work, flush=True)
    return result['passed']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nzbget', required=True)
    parser.add_argument('--par2', default=shutil.which('par2'))
    parser.add_argument('--case', choices=CASES + ('all',), default='ram_part40')
    args = parser.parse_args()
    assert args.par2, 'independent par2 reference tool is required'
    binary, par2 = str(Path(args.nzbget).resolve()), str(Path(args.par2).resolve())
    cases = CASES if args.case == 'all' else (args.case,)
    outcomes = [run_case(binary, par2, case) for case in cases]
    return 0 if all(outcomes) else 1


if __name__ == '__main__':
    raise SystemExit(main())
