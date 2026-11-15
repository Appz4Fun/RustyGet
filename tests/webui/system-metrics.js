// Machine tiles on Status, system charts on Statistics, the libraries table and
// the License page. Offline: the committed WebUI with its real startup over
// synthetic RPC replies (details-fixture); nothing reaches a daemon.
// Usage: NZBGET_CHROME_CHANNEL=none node tests/webui/system-metrics.js http://127.0.0.1:17999/
// Optional screenshots: NZBGET_UI_CAPTURE_DIR=/some/dir
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

(async () => {
  const base = fixtureURL(process.argv[2] || 'http://127.0.0.1:17999/');
  const capture = process.env.NZBGET_UI_CAPTURE_DIR;
  const browser = await chromium.launch(process.env.NZBGET_CHROME_CHANNEL === 'none' ? {} : { channel: process.env.NZBGET_CHROME_CHANNEL || 'chrome' })
    .catch(() => chromium.launch());
  const failures = [];
  let cases = 0;
  try {
    for (const theme of ['light', 'dark']) for (const [width, height] of [[1440, 900], [390, 844]]) {
      const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: capture ? 2 : 1 });
      await context.addInitScript(t => localStorage.setItem('Theme', t), theme);
      const page = await context.newPage();
      const errors = [], ranges = [];
      page.on('pageerror', e => errors.push(e.message));
      await installDetailsFixture(page, base, [], { patchReply: (method, result) => {
        if (method === 'systemstats') ranges.push(result.History.Interval);
        return result;
      } });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      const tag = `${theme}-${width}`;
      const shot = async name => { if (capture) await page.screenshot({ path: path.join(capture, `${name}-${tag}.jpg`), quality: 85, fullPage: false }); };
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${tag}/${label}: ${error.message}`); }
      };

      await check('status tiles and library links', async () => {
        await page.evaluate(() => $('a[href="#ConfigTab"]').first().click());
        await page.waitForFunction(() => document.querySelector('#ConfigNav a[href="#NEWS-SERVERS"]'));
        await page.evaluate(() => $('#ConfigNav a[href="#Config-SystemInfo"]').click());
        await page.locator('#SysInfo_Machine .sysmetrics-tile').first().waitFor({ timeout: 5000 });
        const text = await page.locator('#SysInfo_Machine').textContent();
        for (const want of ['Load average', '16 CPUs', 'Ryzen 5000 (Zen 3)', '64.0 GiB', 'RustyGet', 'Swap', 'zswap']) {
          assert(text.includes(want), `tiles show ${want}: ${text}`);
        }
        const icons = require('../../webui/lib/material-symbols-rounded.json').icon_names;
        for (const name of await page.$$eval('#SysInfo_Machine .material-icon', els => els.map(e => e.textContent))) {
          assert(icons.includes(name), `icon ${name} is in the bundled font subset`);
        }
        const links = await page.$$eval('#SysInfo_LibrariesTable a', as => as.map(a => [a.textContent, a.href]));
        const byName = Object.fromEntries(links);
        assert.equal(byName.zipfast, 'https://github.com/xbmc4lyfe/zipfast');
        assert.equal(byName.zipview, 'https://github.com/xbmc4lyfe/zipview');
        assert.equal(byName['par-rs'], 'https://github.com/xbmc4lyfe/par-rs');
        assert(byName.jQuery && byName.Elycharts, 'web interface libraries are listed');
        assert(links.every(([, href]) => /^https:\/\//.test(href)), 'every library links to its home');
        const rows = await page.locator('#SysInfo_LibrariesTable tr').count();
        assert(rows >= 20, `all libraries listed (${rows})`);
        // the Status page lists errors only; warnings and info stay in Settings
        assert.equal(await page.locator('#collapseWarnings, #collapseInfo').count(), 0);
        const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
        assert(overflow <= 0, `no horizontal scroll (${overflow})`);
        await shot('status');
      });

      await check('license page', async () => {
        await page.evaluate(() => $('#ConfigNav a[href="#Config-Licenses"]').click());
        await page.locator('#ConfigLicenses').waitFor({ state: 'visible', timeout: 5000 });
        const pct = await page.locator('#LicenseInternalPercent').innerText();
        const { internal, total, percent } = await page.evaluate(() => Libraries.stats());
        assert.equal(pct, percent + '%');
        assert.equal(await page.locator('#LicenseInternalCount').innerText(), `${internal} of ${total} libraries are internal`);
        assert.equal(await page.locator('.license-card--internal').count(), internal);
        const names = await page.$$eval('.license-card__name', els => els.map(e => e.textContent));
        for (const want of ['zipfast', 'zipview', 'par-rs', 'flate2', 'jQuery']) assert(names.includes(want), want);
        for (const gone of ['iconSweets', 'selfh.st/icons']) assert(!names.includes(gone), `${gone} removed`);
        await page.locator('#ConfigLicenses').scrollIntoViewIfNeeded();
        await shot('license');
      });

      await check('statistics charts', async () => {
        await page.evaluate(() => $('a[href="#StatisticsTab"]').first().click());
        await page.locator('#SystemStats_Charts .sysmetrics-chart path.sysmetrics-line').first().waitFor({ timeout: 5000 });
        const ids = await page.$$eval('#SystemStats_Charts .sysmetrics-chart', els => els.map(e => e.id));
        assert.deepEqual(ids, ['SystemChart_Speed', 'SystemChart_Connections', 'SystemChart_Load', 'SystemChart_Cpu', 'SystemChart_Ram']);
        assert.equal(await page.locator('#SystemChart_Load path.sysmetrics-line').count(), 3);
        assert.equal(await page.locator('#SystemChart_Cpu path.sysmetrics-line').count(), 2);
        assert.equal(await page.locator('#SystemChart_Ram .sysmetrics-legend__item').count(), 2);
        assert((await page.locator('#SystemStats_Tiles').textContent()).includes('Load average'));
        // hover shows a tooltip with every series
        await page.locator('#SystemChart_Cpu').scrollIntoViewIfNeeded();
        const box = await page.locator('#SystemChart_Cpu .sysmetrics-hit').boundingBox();
        await page.mouse.move(box.x + box.width * 0.5, box.y + box.height * 0.5);
        const tip = page.locator('#SystemChart_Cpu .sysmetrics-tip');
        await tip.waitFor({ state: 'visible' });
        const tipText = await tip.innerText();
        assert(tipText.includes('Machine') && tipText.includes('RustyGet'), tipText);
        await shot('statistics');
        await page.mouse.move(0, 0);
        // the 24h range asks for the minute history
        await page.locator('#SystemStats_Range button', { hasText: '24h' }).click();
        await page.waitForFunction(() => SystemMetrics.getLast() && SystemMetrics.getLast().History.Interval === 60);
        assert.equal(await page.locator('#SystemStats_Range button.btn-active').textContent(), '24h');
        const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
        assert(overflow <= 0, `no horizontal scroll (${overflow})`);
      });

      await check('no page errors', async () => assert.deepEqual(errors, []));
      await context.close();
    }
  } finally {
    await browser.close();
  }
  if (failures.length) {
    console.error(failures.join('\n'));
    console.error(`FAIL ${failures.length} of ${cases}`);
    process.exit(1);
  }
  console.log(`PASS ${cases} cases`);
})();
