#!/usr/bin/env python3
# Searches the local nzbhydra2 for each query and downloads every result's NZB into ./u5/<tag>/.
import os, re, sys, glob, json, urllib.request, urllib.parse, xml.etree.ElementTree as ET
cfg = None
for p in glob.glob('/mnt/nvme/containers/nzbhydra/data/**/nzbhydra.yml', recursive=True) + glob.glob('/var/lib/nzbhydra*/**/nzbhydra.yml', recursive=True) + glob.glob('/opt/nzbhydra*/**/nzbhydra.yml', recursive=True) + glob.glob('/home/*/.config/nzbhydra*/**/nzbhydra.yml', recursive=True) + glob.glob('/root/**/nzbhydra.yml', recursive=True):
    cfg = p; break
if not cfg:
    print('no nzbhydra.yml found'); sys.exit(1)
key = os.environ['HK']
print('config', cfg)
Q = {'franklin': 'Franklin Bash S02E01', 'mass': 'The Mass Is Ended 1985', 'constellation': 'Constellation S01E05',
     'thirdday': 'The Third Day S01E01', 'trust': 'Trust 2018 S01E02'}
WANT = {'franklin': r'franklin.*bash.*s02e01.*jbent', 'mass': r'mass.*ended.*usury', 'constellation': r'constellation.*s01e05.*flux',
        'thirdday': r'third.*day.*s01e01.*ntb', 'trust': r'trust.*s01e02.*edith'}
for tag, q in Q.items():
    url = 'http://127.0.0.1:5076/api?' + urllib.parse.urlencode(dict(t='search', q=q, apikey=key, limit=100))
    try:
        root = ET.fromstring(urllib.request.urlopen(url, timeout=120).read())
    except Exception as e:
        print(tag, 'search failed', e); continue
    items = root.iter('item')
    os.makedirs(f'u5/{tag}', exist_ok=True)
    hits = 0
    for it in items:
        title = it.findtext('title') or ''
        if not re.search(WANT[tag], title, re.I): continue
        link = it.find('enclosure').get('url') if it.find('enclosure') is not None else it.findtext('link')
        hits += 1
        dst = f'u5/{tag}/{hits:02d}-' + re.sub(r'[^\w.\-]+', '_', title)[:150] + '.nzb'
        try:
            open(dst, 'wb').write(urllib.request.urlopen(link, timeout=120).read())
            print(tag, 'got', title)
        except Exception as e:
            print(tag, 'download failed', title, e)
    print(tag, 'matching results', hits)
