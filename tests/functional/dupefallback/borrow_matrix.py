#!/usr/bin/env python3
#
#  This file is part of nzbget. See <https://nzbget.com>.
#
#  Timed borrowing matrix. One inner file is posted in several packings (bare,
#  RAR4 store, RAR5 store, 7z copy, 7z LZMA2 raw chunks, 7z with encrypted
#  headers, a nested archive). Each scenario downloads one packing with a share
#  of its articles missing, with another packing (an "alt") or the same packing
#  posted again (a "twin") as the duplicate, through a rate-limited local news
#  server, and reports how long the download took against the same packing
#  downloaded clean.
#
#  usage: borrow_matrix.py --nzbget <binary> [--size-mb 500] [--rate-kb 10000]
#                          [--fail-percent 10] [--only <regex>] [--json out.json]
#
import argparse
import hashlib
import json
import os
import random
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generators  # noqa: E402
import harness  # noqa: E402

INNER = 'movie.mkv'
PASSWORD = 'matrix-secret'


def vint(value):
    out = bytearray()
    while True:
        b = value & 0x7f
        value >>= 7
        if value:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def rar5_block(body):
    """body = header type .. end of header; prepend size and CRC32."""
    sized = vint(len(body)) + body
    return struct.pack('<I', zlib.crc32(sized) & 0xffffffff) + sized


def rar5_store_volumes(inner_name, data, volume_size):
    """RAR5 store-mode volumes with real header and data CRCs. A non-final part carries the
    CRC of its own packed data, the final part the CRC of the whole file."""
    name = inner_name.encode()
    full_crc = zlib.crc32(data) & 0xffffffff
    count = (len(data) + volume_size - 1) // volume_size
    vols = []
    for k in range(count):
        chunk = data[k * volume_size:(k + 1) * volume_size]
        last = k == count - 1
        arc_flags = 0x0001 | (0x0002 if k > 0 else 0)          # volume, volume number present
        main = vint(1) + vint(0) + vint(arc_flags) + (vint(k) if k > 0 else b'')
        hdr_flags = 0x0002 | (0x0008 if k > 0 else 0) | (0x0010 if not last else 0)
        crc = full_crc if last else (zlib.crc32(chunk) & 0xffffffff)
        fh = vint(2) + vint(hdr_flags) + vint(len(chunk))
        fh += vint(0x0004) + vint(len(data)) + vint(0x20)       # file flags (crc), unpacked, attrs
        fh += struct.pack('<I', crc)
        fh += vint(0) + vint(1) + vint(len(name)) + name        # store, unix, name
        end = vint(5) + vint(0) + vint(0 if last else 0x0001)
        vols.append(b'Rar!\x1a\x07\x01\x00' + rar5_block(main) + rar5_block(fh) + chunk + rar5_block(end))
    return vols


def seven_zip(files, extra):
    seven = generators.SEVENZIP_PATH
    with tempfile.TemporaryDirectory(prefix='bm7z-') as work:
        names = []
        for name, data in files:
            with open(os.path.join(work, name), 'wb') as f:
                f.write(data)
            names.append(name)
        out = os.path.join(work, 'out.7z')
        subprocess.run([seven, 'a', '-bd'] + extra + [out] + names, cwd=work, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        with open(out, 'rb') as f:
            return f.read()


def split(data, size, fmt):
    return [(fmt % (i + 1), data[p:p + size]) for i, p in enumerate(range(0, len(data), size))]


def build_packings(data, vol):
    """name -> (list of (file name, bytes), password or None)."""
    packs = {}
    packs['bare'] = ([(INNER, data)], None)
    packs['rar4'] = ([('rel.part%02d.rar' % (i + 1), v)
                      for i, v in enumerate(generators.rar3_store_volumes_valid(INNER, data, vol))], None)
    packs['rar5'] = ([('rel.part%02d.rar' % (i + 1), v)
                      for i, v in enumerate(rar5_store_volumes(INNER, data, vol))], None)
    if generators.HAVE_7Z:
        packs['7zcopy'] = (split(seven_zip([(INNER, data)], ['-m0=Copy']), vol, 'rel.7z.%03d'), None)
        packs['7zlzma2'] = (split(seven_zip([(INNER, data)], ['-m0=LZMA2', '-mx=5']), vol, 'rel.7z.%03d'), None)
        packs['7zenc'] = (split(seven_zip([(INNER, data)], ['-m0=Copy', '-mhe=on', '-p' + PASSWORD]),
                                vol, 'rel.7z.%03d'), PASSWORD)
        # nested: the mkv in one RAR4, that in one RAR5, that in a 7z
        r4 = b''.join(generators.rar3_store_volumes_valid(INNER, data, len(data)))
        r5 = b''.join(rar5_store_volumes('inner.rar', r4, len(r4)))
        nested = seven_zip([('inner5.rar', r5)], ['-m0=Copy'])
        packs['nested'] = (split(nested, vol, 'rel.7z.%03d'), None)
    return packs


class Bench:
    def __init__(self, args):
        self.args = args
        self.root = tempfile.mkdtemp(prefix='borrow-matrix-')
        self.datadir = os.path.join(self.root, 'data')
        os.makedirs(self.datadir)
        self.size = args.size_mb * 1000 * 1000
        self.seg = args.article_kb * 1000
        self.vol = args.volume_mb * 1000 * 1000
        self.data = b'\x1a\x45\xdf\xa3' + os.urandom(self.size - 4)
        self.md5 = hashlib.md5(self.data).hexdigest()
        t0 = time.time()
        self.packs = build_packings(self.data, self.vol)
        self.placed = {}
        print('built %d packings of a %d MB file in %.0f s' % (len(self.packs), args.size_mb, time.time() - t0),
              flush=True)
        self.results = []

    def place(self, pack, copy):
        """Write packing `pack` as post `copy` (a distinct set of message-ids) and return
        [(served path, name, size)]."""
        key = (pack, copy)
        if key not in self.placed:
            files, _ = self.packs[pack]
            out = []
            for name, blob in files:
                rel = '%s-%s/%s' % (pack, copy, name)
                full = os.path.join(self.datadir, rel)
                os.makedirs(os.path.dirname(full), exist_ok=True)
                with open(full, 'wb') as f:
                    f.write(blob)
                out.append((rel, name, len(blob)))
            self.placed[key] = out
        return self.placed[key]

    def members(self, pack, copy, fail_percent, seed, with_par):
        rnd = random.Random(seed)
        members = []
        for rel, name, size in self.place(pack, copy):
            parts = (size + self.seg - 1) // self.seg
            missing = set()
            if fail_percent > 0:
                # never the first article of a volume: it holds the archive headers
                pool = list(range(2, parts + 1))
                want = max(1, round(len(pool) * fail_percent / 100.0)) if pool else 0
                missing = set(rnd.sample(pool, min(want, len(pool))))
            members.append((rel, name, size, self.seg, missing))
        if with_par:
            files, _ = self.packs[pack]
            par = generators.par2_index(files)
            rel = '%s-%s/rel.par2' % (pack, copy)
            with open(os.path.join(self.datadir, rel), 'wb') as f:
                f.write(par)
            members.append((rel, 'rel.par2', len(par), self.seg, set()))
        return members

    def run(self, label, target, donors=(), fail=0, twin=False, with_par=False, extra=(), note=''):
        """donors: list of packing names. A donor equal to the target packing is a twin."""
        if self.args.only and not re.search(self.args.only, label):
            return None
        stage = tempfile.mkdtemp(prefix='bm-run-', dir=self.root)
        t = harness.LocalTarget(self.args.nzbget, stage)
        nntp = harness.free_port()
        rpc = harness.free_port()
        while rpc == nntp:
            rpc = harness.free_port()
        daemon = harness.Daemon(t, nntp, rpc)
        daemon.datadir = self.datadir
        options = ['DupeArticleFallback=live', 'ParCheck=auto', 'ParRepair=yes', 'Unpack=yes',
                   'DirectUnpack=yes', 'DirectRename=no', 'HealthCheck=none', 'ArticleRetries=0',
                   'ArticleInterval=0', 'Server1.Connections=%d' % self.args.connections,
                   'ArticleCache=200', 'DownloadRate=%d' % self.args.rate_kb,
                   'DupeStreamDecompress=yes', 'DetailTarget=log', 'InfoTarget=log',
                   'UnpackCleanupDisk=yes'] + list(extra)
        res = {'label': label, 'target': target, 'donors': list(donors), 'fail_percent': fail, 'note': note}
        try:
            daemon.write_config(options)
            daemon.start_nserv()
            daemon.start()
            api = daemon.wait_ready()
            tmem = self.members(target, 'T', fail, 1000 + len(self.results), with_par)
            res['missing_articles'] = sum(len(m[4]) for m in tmem)
            res['articles'] = sum((m[2] + self.seg - 1) // self.seg for m in tmem)
            for i, d in enumerate(donors):
                dmem = self.members(d, 'D%d' % i, 0, 0, with_par)
                dnzb = harness.build_multi_nzb(dmem, password=self.packs[d][1])
                daemon.append(api, 'Donor%d' % i, dnzb, True, 'bm-key', 50)
            tnzb = harness.build_multi_nzb(tmem, password=self.packs[target][1])
            start = time.time()
            daemon.append(api, 'Release', tnzb, False, 'bm-key', 100)
            h = daemon.wait_history(api, 'Release', timeout=self.args.timeout)
            res['seconds'] = round(time.time() - start, 1)
            res['status'] = h['Status']
            res['download_sec'] = h.get('DownloadTimeSec')
            res['post_sec'] = h.get('PostTotalTimeSec')
            res['downloaded_mb'] = h.get('DownloadedSizeMB')
            res['recovered_articles'] = int(h.get('DupeRecoveredArticles', 0))
            ok = False
            for base in (('main', 'dst'), ('main', 'inter')):
                for rel in t.find_files(*base):
                    if rel.endswith('.mkv') and os.path.getsize(t.path(rel)) == self.size:
                        with open(t.path(rel), 'rb') as f:
                            ok = ok or hashlib.md5(f.read()).hexdigest() == self.md5
            res['output_identical'] = ok
            log = t.read_file('nzbget.log').decode('latin1', 'replace')
            res['how'] = sorted(set(re.findall(r'\((cross-packing|decompressed|7z raw chunks)\)', log)))
            if re.search(r'Recovered \d+ of \d+ missing article', log):
                res['how'].append('twin articles')
            if re.search(r'Recovered [\d.]+ MB \(\d+ donor article', log) and not res['how']:
                res['how'].append('same-packing bytes')
            res['borrowed_mb'] = round(sum(float(x) for x in re.findall(r'Recovered ([\d.]+) MB', log)), 1)
            res['skips'] = sorted(set(m[:110] for m in re.findall(
                r'(?:Skipping packing|Not borrowing from 7z|Skipping decompression|not possible)[^\n]*', log)))[:3]
        except Exception as e:  # noqa: BLE001
            res['error'] = repr(e)[:200]
            res['seconds'] = round(time.time() - start, 1) if 'start' in dir() else None
        finally:
            t.teardown(self.args.keep)
        self.results.append(res)
        print('%-34s %7s s  %-18s ok=%-5s borrowed=%s MB how=%s %s' % (
            label, res.get('seconds'), res.get('status'), res.get('output_identical'),
            res.get('borrowed_mb'), ','.join(res.get('how', [])), res.get('error', '')), flush=True)
        return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--nzbget', required=True)
    ap.add_argument('--size-mb', type=int, default=500)
    ap.add_argument('--volume-mb', type=int, default=50)
    ap.add_argument('--article-kb', type=int, default=700)
    ap.add_argument('--rate-kb', type=int, default=10000)
    ap.add_argument('--connections', type=int, default=8)
    ap.add_argument('--fail-percent', type=float, default=10)
    ap.add_argument('--timeout', type=int, default=1500)
    ap.add_argument('--only')
    ap.add_argument('--json')
    ap.add_argument('--keep', action='store_true')
    args = ap.parse_args()

    b = Bench(args)
    f = args.fail_percent
    plain = [p for p in ('bare', 'rar4', 'rar5', '7zcopy') if p in b.packs]
    try:
        for p in plain:
            b.run('clean %s' % p, p)
        # alts: every target packing against every other packing
        for tgt in plain:
            for don in plain + (['7zlzma2'] if '7zlzma2' in b.packs else []):
                if don != tgt:
                    b.run('alt %s <- %s' % (tgt, don), tgt, [don], fail=f)
        # twins: the same post again; with a par2 index articles are swapped one for one
        for tgt in plain:
            b.run('twin %s (par2)' % tgt, tgt, [tgt], fail=f, with_par=True)
            b.run('twin %s (no par2)' % tgt, tgt, [tgt], fail=f)
        # lighter and heavier damage on one pair
        if 'rar5' in b.packs:
            b.run('alt rar5 <- rar4 1%', 'rar5', ['rar4'], fail=1)
            b.run('alt rar5 <- rar4 30%', 'rar5', ['rar4'], fail=30)
            b.run('twin rar5 (par2) 30%', 'rar5', ['rar5'], fail=30, with_par=True)
            b.run('alt rar5 <- 2 alts', 'rar5', ['rar4', 'bare'], fail=f)
        # encrypted headers
        if '7zenc' in b.packs:
            b.run('clean 7zenc', '7zenc')
            b.run('alt rar5 <- 7zenc', 'rar5', ['7zenc'], fail=f)
            b.run('alt 7zenc <- rar5', '7zenc', ['rar5'], fail=f)
            b.run('twin 7zenc (no par2)', '7zenc', ['7zenc'], fail=f)
        # nested archive as target and as donor
        if 'nested' in b.packs:
            b.run('alt rar5 <- nested', 'rar5', ['nested'], fail=f)
        # no duplicate at all: what the damage costs without borrowing
        b.run('no alt rar5 (fails)', 'rar5', [], fail=f)
    finally:
        if args.json:
            with open(args.json, 'w') as out:
                json.dump({'size_mb': args.size_mb, 'rate_kb': args.rate_kb, 'results': b.results}, out, indent=1)
        if not args.keep:
            shutil.rmtree(b.root, ignore_errors=True)
    return 0


if __name__ == '__main__':
    sys.exit(main())
