// Copies under a DupeKey that are no fleet get the fleet's toggle and list in
// Downloads (6790, 2026-11-26: only the Dune fleet had the arrow; 105 other
// downloads added with dupe NZBs had their backups in History as
// DELETED/DUPE and showed none). The daemon's `history` reply (no argument:
// hidden DUP records left out, DELETED/DUPE NZB copies kept, with DupeKey and
// their Dupe* parameters) is what the list is built from, in the same
// renderer as a fleet (DownloadsUI.buildFleets / buildFleetRow).
// Checks: the toggle on a plain download with History backups, matched by
// DupeKey ignoring ASCII case; queued siblings keep their own rows, each with
// a toggle; no toggle without another copy; opened, the download first, then
// the History backups in donor order (twin, borrowable, ..., then score) with
// kind (twin / alt / container / other), alive share, borrowable and role,
// then failed History copies; a fleet also lists its History copies after its
// queued members; a History copy opens its History panel; a new History
// reply alone redraws the toggle; one index per refresh stays fast with 1400
// History items; desktop and phone, light and dark, no console errors.
// Offline (details-fixture): committed assets, real startup and renderers,
// synthetic RPC replies.
// Usage: node tests/webui/dupe-copies-downloads.js http://127.0.0.1:17999/
// Optional screenshots: NZBGET_UI_CAPTURE_DIR=/some/dir
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

const PLAIN = 'Example.Feature.1993.2160p.UHD.BluRay.REMUX-GRP';
const PLAIN_KEY = 'pair:example_feature_4k:1';
const SIB = 'Sample.Show.S01E01.1080p.WEB-DL.x264-GRP';
const SIB_KEY = 'pair:sample_show_s01e01:1';
const LONE = 'Lone.Release.2020.720p.HDTV.x264-GRP';
const FLEET = 'Fleet.Movie.2024.1080p.WEB-DL.x264-GRP';
const FLEET_KEY = 'dupes:fleet.movie.2024.1080p.web.dl.x264.grp';

function params(map) {
  return Object.entries(map).map(([Name, Value]) => ({ Name, Value }));
}

function item(base, id, name, key, extra) {
  const copy = JSON.parse(JSON.stringify(base));
  return Object.assign(copy, {
    NZBID: id, ID: id, FirstID: id * 10, LastID: id * 10 + 9, NZBName: name, NZBNicename: name, Name: name,
    NZBFilename: name + '.nzb', DupeKey: key, DupeMode: 'SCORE', Category: 'Movies', MessageCount: 0, Kind: 'NZB',
    SuccessArticles: 0, FailedArticles: 0, Health: 1000, CriticalHealth: 950, Log: []
  }, extra);
}

function queue(groups) {
  const base = groups[0];
  const held = { Status: 'PAUSED', MaxPriority: -100, MinPriority: -100 };
  return [
    item(base, 741, PLAIN, PLAIN_KEY, { Status: 'DOWNLOADING', ActiveDownloads: 8, DupeScore: 1,
      Parameters: params({ DupeFiles: 'e240e0b1925d79b6-174', DupeHealth: '100', DupeCopyKind: 'borrowable' }) }),
    item(base, 801, SIB, SIB_KEY, { Status: 'QUEUED', DupeScore: 2 }),
    item(base, 802, SIB + '_2', SIB_KEY, { Status: 'QUEUED', DupeScore: 1 }),
    item(base, 810, LONE, 'pair:lone_release:1', { Status: 'QUEUED', DupeScore: 1 }),
    item(base, 51, FLEET, FLEET_KEY, { Status: 'DOWNLOADING', ActiveDownloads: 4, DupeScore: 100,
      Parameters: params({ DupeFleetOrder: '0', DupeFleetPrimary: '51', DupeAlive: '99', DupeFleet: '1' }) }),
    item(base, 52, FLEET + '_1', FLEET_KEY, Object.assign({}, held, { DupeScore: 99,
      Parameters: params({ DupeHeld: 'yes', DupeFleetOrder: '1', DupeFleetPrimary: '51', DupeKind: 'alt',
        DupeCopyKind: 'borrowable', DupeAlive: '97', DupeFleet: '2' }) }))
  ];
}

// History, in the daemon's order (newest first), with copies of the plain
// download under its key in another case
function history(base, extra) {
  const now = Math.floor(Date.now() / 1000);
  const dupe = (id, suffix, key, score, p, t) => item(base, id, PLAIN + suffix, key, {
    Status: 'DELETED/DUPE', DeleteStatus: 'DUPE', DupeScore: score, HistoryTime: now - t, Parameters: params(p) });
  const list = [
    item(base, 90, 'Other.Show.S01E01.720p.HDTV.x264-OTHER', 'other-key', { Status: 'SUCCESS/ALL', DeleteStatus: 'NONE',
      ParStatus: 'SUCCESS', UnpackStatus: 'SUCCESS', HistoryTime: now - 30, Parameters: [] }),
    dupe(748, '_8', PLAIN_KEY, 1, { DupeFiles: '077a8410c5b9b152-1', DupeHealth: '100', DupeCopyKind: 'borrowable', DupeKind: 'container' }, 60),
    dupe(747, '_7', PLAIN_KEY.toUpperCase(), 1, { DupeFiles: 'eea70fdaf6846e6d-174', DupeHealth: '99', DupeCopyKind: 'borrowable', DupeKind: 'alt', DupeVideo: 'same' }, 61),
    dupe(746, '_6', PLAIN_KEY, 1, { DupeHealth: '0', DupeCopyKind: 'unborrowable', DupeKind: 'other', DupeAliveSample: '0/40', DupeAlive: '0' }, 62),
    dupe(745, '_5', PLAIN_KEY, 1, { DupeFiles: 'e240e0b1925d79b6-174', DupeHealth: '98.5', DupeCopyKind: 'twin', DupeKind: 'twin' }, 63),
    dupe(744, '_4', PLAIN_KEY, 3, { DupeHealth: '91', DupeCopyKind: 'borrowable-encrypted', DupeKind: 'container' }, 64),
    item(base, 740, PLAIN + '_0', PLAIN_KEY, { Status: 'FAILURE/HEALTH', DeleteStatus: 'NONE', ParStatus: 'NONE',
      DupeScore: 1, HistoryTime: now - 900, SuccessArticles: 300, FailedArticles: 700, Health: 300,
      Parameters: params({ DupeFiles: 'b3727db14b3e3a4b-197' }) }),
    dupe(53, '', FLEET_KEY, 50, {}, 70)
  ];
  list[list.length - 1].NZBName = list[list.length - 1].Name = list[list.length - 1].NZBNicename = FLEET + ' (2)';
  return list.concat(extra || []);
}

(async () => {
  const base = fixtureURL(process.argv[2] || 'http://127.0.0.1:17999/');
  const capture = process.env.NZBGET_UI_CAPTURE_DIR;
  const browser = await chromium.launch(process.env.NZBGET_CHROME_CHANNEL === 'none' ? {} : { channel: process.env.NZBGET_CHROME_CHANNEL || 'chrome' })
    .catch(() => chromium.launch());
  const failures = [];
  let cases = 0;
  try {
    for (const theme of ['light', 'dark']) for (const [width, height] of [[1440, 900], [390, 844]]) {
      const context = await browser.newContext({ viewport: { width, height } });
      await context.addInitScript(t => localStorage.setItem('Theme', t), theme);
      const page = await context.newPage();
      const errors = [], writes = [];
      const state = { extraHistory: [] };
      page.on('pageerror', e => errors.push(e.message));
      page.on('console', m => {
        if (m.type() === 'error' && !/\/favicon\.ico$/.test(m.location().url)) errors.push(m.text() + ' @' + m.location().url);
      });
      let queueBase = null;
      await installDetailsFixture(page, base, writes, {
        patchReply: (method, result) => {
          if (method === 'listgroups') { queueBase = queueBase || result[0]; return queue(result); }
          if (method === 'history') return history(result[0], state.extraHistory);
          return result;
        }
      });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      // the History reply arrives in the same refresh; the toggle shows once both are in
      await page.locator(`#DownloadsTable a.fleet-toggle[data-fleet-key="${PLAIN_KEY}"]`).waitFor({ timeout: 10000 });
      const tag = `${theme}-${width}`;
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${tag}/${label}: ${error.message}`); }
      };
      const shot = async name => {
        if (!capture) return;
        await page.waitForTimeout(400);
        await page.screenshot({ path: path.join(capture, `dupe-copies-${name}-${tag}.jpg`), type: 'jpeg', quality: 82, fullPage: true });
      };
      const toggle = key => page.locator(`#DownloadsTable a.fleet-toggle[data-fleet-key="${key}"]`);
      const rowSubs = key => page.locator(`#DownloadsTable tbody tr:has(a.fleet-toggle[data-fleet-key="${key}"]) .fleet-sub`);
      const sub = id => page.locator(`#DownloadsTable .fleet-sub[data-nzbid="${id}"]`);

      await check('a plain download with History backups gets the toggle', async () => {
        assert.match(await toggle(PLAIN_KEY).innerText(), /\+6 copies/);
        assert.equal(await toggle(PLAIN_KEY).getAttribute('aria-expanded'), 'false');
        assert.equal(await page.locator('#DownloadsTable tbody tr', { hasText: 'Example Feature' }).count(), 1, 'one row: the download');
        for (const id of [748, 747, 746, 745, 744, 740]) {
          assert.equal(await page.locator(`#DownloadsTable tbody a[data-nzbid="${id}"]`).count(), 0, 'History copy ' + id + ' not in Downloads');
        }
      });

      await check('queued siblings keep their rows, each with a toggle; none without a copy', async () => {
        assert.equal(await page.locator('#DownloadsTable tbody a.fleet-toggle[data-fleet-key="' + SIB_KEY + '"]').count(), 2);
        for (const id of [801, 802]) {
          assert.equal(await page.locator(`#DownloadsTable tbody tr a[data-nzbid="${id}"]:not(.fleet-sub-name)`).count(), 1, 'row ' + id + ' stays');
        }
        assert.match(await toggle(SIB_KEY).first().innerText(), /\+1 copy\b/);
        const lone = page.locator('#DownloadsTable tbody tr', { hasText: 'Lone Release' });
        assert.equal(await lone.count(), 1);
        assert.equal(await lone.locator('a.fleet-toggle').count(), 0, 'no toggle without another copy');
      });

      await shot('collapsed');

      await check('opened: the download, then backups in donor order, with kind, alive, borrowable and role', async () => {
        await toggle(PLAIN_KEY).click();
        await sub(745).waitFor();
        const list = rowSubs(PLAIN_KEY);
        const ids = await list.evaluateAll(els => els.map(e => Number(e.getAttribute('data-nzbid'))));
        // twin, borrowable (score 1, by ID), borrowable-encrypted, unborrowable; then the failed copy
        assert.deepEqual(ids, [741, 745, 747, 748, 744, 746, 740]);
        const pos = (await list.locator('.fleet-sub-pos').allInnerTexts()).map(t => t.toLowerCase());
        assert.deepEqual(pos, ['primary', 'backup', 'backup', 'backup', 'backup', 'backup', 'history']);
        assert.match(await sub(741).innerText(), /100\.0% alive · borrowable/);
        assert.match(await sub(741).innerText(), /downloading/i);
        assert.match(await sub(745).innerText(), /twin · 98\.5% alive/);
        assert.match(await sub(747).innerText(), /alt · 99\.0% alive · borrowable/);
        assert.match(await sub(748).innerText(), /container · 100\.0% alive · borrowable/);
        assert.match(await sub(744).innerText(), /container · 91\.0% alive · borrowable \(encrypted\)/);
        assert.match(await sub(746).innerText(), /other · dead \(0\/40 sampled\) · not borrowable/);
        for (const id of [745, 747, 748, 744, 746]) assert.match(await sub(id).innerText(), /backup in history/);
        assert.match(await sub(740).innerText(), /30\.0% alive/);
        assert.match(await sub(740).innerText(), /failure.* · history/i);
        assert.equal(await sub(745).getAttribute('data-fleet-source'), 'history');
        assert.equal(await toggle(PLAIN_KEY).getAttribute('aria-expanded'), 'true');
        await shot('expanded');
      });

      await check('a History copy opens its History panel', async () => {
        await sub(747).locator('a.fleet-sub-name').click();
        await page.locator('#HistoryEditDialog.in').waitFor({ timeout: 5000 });
        assert.match(await page.locator('#HistoryEditDialog').innerText(), /Example Feature|Example\.Feature/);
        await page.keyboard.press('Escape');
        await page.waitForFunction(() => !document.querySelector('#HistoryEditDialog.in'), null, { timeout: 5000 }).catch(async () => {
          await page.locator('#HistoryEditDialog [data-dismiss="modal"]').first().click();
          await page.waitForFunction(() => !document.querySelector('#HistoryEditDialog.in'));
        });
      });

      await check('a fleet lists its History copies after its queued members', async () => {
        assert.match(await toggle(FLEET_KEY).innerText(), /\+2 copies/);
        await toggle(FLEET_KEY).click();
        await sub(53).waitFor();
        const ids = await rowSubs(FLEET_KEY)
          .evaluateAll(els => els.map(e => Number(e.getAttribute('data-nzbid'))));
        assert.deepEqual(ids, [51, 52, 53]);
        const pos = (await rowSubs(FLEET_KEY).locator('.fleet-sub-pos').allInnerTexts())
          .map(t => t.toLowerCase());
        assert.deepEqual(pos, ['primary', 'next', 'backup']);
        assert.match(await sub(52).innerText(), /held donor/);
        assert.match(await sub(53).innerText(), /unverified/);
        await toggle(FLEET_KEY).click();
      });

      await check('a new History reply alone redraws the list', async () => {
        state.extraHistory = [item(queueBase || {}, 749, PLAIN + '_9', PLAIN_KEY, { Status: 'DELETED/DUPE', DeleteStatus: 'DUPE',
          DupeScore: 1, HistoryTime: Math.floor(Date.now() / 1000), Parameters: params({ DupeHealth: '95', DupeCopyKind: 'borrowable', DupeKind: 'alt' }) })];
        await page.evaluate(() => Refresher.update());
        await page.waitForFunction(k => /\+7 copies/.test((document.querySelector(`#DownloadsTable a.fleet-toggle[data-fleet-key="${k}"]`) || {}).textContent || ''),
          PLAIN_KEY, { timeout: 10000 });
        await sub(749).waitFor();
        state.extraHistory = [];
        await page.evaluate(() => Refresher.update());
        await page.waitForFunction(() => !document.querySelector('#DownloadsTable .fleet-sub[data-nzbid="749"]'), null, { timeout: 10000 });
      });

      await check('one History index per refresh: 110 downloads, 1400 History items stay fast', async () => {
        const ms = await page.evaluate(() => {
          const p = map => Object.entries(map).map(([Name, Value]) => ({ Name, Value }));
          const groups = [], hist = [];
          for (let i = 0; i < 110; i++) groups.push({ NZBID: i + 1, Kind: 'NZB', DupeKey: 'pair:k' + i + ':1', Parameters: p({ DupeHealth: '100' }) });
          for (let i = 0; i < 1400; i++) hist.push({ NZBID: 10000 + i, Kind: 'NZB', Status: i % 9 ? 'DELETED/DUPE' : 'FAILURE/HEALTH',
            DupeKey: 'PAIR:K' + (i % 120) + ':1', DupeScore: i % 5, HistoryTime: i, Parameters: p({ DupeCopyKind: 'borrowable', DupeKind: 'container' }) });
          let reads = 0;
          const tracked = hist.map(h => new Proxy(h, { get(t, k) { if (k === 'DupeKey') reads++; return t[k]; } }));
          const t0 = performance.now();
          let fleets;
          for (let r = 0; r < 10; r++) fleets = DownloadsUI.buildFleets(groups, tracked);
          const t = (performance.now() - t0) / 10;
          return { t, reads: reads / 10, listed: fleets.copiesById[1].members.length, rows: Object.keys(fleets.copiesById).length };
        });
        assert.equal(ms.reads, 1400, 'each History item read once per call: ' + ms.reads);
        assert.equal(ms.rows, 110);
        assert.equal(ms.listed, 13, 'the download plus its 12 History copies');
        assert(ms.t < 50, 'buildFleets took ' + ms.t.toFixed(1) + ' ms');
      });

      await check('no console errors', async () => assert.deepEqual(errors, []));
      await context.close();
    }
  } finally {
    await browser.close();
  }
  if (failures.length) {
    console.error(failures.join('\n'));
    console.error(`dupe-copies-downloads: ${failures.length} of ${cases} checks failed`);
    process.exit(1);
  }
  console.log(`dupe-copies-downloads: ${cases} checks passed`);
})();
