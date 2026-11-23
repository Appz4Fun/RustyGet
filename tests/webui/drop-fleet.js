// Copies of one release added together are one fleet (docs/DROP_AS_FLEET.md):
// the Add dialog's grouping, the Downloads fleet and its labels with honest
// health, and History's labelled donors. Offline (details-fixture): committed
// assets, real startup and renderers, synthetic RPC replies; writes intercepted.
// Usage: node tests/webui/drop-fleet.js http://127.0.0.1:17999/
// Optional screenshots: NZBGET_UI_CAPTURE_DIR=/some/dir
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

const REL = 'Drop.Movie.2024.1080p.WEB-DL.x264-GRP';
const KEY = 'dupes:drop.movie.2024.1080p.web.dl.x264.grp';
const OTHER = 'Drop.Show.S01E02.720p.HDTV.x264-GRP';
const OTHER_KEY = 'dupes:drop.show.s01e02.720p.hdtv.x264.grp';
const MB = 1048576;

function params(map) {
  return Object.entries(map).map(([Name, Value]) => ({ Name, Value }));
}

function item(base, id, name, extra) {
  const copy = JSON.parse(JSON.stringify(base));
  return Object.assign(copy, {
    NZBID: id, ID: id, FirstID: id * 10, LastID: id * 10 + 9, NZBName: name, NZBNicename: name, Name: name,
    NZBFilename: name + '.nzb', DupeKey: KEY, DupeMode: 'SCORE', Category: 'Movies', MessageCount: 0
  }, extra);
}

// the fleet: a primary downloading, a copy of another release judged on a sample
// (projected 3% while its Health counts untried articles as good), and four
// backups in history: a twin, a borrowable alt 48% alive, another container
// (unverified) and a dead copy kept as a donor
function fleetQueue(groups, primaryDone) {
  const base = groups[0];
  const q = [];
  if (!primaryDone) {
    q.push(item(base, 51, REL, {
      Status: 'DOWNLOADING', ActiveDownloads: 4, DupeScore: 100, Health: 1000, CriticalHealth: 950,
      Parameters: params({ DupeFiles: 'a3f91c02d4e8b711-30', DupeCopyKind: 'twin', DupeFleet: '1', DupeAlive: '93' })
    }));
  }
  q.push(item(base, 55, OTHER, {
    DupeKey: OTHER_KEY, Status: 'DOWNLOADING', ActiveDownloads: 2, DupeScore: 100, Health: 988, CriticalHealth: 950,
    TotalArticles: 95000, FailedArticles: 194, SuccessArticles: 6,
    Parameters: params({ DupeFleet: '1', DupeProjected: '3.0' })
  }));
  return q;
}

function fleetHistory(history, primaryDone) {
  const base = history[0];
  const now = Math.floor(Date.now() / 1000);
  const dupe = { Status: 'DELETED/DUPE', DeleteStatus: 'DUPE', ParStatus: 'NONE', UnpackStatus: 'NONE', HistoryTime: now - 60,
    SuccessArticles: 0, FailedArticles: 0, DupeRecoveredArticles: 0 };
  const h = [
    item(base, 61, REL + ' (1)', Object.assign({}, dupe, { DupeScore: 99, Health: 1000,
      Parameters: params({ DupeFiles: 'a3f91c02d4e8b711-30', DupeKind: 'twin', DupeCopyKind: 'twin', DupeAlive: '97', DupeFleet: '2' }) })),
    item(base, 62, REL + '_1', Object.assign({}, dupe, { DupeScore: 98, Health: 1000,
      Parameters: params({ DupeFiles: 'c07be15a99d2f310-36', DupeKind: 'alt', DupeCopyKind: 'borrowable', DupeAlive: '48', DupeFleet: '3' }) })),
    item(base, 63, REL + '_2', Object.assign({}, dupe, { DupeScore: 97, Health: 1000,
      Parameters: params({ DupeFiles: 'd11be15a99d2f310-36', DupeKind: 'container', DupeCopyKind: 'unknown', DupeAlive: '65', DupeFleet: '4' }) })),
    // m5air row 41: DupeHealth 17 with DupeAlive 0 and a Health of 100%: a donor 17% alive
    item(base, 65, REL + '_4', Object.assign({}, dupe, { DupeScore: 95, Health: 1000,
      Parameters: params({ DupeFleet: '6', DupeCopyKind: 'unknown', DupeFiles: 'lost', DupeSampled: 'none',
        DupeHealth: '17', DupeAlive: '0' }) })),
    // tried, parked on a sample (Health 99%, nothing arrived): kept as a donor
    item(base, 64, REL + '_3', Object.assign({}, dupe, { DupeScore: 96, Health: 990, FailedArticles: 200,
      Parameters: params({ DupeParked: '0.0', DupeAlive: '0.0', DupeProjected: '0.0', DupeFleet: '5' }) }))
  ];
  if (primaryDone) {
    h.unshift(item(base, 51, REL, { Status: 'SUCCESS/ALL', DeleteStatus: 'NONE', ParStatus: 'SUCCESS', UnpackStatus: 'SUCCESS',
      DupeScore: 100, Health: 1000, HistoryTime: now - 10, DupeRecoveredArticles: 2,
      Parameters: params({ DupeFiles: 'a3f91c02d4e8b711-30', DupeFleet: '1' }) }));
  }
  return h;
}

function nzbFile(name, seed) {
  return { name, mimeType: 'application/x-nzb', buffer: Buffer.from(
    `<?xml version="1.0" encoding="UTF-8"?>\n<nzb xmlns="http://www.newzbin.com/DTD/2003/nzb"><file poster="p" date="1" subject="${seed}"></file></nzb>\n`) };
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
      const state = { primaryDone: false };
      page.on('pageerror', e => errors.push(e.message));
      page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
      await installDetailsFixture(page, base, writes, {
        answer: rpc => rpc.method === 'appendfleet' ? { Chosen: 1, Complete: true, Reason: '',
          Members: rpc.params.slice(4).filter((_, i) => i % 2 === 0).map((Name, i) => ({ NZBID: i + 1, Name, Rank: i + 1, Alive: 100,
            Status: i === 0 ? 'QUEUED' : 'BACKUP', SameAs: 0, SameAsRank: 0, Reason: '' })) } : undefined,
        patchReply: (method, result) => {
          if (method === 'listgroups') return fleetQueue(result, state.primaryDone);
          if (method === 'history') return fleetHistory(result, state.primaryDone);
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
        if (capture) await page.waitForTimeout(500);
        if (capture) await page.screenshot({ path: path.join(capture, `drop-fleet-${name}-${tag}.jpg`), type: 'jpeg', quality: 80 });
      };

      const files = [
        nzbFile(`${REL}.nzb`, 'a'), nzbFile(`${REL} (1).nzb`, 'b'), nzbFile(`${REL}_1.nzb`, 'c'),
        nzbFile(`${REL}_2.nzb`, 'a'), // the same content as the first: dropped before upload
        nzbFile(`${OTHER}.nzb`, 'd'), nzbFile(`${OTHER}_1.nzb`, 'e'),
        nzbFile('Lone.Documentary.2023.2160p.WEB-DL-NOGRP.nzb', 'f')
      ];
      const openWith = async () => {
        await page.evaluate(() => Upload.addClick());
        await page.locator('#AddDialog').waitFor({ state: 'visible' });
        await page.setInputFiles('#AddDialog_Input', files);
        await page.locator('#AddDialog_Files table').nth(files.length - 1).waitFor();
      };
      const submit = async () => {
        const before = writes.length;
        await page.locator('#AddDialog_Submit').click();
        await page.locator('#AddDialog').waitFor({ state: 'hidden', timeout: 15000 });
        return writes.slice(before);
      };

      await check('add dialog groups copies, box on by default', async () => {
        await openWith();
        assert.equal(await page.locator('#AddDialog_Fleet').isChecked(), true, 'the fleet box is on by default');
        assert.equal(await page.locator('#AddDialog_FleetRow').isVisible(), true);
        const plan = await page.locator('#AddDialog_FleetPlan .add-dialog-fleet').allInnerTexts();
        assert.equal(plan.length, 2, 'two releases make two fleets: ' + plan);
        assert.match(plan[0], new RegExp('^4 files → 1 fleet: ' + REL.replace(/\./g, '\\.') + '$'));
        assert.match(plan[1], /^2 files → 1 fleet: Drop\.Show/);
        await shot('add-dialog');
        const sent = await submit();
        const fleets = sent.filter(w => w.method === 'appendfleet');
        assert.equal(fleets.length, 2, 'one appendfleet per release');
        assert.equal(fleets[0].params[0], KEY);
        // exact copy dropped before upload: 3 members of the first release
        assert.deepEqual(fleets[0].params.slice(4).filter((_, i) => i % 2 === 0),
          [`${REL}.nzb`, `${REL} (1).nzb`, `${REL}_1.nzb`]);
        assert.equal(fleets[1].params[0], OTHER_KEY);
        const appends = sent.filter(w => w.method === 'append');
        assert.deepEqual(appends.map(w => w.params[0]), ['Lone.Documentary.2023.2160p.WEB-DL-NOGRP.nzb'], 'a single file is a plain append');
      });

      await check('13 copies: the dialog closes within 3 s', async () => {
        await page.evaluate(() => Upload.addClick());
        await page.locator('#AddDialog').waitFor({ state: 'visible' });
        const many = Array.from({ length: 13 }, (_, i) => nzbFile(`${REL}${i ? '_' + i : ''}.nzb`, 'm' + i));
        await page.setInputFiles('#AddDialog_Input', many);
        await page.locator('#AddDialog_Files table').nth(12).waitFor();
        assert.match(await page.locator('#AddDialog_FleetPlan').innerText(), /^13 files → 1 fleet/);
        const start = Date.now();
        await page.locator('#AddDialog_Submit').click();
        await page.locator('#AddDialog').waitFor({ state: 'hidden', timeout: 3000 });
        assert(Date.now() - start < 3000, 'closed in ' + (Date.now() - start) + ' ms');
      });

      await check('Add paused: plain appends, paused', async () => {
        await openWith();
        await page.locator('#AddDialog_Paused').check();
        assert.equal(await page.locator('#AddDialog_FleetPlan').isVisible(), false);
        const sent = await submit();
        assert.equal(sent.filter(w => w.method === 'appendfleet').length, 0);
        assert(sent.filter(w => w.method === 'append').every(w => w.params[5] === true));
      });

      await check('archives are not grouped', async () => {
        await page.evaluate(() => Upload.addClick());
        await page.locator('#AddDialog').waitFor({ state: 'visible' });
        await page.setInputFiles('#AddDialog_Input', [
          { name: `${REL}.zip`, mimeType: 'application/zip', buffer: Buffer.from('PK a') },
          { name: `${REL}_1.zip`, mimeType: 'application/zip', buffer: Buffer.from('PK b') }]);
        await page.locator('#AddDialog_Files table').nth(1).waitFor();
        assert.equal(await page.locator('#AddDialog_FleetPlan').isVisible(), false);
        const sent = await submit();
        assert.equal(sent.filter(w => w.method === 'appendfleet').length, 0);
        assert.equal(sent.filter(w => w.method === 'append').length, 2);
      });

      await check('box off: plain appends, kept out of the daemon join', async () => {
        await openWith();
        await page.locator('#AddDialog_Fleet').uncheck();
        assert.equal(await page.locator('#AddDialog_FleetPlan').isVisible(), false);
        const sent = await submit();
        assert.equal(sent.filter(w => w.method === 'appendfleet').length, 0);
        const appends = sent.filter(w => w.method === 'append');
        assert.equal(appends.length, files.length, 'every file on its own, the exact copy too');
        assert(appends.every(w => w.params[8] === 'SCORE' && JSON.stringify(w.params[10]).includes('DupeJoin')));
      });

      await check('Force: plain appends in FORCE mode', async () => {
        await openWith();
        await page.locator('#AddDialog_DupeForce').check();
        assert.equal(await page.locator('#AddDialog_FleetPlan').isVisible(), false);
        const sent = await submit();
        assert.equal(sent.filter(w => w.method === 'appendfleet').length, 0);
        const appends = sent.filter(w => w.method === 'append');
        assert.equal(appends.length, files.length);
        assert(appends.every(w => w.params[8] === 'FORCE' && !JSON.stringify(w.params[10]).includes('DupeJoin')));
      });

      await check('downloads: honest health', async () => {
        await page.evaluate(() => Refresher.update());
        const row = page.locator('#DownloadsTable tbody tr', { hasText: 'Drop Show' }).first();
        await row.waitFor();
        const text = await row.innerText();
        assert.match(text, /projected health: 3\.0%/);
        assert.doesNotMatch(text, /health: 98/);
        await shot('downloads');
      });

      await check('downloads: one primary and labelled backups', async () => {
        await page.locator('#DownloadsTable tbody a[data-nzbid="51"]').first().click();
        const panel = page.locator('#DownloadsEditDialog');
        await panel.waitFor({ state: 'visible' });
        await page.locator('#DownloadsEdit_Fleet .fleet-member').first().waitFor();
        const roles = (await page.locator('#DownloadsEdit_Fleet .fleet-member .fleet-role').allInnerTexts()).map(r => r.toLowerCase());
        assert.equal(roles.filter(r => r.startsWith('primary')).length, 1, 'one primary: ' + roles);
        assert(roles.some(r => r.startsWith('twin')), 'a twin: ' + roles);
        assert(roles.some(r => r.startsWith('alt')), 'an alt: ' + roles);
        assert(roles.some(r => r.startsWith('unverified')), 'another container shows unverified: ' + roles);
        const members = await page.locator('#DownloadsEdit_Fleet .fleet-members').innerText();
        assert.match(members, /borrowable/i);
        assert.match(members, /48/);
        // the dead donor shows the health it was judged on, not its 99% Health
        const dead = page.locator('#DownloadsEdit_Fleet .fleet-member', { hasText: 'GRP 3' });
        assert.match(await dead.locator('.fleet-member-health').innerText(), /^0(\.0)?%$/);
        await shot('fleet');
        await page.keyboard.press('Escape');
        await panel.waitFor({ state: 'hidden' });
      });

      const historyRows = async () => {
        await page.locator('#HistoryTabLink').click();
        await page.locator('#HistoryTable tbody tr', { hasText: 'Drop Movie' }).first().waitFor();
        const rows = {};
        for (const row of await page.locator('#HistoryTable tbody tr').all()) {
          const text = await row.innerText();
          const id = await row.locator('a[data-nzbid]').first().getAttribute('data-nzbid').catch(() => null);
          if (id) rows[id] = text;
        }
        return rows;
      };

      await check('history: donors labelled, never FAILURE/HEALTH', async () => {
        const rows = await historyRows();
        for (const id of ['61', '62', '63', '64', '65']) {
          assert(rows[id], 'history row ' + id);
          assert.doesNotMatch(rows[id], /failure/i, 'row ' + id + ' is no failure: ' + rows[id]);
        }
        assert.match(rows['61'], /twin/i);
        assert.match(rows['62'], /alt/i);
        assert.match(rows['62'], /donor · 48\.0% alive/);
        assert.match(rows['63'], /unverified/i);
        assert.match(rows['64'], /dead donor/);
        assert.match(rows['65'], /donor · 17\.0% alive/);
        assert.doesNotMatch(rows['65'], /dead/);
        // the pill shows the place in the fleet and the measured alive share, never 100% beside dead
        assert.match(rows['65'], /#6 · 17\.0% alive/);
        assert.doesNotMatch(rows['64'], /100\.0%/);
        await shot('history');
      });

      await check('history: the list and the detail show the same health', async () => {
        const row = page.locator('#HistoryTable tbody tr', { hasText: 'GRP 4' }).first();
        await row.locator('a[data-nzbid]').first().click();
        await page.locator('#HistoryEditDialog').waitFor({ state: 'visible' });
        const tile = await page.locator('#HistoryEdit_Summary').innerText();
        assert.match(tile, /17(\.0)?%/, 'the detail shows the probed 17%: ' + tile);
        assert.doesNotMatch(tile, /HEALTH\s*100%/i);
        await page.keyboard.press('Escape');
        await page.locator('#HistoryEditDialog').waitFor({ state: 'hidden' });
      });

      await check('history: after the primary succeeds the others stay DELETED/DUPE', async () => {
        state.primaryDone = true;
        await page.evaluate(() => Refresher.update());
        await page.locator('#HistoryTable tbody a[data-nzbid="51"]').first().waitFor({ timeout: 10000 });
        const rows = await historyRows();
        assert.match(rows['51'], /success/i);
        for (const id of ['61', '62', '63', '64']) assert.doesNotMatch(rows[id], /failure/i);
        assert.equal(await page.locator('#DownloadsTable tbody a[data-nzbid="51"]').count(), 0, 'nothing downloads again');
        await shot('history-done');
      });

      await check('no console errors', async () => assert.deepEqual(errors, []));
      await context.close();
    }
  } finally {
    await browser.close();
  }
  if (failures.length) {
    console.error(failures.join('\n'));
    console.error(`drop-fleet: ${failures.length} of ${cases} checks failed`);
    process.exit(1);
  }
  console.log(`drop-fleet: ${cases} checks passed`);
})();
