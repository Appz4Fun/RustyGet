#!/usr/bin/env python3
# STAT first/last of every volume and par2 in each NZB given; print a per-copy damage line.
import sys, os, json
sys.path.insert(0, os.path.expanduser('~/header-loss'))
import census as cz, fullscan as fs
cz.CFG = cz._cfg()
c = cz.C()
for p in sys.argv[1:]:
    r = fs.one(c, p)
    if 'vf' not in r:
        print(os.path.basename(p), r); continue
    dead_first = [i + 1 for i, a in enumerate(r['vf']) if not a]
    dead_last = [i + 1 for i, a in enumerate(r['vl']) if not a]
    print(os.path.basename(p)[-60:], '| set', r['key'][-40:], '| vols', len(r['vf']),
          'first dead', len(dead_first), dead_first[:12], 'last dead', len(dead_last), dead_last[:12],
          '| par2', len(r['pf']), 'first dead', r['pf'].count(False), 'last dead', r['pl'].count(False))
