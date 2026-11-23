// F16: a fleet stays in Downloads (docs/DROP_AS_FLEET.md "Fleets stay in
// Downloads"). One row per fleet (its primary) with a "+N copies" toggle,
// collapsed by default and remembered per viewer; opened, every copy in the
// daemon's failover order (DupeFleetOrder, never re-ranked here) with its
// label and state; copies show "probing…" right after they are added; no
// copy of an open fleet is in History. Offline (details-fixture): committed
// assets, real startup and renderers, synthetic RPC replies.
// Usage: node tests/webui/fleet-downloads.js http://127.0.0.1:17999/
// Optional screenshots: NZBGET_UI_CAPTURE_DIR=/some/dir
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

const REL = 'Fleet.Movie.2024.1080p.WEB-DL.x264-GRP';
const KEY = 'dupes:fleet.movie.2024.1080p.web.dl.x264.grp';
const NEW_REL = 'Probe.Movie.2025.2160p.WEB-DL.x265-GRP';
const NEW_KEY = 'dupes:probe.movie.2025.2160p.web.dl.x265.grp';

function params(map) {
  return Object.entries(map).map(([Name, Value]) => ({ Name, Value }));
}

function item(base, id, name, key, extra) {
  const copy = JSON.parse(JSON.stringify(base));
  return Object.assign(copy, {
    NZBID: id, ID: id, FirstID: id * 10, LastID: id * 10 + 9, NZBName: name, NZBNicename: name, Name: name,
    NZBFilename: name + '.nzb', DupeKey: key, DupeMode: 'SCORE', Category: 'Movies', MessageCount: 0,
    SuccessArticles: 0, FailedArticles: 0, Health: 1000, CriticalHealth: 950
  }, extra);
}

// the daemon's failover order for the open fleet: id -> DupeFleetOrder
const ORDER = { 51: 0, 54: 1, 52: 2, 55: 3, 53: 4 };

// the open fleet, in a shuffled list order, with scores and alive shares that
// would rank otherwise: the web UI must follow DupeFleetOrder only
function fleetQueue(groups, phase) {
  const base = groups[0];
  const held = { Status: 'PAUSED', MaxPriority: -100, MinPriority: -100 };
  const q = [
    item(base, 53, REL + '_3', KEY, Object.assign({}, held, { DupeScore: 99,
      Parameters: params({ DupeHeld: 'yes', DupeFleetOrder: '4', DupeFleetPrimary: '51', DupeAlive: '0', DupeFleet: '5' }) })),
    item(base, 52, REL + '_1', KEY, Object.assign({}, held, { DupeScore: 98,
      Parameters: params({ DupeHeld: 'yes', DupeFleetOrder: '2', DupeFleetPrimary: '51', DupeKind: 'alt',
        DupeCopyKind: 'borrowable', DupeAlive: '97', DupeFleet: '3' }) })),
    item(base, 51, REL, KEY, { Status: 'DOWNLOADING', ActiveDownloads: 4, DupeScore: 100,
      Parameters: params({ DupeFleetOrder: '0', DupeFleetPrimary: '51', DupeCopyKind: 'twin', DupeAlive: '99', DupeFleet: '1' }) }),
    item(base, 55, REL + '_4', KEY, Object.assign({}, held, { DupeScore: 97,
      Parameters: params({ DupeHeld: 'yes', DupeFleetOrder: '3', DupeFleetPrimary: '51', DupeKind: 'container', DupeFleet: '4' }) })),
    item(base, 54, REL + ' (1)', KEY, Object.assign({}, held, { DupeScore: 96,
      Parameters: params({ DupeHeld: 'yes', DupeFleetOrder: '1', DupeFleetPrimary: '51', DupeKind: 'twin',
        DupeCopyKind: 'twin', DupeAlive: '98.5', DupeFleet: '2' }) }))
  ];
  if (phase === 'probing') {
    // copies just added: held and probing, no order yet
    for (const [i, suffix] of ['', '_1', ' (2)'].entries()) {
      q.unshift(item(base, 70 + i, NEW_REL + suffix, NEW_KEY, Object.assign({}, held, { DupeScore: 100 - i,
        Parameters: params({ DupeHeld: 'yes', DupeProbing: 'yes' }) })));
    }
  }
  return q;
}

// History holds only an unrelated download while the fleet is open
function fleetHistory(history) {
  const base = history[0];
  return [item(base, 90, 'Other.Show.S01E01.720p.HDTV.x264-OTHER', 'other-key',
    { Status: 'SUCCESS/ALL', DeleteStatus: 'NONE', ParStatus: 'SUCCESS', UnpackStatus: 'SUCCESS',
      HistoryTime: Math.floor(Date.now() / 1000) - 30, Parameters: [] })];
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
      const state = { phase: 'open' };
      page.on('pageerror', e => errors.push(e.message));
      // a reload asks the loopback port for the favicon outside the fixture's
      // routes; that request is the fixture's, not the web UI's
      page.on('console', m => {
        if (m.type() === 'error' && !/\/favicon\.ico$/.test(m.location().url)) errors.push(m.text() + ' @' + m.location().url);
      });
      await installDetailsFixture(page, base, writes, {
        patchReply: (method, result) => {
          if (method === 'listgroups') return fleetQueue(result, state.phase);
          if (method === 'history') return fleetHistory(result);
          return result;
        }
      });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      const tag = `${theme}-${width}`;
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${tag}/${label}: ${error.message}`); }
      };
      const shot = async name => {
        if (!capture) return;
        await page.waitForTimeout(400);
        await page.screenshot({ path: path.join(capture, `fleet-downloads-${name}-${tag}.jpg`), type: 'jpeg', quality: 82 });
      };
      const fleetRows = () => page.locator('#DownloadsTable tbody tr', { hasText: 'Fleet Movie' });
      const toggle = () => page.locator(`#DownloadsTable a.fleet-toggle[data-fleet-key="${KEY}"]`);

      await check('one row per fleet, collapsed by default', async () => {
        assert.equal(await fleetRows().count(), 1, 'one row for the fleet');
        assert.equal(await page.locator('#DownloadsTable tbody a[data-nzbid="51"]').count(), 1, 'the primary leads the row');
        for (const id of [52, 53, 54, 55]) {
          assert.equal(await page.locator(`#DownloadsTable tbody a[data-nzbid="${id}"]`).count(), 0, 'copy ' + id + ' is folded');
        }
        assert.match(await toggle().innerText(), /\+4 copies/);
        assert.equal(await toggle().getAttribute('aria-expanded'), 'false');
        assert.equal(await page.locator('#DownloadsTable .fleet-sublist').count(), 0);
        await shot('collapsed');
      });

      await check('expanded: every member in failover order with label and state', async () => {
        await toggle().click();
        await page.locator('#DownloadsTable .fleet-sublist .fleet-sub').first().waitFor();
        const ids = await page.locator('#DownloadsTable .fleet-sublist .fleet-sub').evaluateAll(
          els => els.map(e => Number(e.getAttribute('data-nzbid'))));
        const want = Object.keys(ORDER).map(Number).sort((a, b) => ORDER[a] - ORDER[b]);
        assert.deepEqual(ids, want, 'the daemon order, never re-ranked');
        const pos = (await page.locator('#DownloadsTable .fleet-sublist .fleet-sub-pos').allInnerTexts()).map(t => t.toLowerCase());
        assert.deepEqual(pos, ['primary', 'next', '2', '3', '4']);
        const text = id => page.locator(`#DownloadsTable .fleet-sub[data-nzbid="${id}"]`).innerText();
        assert.match(await text(51), /downloading/i);
        assert.match(await text(54), /twin · 98\.5% alive/i);
        assert.match(await text(54), /held donor/i);
        assert.match(await text(52), /alt · 97\.0% alive/i);
        assert.match(await text(55), /container · unverified/i);
        assert.match(await text(53), /dead/i);
        assert.equal(await toggle().getAttribute('aria-expanded'), 'true');
        await shot('expanded');
      });

      await check('the open state is remembered per viewer', async () => {
        await page.reload();
        await page.locator('#DownloadsTable .fleet-sublist .fleet-sub').first().waitFor();
        await toggle().click();
        await page.waitForFunction(() => !document.querySelector('#DownloadsTable .fleet-sublist'));
        await page.reload();
        await page.locator('#DownloadsTable tbody tr').first().waitFor();
        assert.equal(await page.locator('#DownloadsTable .fleet-sublist').count(), 0, 'closed again after a reload');
        const stored = await page.evaluate(() => localStorage.getItem('DownloadsFleetOpen'));
        assert.equal(JSON.parse(stored || '{}')[KEY], undefined);
      });

      await check('copies show as probing right after they are added', async () => {
        state.phase = 'probing';
        await page.evaluate(() => Refresher.update());
        const newToggle = page.locator(`#DownloadsTable a.fleet-toggle[data-fleet-key="${NEW_KEY}"]`);
        await newToggle.waitFor({ timeout: 10000 });
        assert.equal(await page.locator('#DownloadsTable tbody tr', { hasText: 'Probe Movie' }).count(), 1);
        assert.match(await newToggle.innerText(), /\+2 copies · probing…/);
        await newToggle.click();
        await page.locator('#DownloadsTable .fleet-sub[data-nzbid="71"]').waitFor();
        for (const id of [70, 71, 72]) {
          assert.match(await page.locator(`#DownloadsTable .fleet-sub[data-nzbid="${id}"]`).innerText(), /probing…/);
        }
        await shot('probing');
        await newToggle.click();
      });

      await check('no fleet member in History while the fleet is open', async () => {
        await page.locator('#HistoryTabLink').click();
        await page.locator('#HistoryTable tbody tr', { hasText: 'Other Show' }).first().waitFor();
        const names = await page.locator('#HistoryTable tbody tr').allInnerTexts();
        assert(!names.some(n => /Fleet Movie|Probe Movie/.test(n)), 'history: ' + names.join(' | '));
        await shot('history');
        await page.locator('#DownloadsTabLink').click();
      });

      await check('no console errors', async () => assert.deepEqual(errors, []));
      await context.close();
    }
  } finally {
    await browser.close();
  }
  if (failures.length) {
    console.error(failures.join('\n'));
    console.error(`fleet-downloads: ${failures.length} of ${cases} checks failed`);
    process.exit(1);
  }
  console.log(`fleet-downloads: ${cases} checks passed`);
})();
