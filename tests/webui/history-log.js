// History details > Log: every finished download's messages are readable, for
// SUCCESS and FAILURE alike. The tab asks the daemon for the whole log
// (loadlog id, 0, 10000), shows the newest first, pages through it, keeps the
// text inside the panel with readable contrast, and reaches its last row. A
// download whose log was not kept says so instead of "yet". Offline (details
// fixture, no daemon), light and dark themes, desktop and phone widths.
// Set NZBGET_UI_CAPTURE_DIR to save JPEGs.
const assert = require('node:assert/strict');
const path = require('node:path');
const { chromium } = require('playwright');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

const NOW = Math.floor(Date.now() / 1000);
function log(name, count, failed) {
  const out = [];
  for (let i = 0; i < count; i++) {
    let kind = 'INFO', text = `Example message ${i + 1} of ${count} for ${name}`;
    if (i === 0) text = `Collection ${name} added to queue`;
    if (i === 5) { kind = 'DETAIL'; text = `Fingerprint of ${name}: a long line that has to wrap inside the panel ` + 'without a horizontal scrollbar '.repeat(6); }
    if (failed && i === count - 3) { kind = 'WARNING'; text = `Par-check for ${name}: 74 blocks short, more recovery data is needed`; }
    if (failed && i === count - 2) { kind = 'ERROR'; text = `Par-check failed for ${name}: not enough recovery blocks`; }
    if (i === count - 1) text = `Collection ${name} added to history`;
    out.push({ ID: i + 1, Kind: kind, Time: NOW - 3600 + i * 10, Text: text });
  }
  return out;
}
// id -> [name, status, messages]
const JOBS = {
  950: ['Example.Success.Log', 'SUCCESS/PAR', log('Example.Success.Log', 54, false)],
  951: ['Example.Failure.Log', 'FAILURE/PAR', log('Example.Failure.Log', 141, true)],
  952: ['Example.Failure.Nolog', 'FAILURE/PAR', []],
};

function withJobs(result) {
  const template = result[result.length - 1];
  const added = Object.entries(JOBS).map(([id, [name, status, messages]], i) => ({
    ...template, NZBID: +id, ID: +id, NZBName: name, NZBNicename: name, Name: name, NZBFilename: name + '.nzb',
    DupeKey: '', Status: status, ParStatus: status.endsWith('PAR') && status.startsWith('FAILURE') ? 'FAILURE' : 'SUCCESS',
    HistoryTime: NOW - 60 * (i + 1), MessageCount: messages.length, Parameters: [{ Name: '*Unpack:', Value: 'yes' }] }));
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
      const context = await browser.newContext({ viewport: { width, height }, isMobile: width < 600, hasTouch: width < 600 });
      await context.addInitScript(t => localStorage.setItem('Theme', t), theme);
      const page = await context.newPage();
      const errors = [];
      const loadlogs = [];
      page.on('pageerror', e => errors.push(e.message));
      page.on('dialog', d => { errors.push('dialog: ' + d.message()); d.dismiss(); });
      await installDetailsFixture(page, base, [], {
        patchReply: (method, result) => method === 'history' ? withJobs(result) : result
      });
      // loadlog is a safe method: a GET with its parameters in the query
      await page.route(/\/jsonrpc\/loadlog/, route => {
        const params = new URL(route.request().url()).searchParams.getAll('').map(Number);
        if (!JOBS[params[0]]) return route.fallback();
        loadlogs.push(params);
        return route.fulfill({ contentType: 'application/json', json: { id: 1, result: JOBS[params[0]][2] } });
      });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      await page.locator('#HistoryTabLink').click();
      await page.locator('#HistoryTable tbody tr', { hasText: 'Example Success Log' }).first().waitFor();
      const tag = `${theme}-${width}`;
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) {
          failures.push(`${tag}/${label}: ${error.message.split('\n')[0]}`);
          await perPage(10).catch(() => {});
          await close().catch(() => {});
        }
      };
      const openLog = async name => {
        await page.locator('#HistoryTable tbody tr', { hasText: name.replace(/\./g, ' ') }).first().locator('a[data-nzbid]').first().click();
        await page.locator('#HistoryEditDialog').waitFor({ state: 'visible' });
        await page.locator('#HistoryEdit_Log').click();
        await page.locator('#HistoryEdit_LogTab').waitFor({ state: 'visible' });
        await page.waitForFunction(() => !document.querySelector('#HistoryEditDialog .loading-block') ||
          getComputedStyle(document.querySelector('#HistoryEditDialog .loading-block')).display === 'none');
        // the panel slides in: measure once it has settled
        await page.waitForFunction(() => {
          const d = document.querySelector('#HistoryEditDialog').getBoundingClientRect();
          return d.right <= window.innerWidth + 1 && d.left >= -1;
        }, null, { timeout: 5000 });
      };
      // the native select sits under the styled one: set it as the page would
      const perPage = n => page.evaluate(v => $('#HistoryLogRecordsPerPage').val(v).trigger('change'), String(n));
      const close = async () => {
        if (!await page.locator('#HistoryEditDialog').isVisible()) return;
        // Escape in the log search clears the search; from the panel it closes it
        await page.evaluate(() => document.activeElement && document.activeElement.blur());
        await page.keyboard.press('Escape');
        await page.locator('#HistoryEditDialog').waitFor({ state: 'hidden' });
      };
      // every visible log row: text inside the panel, unclipped, with readable contrast
      const readable = () => page.evaluate(() => {
        const rgb = c => (c.match(/[\d.]+/g) || []).map(Number);
        const lum = ([r, g, b]) => {
          const f = v => { v /= 255; return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); };
          return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
        };
        const background = el => {
          for (let e = el; e; e = e.parentElement) {
            const c = rgb(getComputedStyle(e).backgroundColor);
            if (c.length >= 3 && (c.length < 4 || c[3] > 0.5)) return c;
          }
          return [255, 255, 255];
        };
        const panel = document.querySelector('#HistoryEditDialog').getBoundingClientRect();
        const problems = [];
        let worst = 21;
        const rows = [...document.querySelectorAll('#HistoryEdit_LogTable tbody tr')];
        for (const row of rows) {
          const cell = row.querySelector('td:last-child');
          const r = cell.getBoundingClientRect();
          const style = getComputedStyle(cell);
          const fg = rgb(style.color), bg = background(cell);
          const [a, b] = [lum(fg), lum(bg)].sort((x, y) => y - x);
          const ratio = (a + 0.05) / (b + 0.05);
          worst = Math.min(worst, ratio);
          if (ratio < 4.5) problems.push(`contrast ${ratio.toFixed(2)} for "${cell.innerText.slice(0, 40)}"`);
          if (r.left < panel.left - 1 || r.right > panel.right + 1) problems.push(`outside the panel: ${cell.innerText.slice(0, 40)}`);
          if (cell.scrollWidth > cell.clientWidth + 1) problems.push(`clipped: ${cell.innerText.slice(0, 40)}`);
          if (style.visibility === 'hidden' || +style.opacity < 0.5 || parseFloat(style.fontSize) < 11) problems.push(`hard to see: ${cell.innerText.slice(0, 40)}`);
          if (!cell.innerText.trim()) problems.push('an empty text cell');
        }
        return { rows: rows.length, texts: rows.map(r => r.querySelector('td:last-child').innerText), problems, worst,
          pageScroll: document.documentElement.scrollWidth > window.innerWidth + 1 };
      });
      // scrolls the last row into view: no internal container may keep it out of reach
      const lastRowReachable = () => page.evaluate(() => {
        const rows = document.querySelectorAll('#HistoryEdit_LogTable tbody tr');
        const last = rows[rows.length - 1];
        last.scrollIntoView({ block: 'end' });
        const r = last.getBoundingClientRect();
        const hit = document.elementFromPoint(r.left + Math.min(r.width / 2, 40), r.top + Math.min(r.height / 2, 10));
        return r.height > 0 && r.bottom <= window.innerHeight + 1 && r.top >= 0 && !!hit && last.contains(hit);
      });

      for (const [id, [name, , messages]] of Object.entries(JOBS).slice(0, 2)) {
        await check(`${name}: the whole log, newest first, readable`, async () => {
          await openLog(name);
          assert.deepEqual(loadlogs[loadlogs.length - 1], [+id, 0, 10000], 'asks for the whole log');
          await page.waitForFunction(() => document.querySelectorAll('#HistoryEdit_LogTable tbody tr').length > 0, null, { timeout: 5000 });
          const r = await readable();
          assert.deepEqual(r.problems, []);
          assert.equal(r.pageScroll, false, 'no horizontal page scroll');
          assert.equal(r.rows, 10, 'ten rows a page');
          assert.match(r.texts[0], new RegExp(`Collection ${name} added to history`), 'newest first');
          assert.equal(await page.locator('#HistoryEdit_LogEmpty').isVisible(), false);
          assert.match(await page.locator('#HistoryEdit_LogTable_pager').innerText(), new RegExp(String(Math.ceil(messages.length / 10))), 'pages to the oldest');
          assert(await lastRowReachable(), 'the last row on the page can be reached');
          if (capture) {
            await page.evaluate(() => document.fonts.ready);
            await page.screenshot({ path: path.join(capture, `history-log-${id}-${tag}.jpg`), type: 'jpeg', quality: 88 });
          }
          // all of it on one page: the oldest message is there and reachable too
          await perPage(1000);
          await page.waitForFunction(n => document.querySelectorAll('#HistoryEdit_LogTable tbody tr').length === n, messages.length, { timeout: 5000 })
            .catch(async () => { throw new Error(`one page: ${messages.length} wanted, ${await page.locator('#HistoryEdit_LogTable tbody tr').count()} shown`); });
          const all = await readable();
          assert.deepEqual(all.problems, []);
          assert.match(all.texts[all.texts.length - 1], new RegExp(`Collection ${name} added to queue`), 'oldest last');
          assert(await lastRowReachable(), 'the oldest message can be reached');
          // search narrows the log; Enter runs it and never saves and closes the panel
          await page.locator('#HistoryEdit_LogTable_filter').fill('recovery');
          await page.locator('#HistoryEdit_LogTable_filter').press('Enter');
          const want = messages.filter(m => m.Text.includes('recovery')).length;
          await page.waitForFunction(n => document.querySelectorAll('#HistoryEdit_LogTable tbody tr').length === n ||
            (n === 0 && document.querySelectorAll('#HistoryEdit_LogTable tbody tr').length <= 1), want, { timeout: 5000 })
            .catch(async () => { throw new Error(`search: ${want} wanted, ${await page.locator('#HistoryEdit_LogTable tbody tr').count()} shown`); });
          assert(await page.locator('#HistoryEditDialog').isVisible(), 'Enter in the log search keeps the panel open');
          assert(await page.locator('#HistoryEdit_LogTab').isVisible(), 'and on its Log page');
          await perPage(10);
          await close();
        });
      }

      await check('a failed download without a kept log says so', async () => {
        await openLog('Example.Failure.Nolog');
        await page.locator('#HistoryEdit_LogEmpty').waitFor({ state: 'visible' });
        assert.equal((await page.locator('#HistoryEdit_LogEmpty').innerText()).trim(), 'No log was kept for this download.');
        assert.equal(await page.locator('#HistoryEdit_LogTable tbody tr td:last-child').count() === 0 ||
          !(await page.locator('#HistoryEdit_LogTable tbody tr td:last-child').first().innerText()).trim(), true);
        if (capture) await page.screenshot({ path: path.join(capture, `history-log-952-${tag}.jpg`), type: 'jpeg', quality: 88 });
        await close();
        // and the next one opened shows its own log, not the empty note
        await openLog('Example.Success.Log');
        await page.waitForFunction(() => document.querySelectorAll('#HistoryEdit_LogTable tbody tr').length === 10, null, { timeout: 5000 });
        assert.equal(await page.locator('#HistoryEdit_LogEmpty').isVisible(), false);
        await close();
      });

      await check('no page errors', async () => assert.deepEqual(errors, []));
      await context.close();
    }
  } finally { await browser.close(); }
  if (failures.length) {
    console.error(failures.join('\n'));
    process.exitCode = 1;
  }
  process.stdout.write(`history-log: ${cases - failures.length} of ${cases} cases passed\n`);
})().catch(error => { console.error(error); process.exitCode = 1; });
