#!/usr/bin/env python3
"""STAT the first and the last article of every RAR volume and every par2 file of every release.
Usage: python3 fullscan.py OUT.jsonl DIR...      (resumable; NNTP server as in census.py)
       python3 fullscan.py --summary FILE.jsonl...
Per release: set key (RAR set name), per-volume first/last alive, per-par2 first/last alive."""
import sys, os, re, json, threading, queue
from collections import Counter, defaultdict
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import census as cz

def stat(c, mid):
    return c.cmd('STAT <' + mid.strip('<>') + '>').startswith('223')

def setkey(vols):
    n = vols[0][1].lower()
    n = re.sub(r'\.part\d+\.rar$|\.rar$|\.r\d\d$', '', n)
    return n

def one(c, p):
    fl = cz.files(p)
    vols = sorted((cz.volno(n), n, s) for n, s in fl if cz.volno(n) is not None)
    if len(vols) < 3: return dict(nzb=p, skip=1)
    pars = [(n, s) for n, s in fl if n.lower().endswith('.par2')]
    mids = ([s[0][1] for _, _, s in vols] + [s[-1][1] for _, _, s in vols]
            + [s[0][1] for _, s in pars] + [s[-1][1] for _, s in pars])
    ok = pstat(c, mids)
    nv, np_ = len(vols), len(pars)
    return dict(nzb=p, key=setkey(vols), sizes=[sum(x[2] for x in s) for _, _, s in vols],
                vf=ok[:nv], vl=ok[nv:2 * nv], pf=ok[2 * nv:2 * nv + np_], pl=ok[2 * nv + np_:],
                pn=[n for n, _ in pars])

def pstat(c, mids, window=50):
    """STATs pipelined in windows: one round trip per window instead of per article."""
    out = []
    for i in range(0, len(mids), window):
        chunk = mids[i:i + window]
        c.s.sendall(''.join('STAT <' + m.strip('<>') + '>\r\n' for m in chunk).encode())
        for _ in chunk:
            line = c.f.readline()
            if not line: raise EOFError('connection closed')
            out.append(line.startswith(b'223'))
    return out

def run(out, dirs):
    allf = []
    for d in dirs:
        for root, _, fs in os.walk(d):
            allf += [os.path.join(root, f) for f in fs if re.search(r'\.nzb(\.\d+)?(\.queued|\.gz|\.error)?$', f, re.I)]
    done = set()
    if os.path.exists(out):
        done = {json.loads(l)['nzb'] for l in open(out) if l.strip()}
    print('nzbs', len(allf), 'already done', len(done), flush=True)
    q = queue.Queue(); [q.put(p) for p in allf if p not in done]
    f = open(out, 'a'); lock = threading.Lock(); n = [0]
    def w():
        c = None
        while True:
            try: p = q.get_nowait()
            except queue.Empty: return
            rec = None
            for a in range(3):
                try:
                    if c is None: c = cz.C()
                    rec = one(c, p); break
                except (OSError, EOFError) as e:
                    c = None; rec = dict(nzb=p, err=str(e)[:80])
                except Exception as e:
                    rec = dict(nzb=p, err='parse: ' + str(e)[:80]); break
            with lock:
                f.write(json.dumps(rec) + '\n'); f.flush(); n[0] += 1
                if n[0] % 500 == 0: print('done', n[0], flush=True)
    ts = [threading.Thread(target=w) for _ in range(int(os.environ.get('CONNS', '16')))]
    [t.start() for t in ts]; [t.join() for t in ts]
    print('scan finished', flush=True)

def summary(paths):
    rs = [json.loads(l) for p in paths for l in open(p) if l.strip()]
    print('nzbs scanned', len(rs), ' errors', sum('err' in r for r in rs), ' under 3 rar volumes', sum('skip' in r for r in rs))
    rs = [r for r in rs if 'vf' in r]
    print('releases (nzbs) with >= 3 rar volumes:', len(rs))
    def kind(r):
        vf, vl = r['vf'], r['vl']
        if all(vf) and all(vl): return 'intact (all first+last alive)'
        if not any(vf) and not any(vl): return 'all volume first+last dead (takedown)'
        if not any(vf) and all(vl): return 'EVERY volume lost its first article, every last alive'
        if not any(vf): return 'every volume lost its first article, some lasts dead'
        if not all(vf): return 'some volumes lost their first article'
        return 'firsts alive, some lasts dead'
    k = Counter(kind(r) for r in rs)
    for a, b in k.most_common(): print(f'  {b:6d}  {a}')
    allf = [r for r in rs if not any(r['vf']) and any(r['vl'])]
    def parpath(r):
        if not r['pf']: return 'no par2 files'
        if any(r['pf']): return 'some par2 first articles alive'
        if any(r['pl']): return 'every par2 lost its first article, some par2 last articles alive'
        return 'every par2 first+last dead'
    print('releases where every volume lost its first article but some last articles live:', len(allf))
    for a, b in Counter(parpath(r) for r in allf).most_common(): print(f'  {b:6d}  {a}')
    # copies: same RAR set name
    groups = defaultdict(list)
    for r in rs: groups[r['key']].append(r)
    multi = {k: g for k, g in groups.items() if len(g) > 1}
    print('distinct RAR set names', len(groups), ' with more than one copy', len(multi))
    worst = [k for k, g in groups.items() if all(not any(r['vf']) for r in g)]
    worst_live = [k for k in worst if any(any(r['vl']) for r in groups[k])]
    print('set names where EVERY copy lost the first article of EVERY volume:', len(worst),
          ' (of which some last articles alive:', len(worst_live), ', more than one copy:', sum(len(groups[k]) > 1 for k in worst_live), ')')
    for k in worst_live[:25]:
        g = groups[k]
        print('   ', k[:90], 'copies', len(g), 'vols', [len(r['vf']) for r in g], 'par2', [len(r['pf']) for r in g])

if __name__ == '__main__':
    if sys.argv[1] == '--summary': summary(sys.argv[2:])
    else:
        cz.CFG = cz._cfg(); run(sys.argv[1], sys.argv[2:])
