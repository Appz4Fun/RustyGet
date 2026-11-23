// The Add dialog's grouping of copies of one release (webui/dupegroup.js),
// docs/DROP_AS_FLEET.md. Plain Node, no browser and no daemon:
//   node tests/webui/dupe-group.js
// The normalizer cases are the ones the Rust unit tests check
// (rust/src/releasename.rs), so the two copies of the rule stay the same.
const assert = require('node:assert/strict');
const path = require('node:path');
const DupeGroup = require(path.join(__dirname, '..', '..', 'webui', 'dupegroup.js'));

let passed = 0;
const failures = [];
function check(label, fn) {
  try {
    fn();
    passed++;
  } catch (error) {
    failures.push(`${label}: ${error.message}`);
  }
}

const BASE = 'Dune.Part.Two.2024.UHD.BluRay.2160p.TrueHD.Atmos.7.1.DV.HEVC.REMUX-FraMeSToR';
const WANT = 'dune.part.two.2024.uhd.bluray.2160p.truehd.atmos.7.1.dv.hevc.remux.framestor';

check('normalize matches the daemon (releasename.rs vectors)', () => {
  for (const [name, want] of [
    ['Lucifer S02E14 1080p BluRay x264-DEFLATE.nzb', 'lucifer.s02e14.1080p.bluray.x264.deflate'],
    ['Some.Movie.2020.1080p.WEB-DL-GRP-xpost [nzbgeek].nzb', 'some.movie.2020.1080p.web.dl.grp'],
    ['A_B--C.mkv', 'a.b.c'],
    ['Show.S01E01.1080p.WEB.H264-GRP', 'show.s01e01.1080p.web.h264.grp'],
    ['Movie.2020.1080p.WEB.DDP5.1.H.265', 'movie.2020.1080p.web.ddp5.1.h.265'],
    ['Movie.2020.1080p.WEB-GRP.mkv.001', 'movie.2020.1080p.web.grp'],
  ]) assert.equal(DupeGroup.normalize(name), want, name);
});

check('copy suffixes are stripped', () => {
  assert.equal(DupeGroup.groupName(BASE), WANT);
  for (const suffix of ['.nzb', ' (1).nzb', '(12).nzb', '_1.nzb', '_14.nzb', '-3.nzb', '.2.nzb',
    '.nzb.queued', '.nzb.2.queued', '_5.nzb.2.queued', '_1 (1).nzb', '.nzb.nzb']) {
    assert.equal(DupeGroup.groupName(BASE + suffix), WANT, suffix);
  }
  assert.equal(DupeGroup.groupKey('X.2024.1080p-GRP_2.nzb'), 'dupes:x.2024.1080p.grp');
});

check('digits that may be the name itself stay', () => {
  for (const [name, kept] of [
    ['Movie.Part.2', 'Movie.Part.2'],
    ['Movie.2024.1080p.DDP5.1', 'Movie.2024.1080p.DDP5.1'],
    ['Show.S01E01-02', 'Show.S01E01-02'],
    ['Movie.2024.1080p.H-264', 'Movie.2024.1080p.H-264'],
    ['Movie.2024.1080p-GRP2', 'Movie.2024.1080p-GRP2'],
    ['Movie.2024.1080p-D3FiL3D', 'Movie.2024.1080p-D3FiL3D'],
    ['Movie.2024.1080p-EPSiLON2.nzb', 'Movie.2024.1080p-EPSiLON2'],
    ['Movie_2', 'Movie_2'],
    ['Movie.2024', 'Movie.2024'],
    ['Movie.2024.1080p-GRP_2', 'Movie.2024.1080p-GRP'],
    ['Movie.2024.1080p-GRP_2 (3)', 'Movie.2024.1080p-GRP'],
    ['Movie.2024.1080p-GRP.1234', 'Movie.2024.1080p-GRP.1234'],
  ]) assert.equal(DupeGroup.stripCopySuffix(name), kept, name);
  assert.equal(DupeGroup.groupKey(''), '');
  assert.equal(DupeGroup.groupKey('(1).nzb'), '');
});

const ON = { enabled: true, force: false };

check('suffixed copies form one fleet', () => {
  const files = [`${BASE}.nzb`, `${BASE} (1).nzb`, `${BASE}_1.nzb`, `${BASE}_2.nzb`]
    .map((name, i) => ({ name, content: `c${i}`, dupekey: '' }));
  const plan = DupeGroup.plan(files, ON);
  assert.equal(plan.fleets.length, 1);
  assert.deepEqual(plan.fleets[0].members, [0, 1, 2, 3]);
  assert.equal(plan.fleets[0].key, `dupes:${WANT}`);
  assert.equal(plan.fleets[0].title, BASE);
  assert.deepEqual(plan.singles, []);
  assert.deepEqual(plan.dropped, []);
});

check('two releases form two fleets', () => {
  const other = 'Dune.Part.Two.2024.BluRay.1080p.TrueHD.Atmos.7.1.AVC.REMUX-FraMeSToR';
  const files = [`${BASE}.nzb`, `${other}.nzb`, `${BASE}_1.nzb`, `${other}_1.nzb`, `${other} (2).nzb`]
    .map((name, i) => ({ name, content: `c${i}`, dupekey: '' }));
  const plan = DupeGroup.plan(files, ON);
  assert.equal(plan.fleets.length, 2);
  assert.deepEqual(plan.fleets[0].members, [0, 2]);
  assert.deepEqual(plan.fleets[1].members, [1, 3, 4]);
  assert.notEqual(plan.fleets[0].key, plan.fleets[1].key);
});

check('a single file stays a plain append', () => {
  const plan = DupeGroup.plan([{ name: `${BASE}.nzb`, content: 'a', dupekey: '' }], ON);
  assert.deepEqual(plan.fleets, []);
  assert.deepEqual(plan.singles, [0]);
  const mixed = DupeGroup.plan([
    { name: `${BASE}.nzb`, content: 'a', dupekey: '' },
    { name: 'Other.Show.S01E01.1080p.WEB-GRP.nzb', content: 'b', dupekey: '' },
  ], ON);
  assert.deepEqual(mixed.fleets, []);
  assert.deepEqual(mixed.singles, [0, 1]);
});

check('a user-supplied DupeKey wins', () => {
  const files = [
    { name: `${BASE}.nzb`, content: 'a', dupekey: '' },
    { name: `${BASE}_1.nzb`, content: 'b', dupekey: 'imdb=15239678' },
  ];
  const plan = DupeGroup.plan(files, ON);
  assert.equal(plan.fleets.length, 1);
  assert.equal(plan.fleets[0].key, 'imdb=15239678');
});

check('exact duplicate files are dropped before upload', () => {
  const files = [
    { name: `${BASE}.nzb`, content: 'same', dupekey: '' },
    { name: `${BASE}_1.nzb`, content: 'same', dupekey: '' },
    { name: `${BASE}_2.nzb`, content: 'other', dupekey: '' },
  ];
  const plan = DupeGroup.plan(files, ON);
  assert.deepEqual(plan.dropped, [{ index: 1, sameAs: 0 }]);
  assert.deepEqual(plan.fleets[0].members, [0, 2]);
  // two identical files and nothing else: one plain append
  const pair = DupeGroup.plan(files.slice(0, 2), ON);
  assert.deepEqual(pair.fleets, []);
  assert.deepEqual(pair.singles, [0]);
});

check('box off or Force on: every file on its own, as before', () => {
  const files = [`${BASE}.nzb`, `${BASE}_1.nzb`, `${BASE}_1.nzb`]
    .map(name => ({ name, content: 'same', dupekey: '' }));
  for (const opts of [{ enabled: false, force: false }, { enabled: true, force: true }, undefined]) {
    const plan = DupeGroup.plan(files, opts);
    assert.deepEqual(plan.fleets, []);
    assert.deepEqual(plan.dropped, []);
    assert.deepEqual(plan.singles, [0, 1, 2]);
  }
});

check('F9: another container is unverified, never another video', () => {
  assert.equal(DupeGroup.kindLabel('twin'), 'twin');
  assert.equal(DupeGroup.kindLabel('twin:3/4'), 'twin');
  assert.equal(DupeGroup.kindLabel('alt'), 'alt');
  // par2 checksums differ: other packing only
  assert.equal(DupeGroup.kindLabel('container'), 'unverified');
  // the video inside read and different
  assert.equal(DupeGroup.kindLabel('other'), 'other');
  assert.equal(DupeGroup.kindLabel(''), '');
  assert.equal(DupeGroup.kindLabel(undefined), '');
});

if (failures.length) {
  console.error(failures.join('\n'));
  console.error(`dupe-group: ${failures.length} failed, ${passed} passed`);
  process.exit(1);
}
console.log(`dupe-group: ${passed} passed`);
