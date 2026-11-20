#!/usr/bin/env python3
"""Measured from real NZBs: RAR format split, RAR5 quick-open copies, par2 index packets in vol tails.
Usage (on the server, needs the NNTP server in /var/lib/rustyget/rustyget.conf):
  sudo python3 census.py N SEED DIR...   -> census.jsonl + summary on stdout
Per sampled release (>= 3 RAR volumes):
  first article of volume 1: rar4 / rar5 / rar5-hp (header-encrypted) / other / missing
  rar5: last article of a middle volume: quick-open record ('QO') present, recovery record ('RR')
  par2: last and first article of the largest .volNN par2 file: Main / FileDesc / IFSC packets present
"""
import sys, os, re, json, random, socket, threading, queue, gzip, binascii, xml.etree.ElementTree as ET
from collections import Counter
conf = open('/var/lib/rustyget/rustyget.conf').read()
g = lambda k: re.search(r'^Server1\.' + k + r'=(.*)$', conf, re.M).group(1).strip()
HOST, PORT, USER, PASS = g('Host'), int(g('Port')), g('Username'), g('Password')
NS = '{http://www.newzbin.com/DTD/2003/nzb}'

class C:
    def __init__(s):
        s.s = socket.create_connection((HOST, PORT), 30); s.f = s.s.makefile('rb')
        s.f.readline(); s.cmd('AUTHINFO USER ' + USER); s.cmd('AUTHINFO PASS ' + PASS)
    def cmd(s, c): s.s.sendall((c + '\r\n').encode()); return s.f.readline().decode('latin1').strip()
    def body(s, mid):
        r = s.cmd('BODY <' + mid.strip('<>') + '>')
        if not r.startswith('222'): return None
        lines = []
        while True:
            l = s.f.readline()
            if l in (b'.\r\n', b'.\n', b''): break
            if l.startswith(b'..'): l = l[1:]
            lines.append(l.rstrip(b'\r\n'))
        return ydec(lines)

def ydec(lines):
    out = bytearray()
    for l in lines:
        if l.startswith(b'=ybegin') or l.startswith(b'=ypart'): continue
        if l.startswith(b'=yend'): break
        i = 0; n = len(l)
        while i < n:
            c = l[i]
            if c == 61: i += 1; c = (l[i] - 64) & 255
            out.append((c - 42) & 255); i += 1
    return bytes(out)

N, SEED, dirs = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3:]
allf = []
for d in dirs:
    for root, _, fs in os.walk(d):
        allf += [os.path.join(root, f) for f in fs if re.search(r'\.nzb(\.queued|\.gz)?$', f, re.I)]
random.Random(SEED).shuffle(allf)
print('nzbs found', len(allf), flush=True)

def files(path):
    op = gzip.open if path.endswith('.gz') else open
    out = []
    for f in ET.parse(op(path)).getroot().iter(NS + 'file'):
        subj = f.get('subject', ''); m = re.search(r'"([^"]+)"', subj); name = m.group(1) if m else subj
        segs = sorted((int(s.get('number')), s.text.strip(), int(s.get('bytes', 0))) for s in f.iter(NS + 'segment'))
        if segs: out.append((name, segs))
    return out

def volno(name):
    m = re.search(r'\.part(\d+)\.rar$', name, re.I)
    if m: return int(m.group(1))
    if re.search(r'\.rar$', name, re.I): return 0
    m = re.search(r'\.r(\d\d)$', name, re.I)
    return int(m.group(1)) + 1 if m else None

def head_kind(b):
    if b.startswith(b'Rar!\x1a\x07\x01\x00'):
        i = 12
        while b[i] & 0x80: i += 1
        return 'rar5-hp' if b[i + 1] == 4 else 'rar5'
    if b.startswith(b'Rar!\x1a\x07\x00'): return 'rar4'
    return 'other'

PK = {'main': b'PAR 2.0\x00Main\x00\x00\x00\x00', 'filedesc': b'PAR 2.0\x00FileDesc', 'ifsc': b'PAR 2.0\x00IFSC\x00\x00\x00\x00'}
q = queue.Queue(); res = []; lock = threading.Lock()
for p in allf: q.put(p)
def w():
    c = None
    while True:
        with lock:
            if len(res) >= N: return
        try: p = q.get_nowait()
        except queue.Empty: return
        try: fl = files(p)
        except Exception: continue
        vols = sorted((volno(n), n, s) for n, s in fl if volno(n) is not None)
        if len(vols) < 3: continue
        pars = sorted(((sum(x[2] for x in s), n, s) for n, s in fl if re.search(r'\.vol\d+\+\d+\.par2$', n, re.I)), reverse=True)
        rec = dict(nzb=os.path.basename(p), vols=len(vols), par2vols=len(pars))
        for attempt in range(2):
            try:
                if c is None: c = C()
                b = c.body(vols[0][2][0][1])
                rec['head'] = 'missing' if b is None else head_kind(b[:64])
                if rec['head'] == 'rar5':
                    t = c.body(vols[len(vols) // 2][2][-1][1])
                    if t is None: rec['qo'] = 'missing'
                    else:
                        d = t[-8192:]
                        rec['qo'] = b'\x02QO' in d; rec['rr'] = b'\x02RR' in d
                if pars and len(pars[0][2]) >= 2:
                    s = pars[0][2]
                    lt, ft = c.body(s[-1][1]), c.body(s[0][1])
                    rec['par_last'] = None if lt is None else [k for k, v in PK.items() if v in lt]
                    rec['par_first'] = None if ft is None else [k for k, v in PK.items() if v in ft]
                    rec['par_segs'] = len(s)
                break
            except Exception as e:
                c = None; rec['err'] = str(e)[:80]
        with lock:
            if len(res) < N:
                res.append(rec)
                if len(res) % 50 == 0: print('done', len(res), flush=True)
ts = [threading.Thread(target=w) for _ in range(16)]; [t.start() for t in ts]; [t.join() for t in ts]
with open('census.jsonl', 'w') as f:
    for r in res: f.write(json.dumps(r) + '\n')
pc = lambda a, b: f'{a}/{b} ({100*a/max(b,1):.1f}%)'
print('releases sampled', len(res))
print('volume 1 format:', dict(Counter(r.get('head', 'err') for r in res)))
r5 = [r for r in res if r.get('head') == 'rar5' and isinstance(r.get('qo'), bool)]
print('unencrypted rar5, middle volume tail read:', len(r5))
print('  with quick-open copy:', pc(sum(r['qo'] for r in r5), len(r5)))
print('  with recovery record:', pc(sum(r['rr'] for r in r5), len(r5)))
pp = [r for r in res if isinstance(r.get('par_last'), list) and isinstance(r.get('par_first'), list)]
print('releases with a multi-article par2 vol file read:', len(pp))
for k in PK:
    print(f'  {k} in LAST article:', pc(sum(k in r['par_last'] for r in pp), len(pp)), ' in FIRST article:', pc(sum(k in r['par_first'] for r in pp), len(pp)))
print('  main+filedesc+ifsc all in last article:', pc(sum(set(PK) <= set(r['par_last']) for r in pp), len(pp)))
