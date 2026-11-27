// History shows how much each item repaired, and how (the RepairStats parameter
// rust/src/repairsummary.rs keeps): a badge per row and a Repair section in the
// details panel. Offline (details fixture, no daemon), light and dark themes,
// desktop and phone widths. Set NZBGET_UI_CAPTURE_DIR to save JPEGs.
const assert = require('node:assert/strict');
const path = require('node:path');
const { chromium } = require('playwright');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

const MB = 1048576;
// name -> [RepairStats value, expected badge text, badge kind]
const CASES = [
  ['Example.Live.Repair', `v=1;live=12/12/37;par=0/0/0;readback=${3 * MB};twin=0;alt=0;unlabelled=0;payload=0;faildata=${9 * MB};failpar=0;sec=4;method=live`,
    'Repaired live · 12 slices', 'live'],
  ['Example.Post.Repair', `v=1;live=0/0/0;par=30/30/80;readback=0;twin=0;alt=0;unlabelled=0;payload=0;faildata=${22 * MB};failpar=0;sec=95;method=par`,
    'Repaired after download · 30 slices', 'par'],
  ['Example.Twin.Borrow', 'v=1;live=0/0/0;par=0/0/0;readback=0;twin=40;alt=0;unlabelled=0;payload=0;faildata=0;failpar=0;sec=0;method=twin',
    'Borrowed 40 articles (twin)', 'borrow'],
  ['Example.Alt.Borrow', 'v=1;live=0/0/0;par=0/0/0;readback=0;twin=0;alt=7;unlabelled=0;payload=0;faildata=0;failpar=0;sec=0;method=alt',
    'Borrowed 7 articles (alt)', 'borrow'],
  ['Example.Mixed.Repair', `v=1;live=5/5/20;par=0/0/0;readback=${MB};twin=8;alt=0;unlabelled=0;payload=0;faildata=${4 * MB};failpar=0;sec=2;method=live+twin`,
    'Repaired: live PAR + twin · 5 slices, 8 articles', 'mixed'],
  ['Example.Par2.Only', `v=1;live=0/0/0;par=0/0/0;readback=0;twin=0;alt=0;unlabelled=0;payload=0;faildata=0;failpar=${700 * MB};sec=0;method=par2only`,
    'Nothing to repair', 'none'],
  ['Example.Clean', 'v=1;live=0/0/0;par=0/0/0;readback=0;twin=0;alt=0;unlabelled=0;payload=0;faildata=0;failpar=0;sec=0;method=clean',
    'Not needed', 'none'],
  ['Example.Not.Repaired', `v=1;live=0/0/0;par=0/0/0;readback=0;twin=0;alt=0;unlabelled=0;payload=0;faildata=${5 * MB};failpar=0;sec=0;method=unrepaired`,
    'Not repaired', 'failed'],
  // still queued when written (no method): no badge; injected markup stays text
  ['Example.Unsettled', 'v=1;live=1/1/1;par=0/0/0;readback=0;twin=0;alt=0;unlabelled=0', null, null],
  // an unknown method name is no record; the markup never renders
  ['Example.Hostile', 'method=<img src=x onerror=alert(1)>live;live=1/1/1', null, null],
  ['Example.Hostile.Mixed', 'method=live+<b>x</b>+twin+live;live=2/2/4;twin=1', 'Repaired: live PAR + twin · 2 slices, 1 articles', 'mixed'],
];

function withRepair(result) {
  const template = result[result.length - 1];
  const now = Math.floor(Date.now() / 1000);
  const added = CASES.map(([name, value], i) => {
    const id = 900 + i;
    return { ...template, NZBID: id, ID: id, NZBName: name, NZBNicename: name, Name: name, NZBFilename: name + '.nzb',
      DupeKey: '', Status: 'SUCCESS/ALL', HistoryTime: now - 60 * (i + 1),
      Parameters: [{ Name: '*Unpack:', Value: 'yes' }, { Name: 'RepairStats', Value: value }] };
  });
  return added.concat(result);
}

(async () => {
  const base = fixtureURL(process.argv[2] || 'http://127.0.0.1:17999/');
  const capture = process.env.NZBGET_UI_CAPTURE_DIR;
  const browser = await chromium.launch({ channel: 'chrome' });
  const failures = [];
  let cases = 0;
  try {
    for (const theme of ['light', 'dark']) for (const [width, height] of [[1440, 900], [390, 844]]) {
      const context = await browser.newContext({ viewport: { width, height } });
      await context.addInitScript(t => localStorage.setItem('Theme', t), theme);
      // every case on one page
      await context.addInitScript(() => localStorage.setItem('HistoryRecordsPerPage', '50'));
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', e => errors.push(e.message));
      page.on('dialog', d => { errors.push('dialog: ' + d.message()); d.dismiss(); });
      await installDetailsFixture(page, base, [], {
        patchReply: (method, result) => method === 'history' ? withRepair(result) : result
      });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      await page.locator('#HistoryTabLink').click();
      await page.locator('#HistoryTable tbody tr', { hasText: 'Example Live Repair' }).first().waitFor();
      const tag = `${theme}-${width}`;
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${tag}/${label}: ${error.message}`); }
      };
      // the list shows names with spaces for dots
      const row = name => page.locator('#HistoryTable tbody tr', { hasText: name.replace(/\./g, ' ') }).first();

      await check('each row has its badge', async () => {
        for (const [name, , text, kind] of CASES) {
          const badge = row(name).locator('.repair-badge');
          if (text === null) {
            assert.equal(await badge.count(), 0, name + ' has no badge before it settled');
            continue;
          }
          assert.equal((await badge.textContent()).trim(), text, name);
          assert.match(await badge.getAttribute('class'), new RegExp('repair-badge--' + kind + '\\b'), name);
        }
        assert.equal(await row('Example.Completed.Release').locator('.repair-badge').count(), 0, 'older entries have none');
        assert.equal(await page.locator('#HistoryTable img').count(), 0, 'parameter text never becomes markup');
        assert.match(await row('Example.Par2.Only').locator('.repair-badge').getAttribute('title'), /only in par2/);
      });

      await check('search finds rows by repair method and amount', async () => {
        const search = page.locator('#HistoryTable_filter');
        const rows = () => page.locator('#HistoryTable tbody tr').count();
        const all = await rows();
        await search.fill('repair:twin');
        await search.press('Enter');
        await page.waitForFunction(n => document.querySelectorAll('#HistoryTable tbody tr').length < n, all, { timeout: 5000 });
        const names = await page.locator('#HistoryTable tbody tr a[data-nzbid]').allTextContents();
        assert(names.some(n => /Twin Borrow/.test(n)) && names.some(n => /Mixed Repair/.test(n)), names.join(','));
        assert(!names.some(n => /Live Repair|Clean/.test(n)), names.join(','));
        await search.fill('repaired>20');
        await search.press('Enter');
        await page.waitForFunction(() => document.querySelectorAll('#HistoryTable tbody tr').length === 2, null, { timeout: 5000 });
        const big = await page.locator('#HistoryTable tbody tr a[data-nzbid]').allTextContents();
        assert.deepEqual(big.map(n => n.trim()).sort(), ['Example Post Repair', 'Example Twin Borrow']);
        await search.fill('');
        await search.press('Enter');
        await page.waitForFunction(n => document.querySelectorAll('#HistoryTable tbody tr').length === n, all, { timeout: 5000 });
      });

      if (capture) {
        await page.evaluate(() => document.fonts.ready);
        await page.screenshot({ path: path.join(capture, `history-repair-list-${tag}.jpg`), type: 'jpeg', quality: 88 });
      }

      await check('the details panel explains the repair', async () => {
        await row('Example.Mixed.Repair').locator('a[data-nzbid]').first().click();
        await page.locator('#HistoryEditDialog').waitFor({ state: 'visible' });
        const section = page.locator('#HistoryEdit_Repair');
        await section.waitFor({ state: 'visible' });
        const text = await section.innerText();
        assert.match(text, /REPAIR/i);
        assert.match(text, /Failed articles\s*4(\.0+)? MB in data files · 0(\.0+)? MB in par2 files/);
        assert.match(text, /Live PAR while downloading\s*5 slices · 5 of 20 recovery blocks used/);
        assert.match(text, /Borrowed from duplicates\s*8 articles \(twin\)/);
        assert.match(text, /Read back from disk\s*1(\.0+)? MB/);
        const fill = await section.locator('.repair-bar__fill').getAttribute('style');
        assert.match(fill, /width:\s*25%/, 'the bar shows 5 of 20 blocks');
        // the side panel slides in: measure once it has settled
        await page.waitForFunction(() => {
          const d = document.querySelector('#HistoryEditDialog').getBoundingClientRect();
          return d.right <= window.innerWidth + 1 && d.left >= -1;
        }, null, { timeout: 5000 });
        const box = await section.boundingBox(), panel = await page.locator('#HistoryEditDialog').boundingBox();
        assert(box && box.x >= panel.x - 1 && box.x + box.width <= panel.x + panel.width + 1,
          `the section fits the panel: ${JSON.stringify(box)} in ${JSON.stringify(panel)}`);
        if (capture) {
          await page.screenshot({ path: path.join(capture, `history-repair-details-${tag}.jpg`), type: 'jpeg', quality: 88 });
        }
        await page.keyboard.press('Escape');
        await page.locator('#HistoryEditDialog').waitFor({ state: 'hidden' });
        await row('Example.Par2.Only').locator('a[data-nzbid]').first().click();
        await page.locator('#HistoryEditDialog').waitFor({ state: 'visible' });
        assert.match(await section.innerText(), /Nothing to repair[\s\S]*0(\.0+)? MB in data files · 700(\.0+)? MB in par2 files/);
        await page.keyboard.press('Escape');
        await page.locator('#HistoryEditDialog').waitFor({ state: 'hidden' });
        await row('Example.Completed.Release').locator('a[data-nzbid]').first().click();
        await page.locator('#HistoryEditDialog').waitFor({ state: 'visible' });
        assert.equal(await section.isVisible(), false, 'no section without a record');
        await page.keyboard.press('Escape');
        await page.locator('#HistoryEditDialog').waitFor({ state: 'hidden' });
      });

      await check('no page errors', async () => assert.deepEqual(errors, []));
      await context.close();
    }
  } finally { await browser.close(); }
  if (failures.length) {
    console.error(failures.join('\n'));
    process.exitCode = 1;
  }
  process.stdout.write(`history-repair: ${cases - failures.length} of ${cases} cases passed\n`);
})().catch(error => { console.error(error); process.exitCode = 1; });
