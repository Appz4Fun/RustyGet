#!/usr/bin/env python3
"""Replay a real release through a throwaway RustyGet instance with the first article of every
RAR volume and every par2 file killed (message-id swapped for one no server has).
Usage: sudo python3 replay.py NAME BINARY PRIMARY_NZB [ALT_NZB...] [--nopar]
Runs on port 6799 in /mnt/20tb/hltest/NAME; never touches 6789/6790. Prints the outcome."""
import sys, os, re, time, json, base64, shutil, subprocess, xmlrpc.client, xml.etree.ElementTree as ET
args = [a for a in sys.argv[1:] if not a.startswith('--')]
nopar = '--nopar' in sys.argv
name, binary, primary, alts = args[0], args[1], args[2], args[3:]
base = f'/mnt/20tb/hltest/{name}'
shutil.rmtree(base, ignore_errors=True)
os.makedirs(base)
NS = 'http://www.newzbin.com/DTD/2003/nzb'
ET.register_namespace('', NS)

def killed(path, tag):
    tree = ET.parse(path); root = tree.getroot(); n = 0; drop = []
    for f in root.iter('{%s}file' % NS):
        subj = f.get('subject', ''); m = re.search(r'"([^"]+)"', subj); fn = (m.group(1) if m else subj).lower()
        is_vol = re.search(r'\.part\d+\.rar$|\.rar$|\.r\d\d$', fn)
        is_par = fn.endswith('.par2')
        if is_par and nopar:
            drop.append(f); continue
        if not (is_vol or is_par): continue
        segs = sorted(f.iter('{%s}segment' % NS), key=lambda s: int(s.get('number')))
        if segs:
            segs[0].text = f'hlkill-{tag}-{n}@invalid.invalid'; n += 1
    for f in drop: root.remove(f)
    out = os.path.join(base, f'{tag}.nzb'); tree.write(out, xml_declaration=True, encoding='UTF-8')
    return out, n, len(drop)

conf = open('/var/lib/rustyget/rustyget.conf').read()
srv = [l for l in conf.splitlines() if re.match(r'Server1\.(Host|Port|Username|Password|Encryption)=', l)]
cfg = [f'MainDir={base}/main', f'DestDir={base}/main/dst', f'InterDir={base}/main/inter', f'NzbDir={base}/main/nzb',
       f'QueueDir={base}/main/queue', f'TempDir={base}/main/tmp', f'LogFile={base}/rustyget.log', f'WebDir={base}/main/web',
       f'LockFile={base}/rustyget.lock', 'ConfigTemplate=', 'RequiredDir=', 'WriteLog=append', 'OutputMode=log',
       'ControlIP=127.0.0.1', 'ControlPort=6799', 'ControlUsername=', 'ControlPassword=',
       'Server1.Active=yes', 'Server1.Level=0', 'Server1.Connections=24', 'UpdateCheck=none', 'DupeCheck=yes',
       'DupeArticleFallback=live', 'DupeStreamDecompress=yes', 'ContinuePartial=yes', 'ArticleCache=2048',
       'FileNaming=auto', 'ReorderFiles=yes', 'PostStrategy=rocket', 'ParCheck=auto', 'ParRepair=yes',
       'ParScan=dupe', 'ParQuick=yes', 'ParRename=yes', 'RarRename=yes', 'DirectRename=yes', 'HealthCheck=dupe',
       'Unpack=yes', 'DirectUnpack=yes', 'ArticleRetries=0', 'NzbCleanupDisk=no'] + srv
for d in ('main', 'main/dst', 'main/inter', 'main/nzb', 'main/queue', 'main/tmp', 'main/web'):
    os.makedirs(f'{base}/{d}', exist_ok=True)
open(f'{base}/rustyget.conf', 'w').write('\n'.join(cfg) + '\n'); os.chmod(f'{base}/rustyget.conf', 0o600)

p_nzb, pk, pd = killed(primary, 'primary')
alt_nzbs = [killed(a, f'alt{i + 1}') for i, a in enumerate(alts)]
print(f'primary {os.path.basename(primary)}: killed {pk} first articles, dropped {pd} par2 files')
for a, (_, k, d) in zip(alts, alt_nzbs): print(f'alt {os.path.basename(a)}: killed {k}, dropped {d} par2 files')
proc = subprocess.Popen([binary, '-c', f'{base}/rustyget.conf', '-s'], stdout=open(f'{base}/stdout.log', 'w'), stderr=subprocess.STDOUT)
api = xmlrpc.client.ServerProxy('http://127.0.0.1:6799/xmlrpc')
for _ in range(60):
    try: api.version(); break
    except Exception: time.sleep(1)
def add(path, nm, paused, score):
    data = base64.standard_b64encode(open(path, 'rb').read()).decode()
    params = []
    return api.append(nm, data, 'test', 0, False, paused, 'hl-key', score, 'score', params)
for i, (pth, _, _) in enumerate(alt_nzbs): add(pth, f'Alt{i + 1}', True, 50 - i)
add(p_nzb, 'Primary', False, 100)
t0 = time.time(); res = None
while time.time() - t0 < 4 * 3600:
    time.sleep(20)
    if proc.poll() is not None: break
    try:
        h = api.history(False); g = api.listgroups(0); pq = api.postqueue(0)
    except Exception: continue
    active = [x for x in g if not str(x.get('Status', '')).startswith('PAUSED')]
    if h and not active and not pq:
        ok = [x for x in h if str(x.get('Status', '')).startswith('SUCCESS')]
        res = (ok or h)[0]; res['History'] = [(x['Name'], x['Status']) for x in h]; break
dt = time.time() - t0
mkvs = []
for root, _, fs in os.walk(f'{base}/main/dst'):
    mkvs += [(os.path.join(root, f), os.path.getsize(os.path.join(root, f))) for f in fs if f.lower().endswith('.mkv')]
print('result', json.dumps(dict(name=res and res['Name'], history=res and res['History'], status=res and res['Status'], par=res and res.get('ParStatus'), unpack=res and res.get('UnpackStatus'),
      minutes=round(dt / 60, 1), mkv=[(os.path.basename(a), b) for a, b in mkvs])))
log = open(f'{base}/rustyget.log', errors='replace').read().splitlines()
keys = re.compile(r'headers|quick-open|Cross-pack|Recovered|still missing|Repair|repaired|par2|Unpack|unpack|failed|Failing over|borrow', re.I)
for l in [l for l in log if keys.search(l) and '\tDETAIL\t' not in l][-40:]: print('   ', l[:260])
try: api.shutdown()
except Exception: pass
proc.wait(timeout=60)
