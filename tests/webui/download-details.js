// Download and history details open as a side panel beside the list, show the
// dupe fleet, and fit without scrolling on a desktop window. Offline: committed
// assets, real startup and renderers, synthetic RPC replies; writes intercepted.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

(async () => {
  const base = fixtureURL(process.argv[2] || 'http://127.0.0.1:17999/');
  const browser = await chromium.launch(process.env.NZBGET_CHROME_CHANNEL === 'none' ? {} : { channel: process.env.NZBGET_CHROME_CHANNEL || 'chrome' })
    .catch(() => chromium.launch());
  const failures = [];
  const capture = process.env.NZBGET_UI_CAPTURE_DIR;
  try {
    for (const theme of ['light', 'dark']) for (const [width, height] of [[1440, 900], [1280, 800], [768, 1024], [390, 844], [320, 640]]) {
      const context = await browser.newContext({ viewport: { width, height } });
      await context.addInitScript(t => localStorage.setItem('Theme', t), theme);
      const page = await context.newPage();
      const errors = [], writes = [];
      page.on('pageerror', e => errors.push(e.message));
      await installDetailsFixture(page, base, writes);
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      const check = async (label, action) => {
        try { await action(); } catch (error) { failures.push(`${theme}/${width}/${label}: ${error.message}`); }
      };
      const panel = page.locator('#DownloadsEditDialog');

      await page.locator('#DownloadsTable tbody a[data-nzbid="41"]').first().click();
      await panel.waitFor({ state: 'visible' });
      await page.waitForTimeout(250);

      await check('side panel', async () => {
        const box = await panel.boundingBox();
        assert(Math.abs(box.y) <= 1 && Math.abs(box.y + box.height - height) <= 1, 'Panel spans the window height');
        assert(Math.abs(box.x + box.width - width) <= 1, 'Panel is docked to the right edge');
        if (width >= 1280) assert(box.width <= 600, 'The list stays visible beside the panel');
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, 'No horizontal page scroll');
      });

      await check('fits without scrolling', async () => {
        if (width < 768 || height < 900) return;
        const body = page.locator('#DownloadsEditDialog .modal-body');
        const { scroll, client } = await body.evaluate(e => ({ scroll: e.scrollHeight, client: e.clientHeight }));
        assert(scroll <= client + 1, `Details fit the panel (${scroll} > ${client})`);
      });

      await check('summary and fleet', async () => {
        assert.match(await page.locator('#DownloadsEdit_Summary').innerText(), /1\.69 GB[\s\S]*97\.0%[\s\S]*2466/);
        const roles = await page.locator('#DownloadsEdit_Fleet .fleet-member .fleet-role').allInnerTexts();
        assert.deepEqual(roles.map(r => r.toLowerCase()), ['primary', 'twin', 'alt', 'primary'], 'Copies in donor priority, history last');
        assert.match(await page.locator('#DownloadsEdit_Fleet .fleet-members').innerText(), /borrowable/i);
        assert.match(await page.locator('#DownloadsEdit_Fleet .fleet-facts').innerText(), /41 \(29\.4 MB\)[\s\S]*3 holes/);
        assert.equal(await page.locator('#DownloadsEdit_Fleet .fleet-member.is-current').count(), 1);
        for (const row of await page.locator('#DownloadsEdit_Fleet .fleet-member').all()) {
          const b = await row.boundingBox(), p = await panel.boundingBox();
          assert(b.x >= p.x && b.x + b.width <= p.x + p.width + 1, 'Fleet rows stay inside the panel');
        }
      });

      if (capture && [1440, 390].includes(width)) {
        await page.screenshot({ path: path.join(capture, `details-download-${theme}-${width}.jpg`), type: 'jpeg', quality: 90 });
      }

      await check('switch to twin', async () => {
        await page.locator('#DownloadsEdit_Fleet a.fleet-member[data-fleet-id="42"]').click();
        await page.waitForFunction(() => /copy 02/.test(document.getElementById('DownloadsEdit_Title').textContent));
        assert(await panel.isVisible());
        assert.match(await page.locator('#DownloadsEdit_Fleet .fleet-member.is-current').innerText(), /copy 02/);
      });

      await check('files page widens the panel', async () => {
        if (width < 768) return;
        const before = (await panel.boundingBox()).width;
        await page.locator('#DownloadsEdit_File').click();
        await page.locator('#DownloadsEdit_FileTab').waitFor({ state: 'visible' });
        assert((await panel.boundingBox()).width > before, 'Files get a wider panel');
        await page.locator('#DownloadsEdit_Back').click();
        await page.locator('#DownloadsEdit_GeneralTab').waitFor({ state: 'visible' });
        assert(Math.abs((await panel.boundingBox()).width - before) <= 1, 'Back restores the panel width');
      });

      await check('save still edits the shown copy', async () => {
        await page.locator('#DownloadsEdit_NZBName').fill('Renamed copy 02');
        await page.locator('#DownloadsEdit_Save').click();
        await panel.waitFor({ state: 'hidden' });
        assert(writes.some(w => w.method === 'editqueue' && w.params[0] === 'GroupSetName' && w.params[1] === 'Renamed copy 02' && w.params[2][0] === 42));
      });

      await check('history copy from the fleet', async () => {
        await page.locator('#DownloadsTable tbody a[data-nzbid="41"]').first().click();
        await panel.waitFor({ state: 'visible' });
        await page.locator('#DownloadsEdit_Fleet a.fleet-member[data-fleet-source="history"]').click();
        await page.locator('#HistoryEditDialog').waitFor({ state: 'visible' });
        assert.equal(await panel.isVisible(), false);
        assert.match(await page.locator('#HistoryEdit_Status').innerText(), /PAR: FAILURE/i);
        assert.match(await page.locator('#HistoryEdit_Summary').innerText(), /1\.69 GB/);
        if (width >= 768 && height >= 900) {
          const { scroll, client } = await page.locator('#HistoryEditDialog .modal-body').evaluate(e => ({ scroll: e.scrollHeight, client: e.clientHeight }));
          assert(scroll <= client + 1, `History details fit the panel (${scroll} > ${client})`);
        }
        if (capture && [1440, 390].includes(width)) {
          await page.screenshot({ path: path.join(capture, `details-history-${theme}-${width}.jpg`), type: 'jpeg', quality: 90 });
        }
        await page.keyboard.press('Escape');
        await page.locator('#HistoryEditDialog').waitFor({ state: 'hidden' });
        assert.equal(await page.evaluate(() => Refresher.isPaused()), false, 'Closing resumes list updates');
      });

      if (errors.length) failures.push(`${theme}/${width}: page errors: ${errors.join('; ')}`);
      await context.close();
    }
  } finally {
    await browser.close();
  }
  if (failures.length) {
    console.error(failures.join('\n'));
    process.exit(1);
  }
  console.log('download-details: all checks passed');
})();
