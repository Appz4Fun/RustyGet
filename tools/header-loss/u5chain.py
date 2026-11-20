#!/usr/bin/env python3
# Picks one NZB per distinct post for each of the user's 5 releases and replays each fleet in turn.
import os, re, glob, hashlib, subprocess, sys
B = os.path.expanduser('~allen/hl-build/rg-wip/rust/target/release/rustyget')
H = os.path.expanduser('~allen/header-loss')
def distinct(pat):
    seen, out = set(), []
    for f in sorted(glob.glob(pat)):
        d = open(f, errors='replace').read()
        ids = sorted(re.findall(r'<segment[^>]*>([^<]+)</segment>', d))
        if not ids: continue
        h = hashlib.md5(''.join(ids).encode()).hexdigest()
        if h not in seen: seen.add(h); out.append(f)
    return out
share = sorted(glob.glob('/mnt/20tb/nzbget/nzbs/Constellation.S01E05.*DV.HDR*FLUX.nzb*') + glob.glob('/mnt/20tb/nzbs/**/Constellation.S01E05.*DV.HDR*FLUX.nzb*', recursive=True))
six = [f for f in share if '.nzb.6' in f]
const = six + [f for f in share if f not in six] + distinct(f'{H}/u5/constellation/*.nzb')
fleets = [('u5const', const),
          ('u5franklin', distinct(f'{H}/u5/franklin/*.nzb')),
          ('u5thirdday', distinct(f'{H}/u5/thirdday/*.nzb')),
          ('u5trust', distinct(f'{H}/u5/trust/*DV*.nzb')),
          ('u5mass', distinct(f'{H}/u5/mass/*720p*.nzb')[:6])]
only = sys.argv[1:]
for name, fl in fleets:
    if only and name not in only: continue
    print('==', name, len(fl), 'copies', flush=True)
    with open(f'{H}/replay_{name}.log', 'w') as log:
        subprocess.run(['python3', f'{H}/replay.py', name, B] + fl, stdout=log, stderr=subprocess.STDOUT)
    print(open(f'{H}/replay_{name}.log').read().split('result', 1)[-1][:600], flush=True)
