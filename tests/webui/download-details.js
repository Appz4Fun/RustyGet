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
      const fixture = {};
      page.on('pageerror', e => errors.push(e.message));
      await installDetailsFixture(page, base, writes, fixture);
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      // late-reply checks wait until the page has run a held reply's callback
      await page.evaluate(() => {
        window.__rpcHandled = [];
        const call = RPC.call;
        RPC.call = function(method, params, done, ...rest) {
          if (typeof done !== 'function') return call.call(this, method, params, done, ...rest);
          return call.call(this, method, params, function(result) {
            try { return done(result); } finally {
              window.__rpcHandled.push(method + ':' + (method === 'listfiles' ? params[2] : params && params[0]));
            }
          }, ...rest);
        };
      });
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
        const stats = await page.locator('#DownloadsEdit_Fleet .fleet-stat').allInnerTexts();
        assert.equal(stats.length, 4, 'Four borrow stats');
        assert.match(stats[0], /Articles borrowed\s+41\b/i);
        assert.match(stats[1], /Par2 borrowed\s+6\s+articles in 2 par2 files/i);
        assert.match(stats[2], /Bytes patched\s+29\.4 MB\s+3 holes/i);
        assert.match(stats[3], /Can borrow from\s+2 verified\s+twins 1 · alts 1/i, 'The twin and the checked borrowable alt; not the history copy');
        assert.equal(await page.locator('#DownloadsEdit_Fleet .fleet-member.is-current').count(), 1);
        const health = await page.locator('#DownloadsEdit_Fleet .fleet-member-health').allInnerTexts();
        assert.deepEqual(health, ['97.0%', '99.4%', '88.1%', '97.0%'], 'Sampled DupeHealth/DupeAlive percentages render as percentages');
        for (const row of await page.locator('#DownloadsEdit_Fleet .fleet-member').all()) {
          const b = await row.boundingBox(), p = await panel.boundingBox();
          assert(b.x >= p.x && b.x + b.width <= p.x + p.width + 1, 'Fleet rows stay inside the panel');
        }
      });

      if (capture && [1440, 390].includes(width)) {
        await page.screenshot({ path: path.join(capture, `details-download-${theme}-${width}.jpg`), type: 'jpeg', quality: 90 });
      }

      await check('donor counting rules', async () => {
        const r = await page.evaluate(() => {
          const g = id => Downloads.groups.find(x => x.NZBID === id);
          const withParam = (item, name, value) => Object.assign({}, item, { Parameters: item.Parameters.map(p => p.Name === name ? { Name: name, Value: value } : p) });
          const run = (self, groups) => {
            const keep = Downloads.groups;
            Downloads.groups = groups;
            try { return DetailsPanel.donors(self, DetailsPanel.members(self), 'queue'); } finally { Downloads.groups = keep; }
          };
          const all = Downloads.groups.slice();
          const unchecked = all.map(x => x.NZBID === 43 ? withParam(x, 'DupeVideo', 'unverified') : x);
          const dead = all.map(x => x.NZBID === 42 ? Object.assign({}, x, { SuccessArticles: 0, FailedArticles: 10, Health: 0 }) : x);
          const hist = History.items.find(x => x.NZBID === 31);
          return {
            fromPrimary: run(g(41), all),
            fromHistoryCopy: (() => {
              const keep = Downloads.groups;
              try { return DetailsPanel.donors(hist, DetailsPanel.members(hist), 'history'); } finally { Downloads.groups = keep; }
            })(),
            fromAlt: run(g(43), all),
            fromTwin: run(g(42), all),
            unchecked: run(g(41), unchecked),
            dead: run(g(41), dead)
          };
        });
        assert.deepEqual(r.fromPrimary, { twins: 1, alts: 1, unverified: 0 }, 'The primary skips the history try of its own post');
        assert.deepEqual(r.fromHistoryCopy, { twins: 1, alts: 1, unverified: 0 }, 'The history try skips the queued copy of its own post');
        assert.deepEqual(r.fromAlt, { twins: 2, alts: 0, unverified: 0 }, 'An alt counts the primary and its twin alike, the history try once');
        assert.deepEqual(r.fromTwin, { twins: 1, alts: 1, unverified: 0 }, 'A twin borrows from its primary post (once, not again for its history copy) and the checked alt');
        assert.deepEqual(r.unchecked, { twins: 1, alts: 0, unverified: 1 }, 'An alt the intake could not check is unverified');
        assert.deepEqual(r.dead, { twins: 0, alts: 1, unverified: 0 }, 'A dead copy is no donor');
      });

      await check('critical health wins over the fixed threshold', async () => {
        const cls = await page.evaluate(() => {
          const g = Object.assign({}, Downloads.groups.find(x => x.NZBID === 41), { Health: 970, CriticalHealth: 990 });
          const div = document.createElement('div'); div.innerHTML = DetailsPanel.summary(g, 'DownloadsEdit');
          return div.querySelectorAll('.details-tile-value')[2].className;
        });
        assert.match(cls, /is-bad/, 'Health below critical is shown as failing');
      });

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

      await check('fleet stays on the copy while its save is in flight', async () => {
        let release;
        fixture.holdMutation = () => new Promise(resolve => { release = resolve; });
        try {
          await page.locator('#DownloadsEdit_NZBName').fill('Held copy 02');
          await page.locator('#DownloadsEdit_Save').click();
          await page.waitForFunction(() => document.getElementById('DownloadsEditDialog').classList.contains('is-saving'));
          await page.locator('#DownloadsEdit_Fleet a.fleet-member[data-fleet-id="43"]').dispatchEvent('click');
          assert.match(await page.locator('#DownloadsEdit_Title').innerText(), /copy 02/, 'No switch while saving');
          await page.locator('.modal-backdrop').last().dispatchEvent('click');
          assert(await panel.isVisible(), 'A click beside the panel does not interrupt a save');
        } finally {
          fixture.holdMutation = undefined;
          if (release) release();
        }
        await panel.waitFor({ state: 'hidden' });
        assert(writes.every(w => w.method !== 'editqueue' || w.params[2][0] === 42), 'Every edit went to the copy being saved');
        await page.locator('#DownloadsTable tbody a[data-nzbid="42"]').first().click();
        await panel.waitFor({ state: 'visible' });
      });

      await check('log for a copy with nothing logged yet', async () => {
        assert(await page.locator('#DownloadsEdit_Log').isVisible(), 'Every download offers its log');
        await page.locator('#DownloadsEdit_Log').click();
        await page.locator('#DownloadsEdit_LogTab').waitFor({ state: 'visible' });
        await page.locator('#DownloadsEdit_LogEmpty').waitFor({ state: 'visible' });
        await page.locator('#DownloadsEdit_Back').click();
        await page.locator('#DownloadsEdit_GeneralTab').waitFor({ state: 'visible' });
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

      await check('downloading copy log', async () => {
        await page.locator('#DownloadsTable tbody a[data-nzbid="41"]').first().click();
        await panel.waitFor({ state: 'visible' });
        await page.locator('#DownloadsEdit_Log').click();
        await page.locator('#DownloadsEdit_LogTab').waitFor({ state: 'visible' });
        await page.waitForFunction(() => /borrowed from twin copy 02/.test(document.getElementById('DownloadsEdit_LogTable').textContent));
        assert.equal(await page.locator('#DownloadsEdit_LogEmpty').isVisible(), false);
        await page.keyboard.press('Escape');
        await panel.waitFor({ state: 'hidden' });
      });

      await check('late log reply for a copy the panel left is dropped', async () => {
        let release;
        await page.evaluate(() => { window.__rpcHandled = []; });
        const asked = new Promise(resolve => {
          fixture.holdLoadlog = () => { resolve(); return new Promise(r => { release = r; }); };
        });
        try {
          await page.locator('#DownloadsTable tbody a[data-nzbid="41"]').first().click();
          await panel.waitFor({ state: 'visible' });
          await page.locator('#DownloadsEdit_Log').click();
          await asked;
          fixture.holdLoadlog = undefined;
          await page.keyboard.press('Escape');
          await panel.waitFor({ state: 'hidden' });
          await page.locator('#DownloadsTable tbody a[data-nzbid="42"]').first().click();
          await panel.waitFor({ state: 'visible' });
          await page.locator('#DownloadsEdit_Log').click();
          await page.locator('#DownloadsEdit_LogEmpty').waitFor({ state: 'visible' });
        } finally {
          fixture.holdLoadlog = undefined;
          if (release) release();
        }
        // the held copy 01 reply must have reached the page before copy 02 is checked
        await page.waitForFunction(k => window.__rpcHandled.includes(k), 'loadlog:41');
        assert.doesNotMatch(await page.locator('#DownloadsEdit_LogTable').innerText(), /copy 01/, "Copy 01's late log stays out of copy 02");
        assert(await page.locator('#DownloadsEdit_LogEmpty').isVisible());
        assert.equal(await page.locator('#DownloadsEditDialog .loading-block:visible').count(), 0, 'No stuck spinner');
        await page.keyboard.press('Escape');
        await panel.waitFor({ state: 'hidden' });
      });

      await check('late files reply for a copy the panel left is dropped', async () => {
        let release;
        await page.evaluate(() => { window.__rpcHandled = []; });
        const asked = new Promise(resolve => {
          fixture.holdListfiles = () => { resolve(); return new Promise(r => { release = r; }); };
        });
        try {
          await page.locator('#DownloadsTable tbody a[data-nzbid="41"]').first().click();
          await panel.waitFor({ state: 'visible' });
          await page.locator('#DownloadsEdit_File').click();
          await asked;
          fixture.holdListfiles = undefined;
          await page.keyboard.press('Escape');
          await panel.waitFor({ state: 'hidden' });
          await page.locator('#DownloadsTable tbody a[data-nzbid="42"]').first().click();
          await panel.waitFor({ state: 'visible' });
        } finally {
          fixture.holdListfiles = undefined;
          if (release) release();
        }
        // the held copy 01 reply must have reached the page before copy 02 is checked
        await page.waitForFunction(k => window.__rpcHandled.includes(k), 'listfiles:41');
        await page.locator('#DownloadsEdit_File').click();
        await page.waitForFunction(() => /copy02\.7z\.001/.test(document.getElementById('DownloadsEdit_FileTable').textContent));
        assert.doesNotMatch(await page.locator('#DownloadsEdit_FileTable').innerText(), /copy01/, "Copy 01's late files stay out of copy 02");
        await page.keyboard.press('Escape');
        await panel.waitFor({ state: 'hidden' });
      });

      await check('files reply arriving after the panel closed is dropped', async () => {
        let release;
        await page.evaluate(() => { window.__rpcHandled = []; });
        const asked = new Promise(resolve => {
          fixture.holdListfiles = () => { resolve(); return new Promise(r => { release = r; }); };
        });
        try {
          await page.locator('#DownloadsTable tbody a[data-nzbid="41"]').first().click();
          await panel.waitFor({ state: 'visible' });
          await page.locator('#DownloadsEdit_File').click();
          await asked;
          fixture.holdListfiles = undefined;
          await page.keyboard.press('Escape');
          await panel.waitFor({ state: 'hidden' });
        } finally {
          fixture.holdListfiles = undefined;
          if (release) release();
        }
        await page.waitForFunction(k => window.__rpcHandled.includes(k), 'listfiles:41');
        assert.doesNotMatch(await page.locator('#DownloadsEdit_FileTable').textContent(), /copy01/, 'A closed panel takes no file list');
      });

      await check('closing one panel keeps the other panel log', async () => {
        await page.locator('#DownloadsTable tbody a[data-nzbid="41"]').first().click();
        await panel.waitFor({ state: 'visible' });
        await page.locator('#DownloadsEdit_Log').click();
        await page.waitForFunction(() => /Downloading copy 01/.test(document.getElementById('DownloadsEdit_LogTable').textContent));
        // the history panel's close handler runs while the downloads panel shows its log
        await page.evaluate(() => { LogTab.reset('History'); });
        const saved = await page.evaluate(() => {
          let out = null;
          const keep = Util.saveToLocalFile;
          Util.saveToLocalFile = (text, type, name) => { out = { text, name }; return true; };
          try { LogTab.export(); } finally { Util.saveToLocalFile = keep; }
          return out;
        });
        assert(saved && /Downloading copy 01/.test(saved.text), 'The downloads log can still be saved');
        await page.keyboard.press('Escape');
        await panel.waitFor({ state: 'hidden' });
      });

      await check('a confirm opened from the panel keeps both open on a click beside them', async () => {
        if (width < 768) return;
        const history = page.locator('#HistoryEditDialog');
        await page.locator('#HistoryTabLink').click();
        await page.locator('#HistoryTable tbody a[data-nzbid="32"]').first().click();
        await history.waitFor({ state: 'visible' });
        await page.evaluate(() => ConfirmDialog.showModal('HistoryEditRedownloadConfirmDialog', function() {}));
        await page.locator('#ConfirmDialog').waitFor({ state: 'visible' });
        await page.mouse.click(12, Math.round(height / 2));
        await page.waitForTimeout(300);
        assert(await history.isVisible(), 'Panel stays open behind its confirm');
        assert(await page.locator('#ConfirmDialog').isVisible(), 'Confirm stays open');
        await page.evaluate(() => $('#ConfirmDialog').modal('hide'));
        await page.locator('#ConfirmDialog').waitFor({ state: 'hidden' });
        await page.mouse.click(12, Math.round(height / 2));
        await history.waitFor({ state: 'hidden' });
        await page.locator('#DownloadsTabLink').click();
      });

      await check('history log and click beside the panel closes it', async () => {
        const history = page.locator('#HistoryEditDialog');
        await page.locator('#HistoryTabLink').click();
        await page.locator('#HistoryTable tbody a[data-nzbid="32"]').first().click();
        await history.waitFor({ state: 'visible' });
        assert(await page.locator('#HistoryEdit_Log').isVisible(), 'A finished download offers its log even with MessageCount 0');
        await page.locator('#HistoryEdit_Log').click();
        await page.locator('#HistoryEdit_LogTab').waitFor({ state: 'visible' });
        await page.waitForFunction(() => /Unpack for Example\.Completed\.Release successful/.test(document.getElementById('HistoryEdit_LogTable').textContent));
        assert.equal(await page.locator('#HistoryEdit_LogEmpty').isVisible(), false);
        if (width < 768) {
          await page.keyboard.press('Escape');
        } else {
          await page.mouse.click(12, Math.round(height / 2));
        }
        await history.waitFor({ state: 'hidden' });
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
