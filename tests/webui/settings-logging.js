// Logging: one "Write a log file" switch replaces WriteLog and RotateLog, NzbLog is
// gone (download messages go to the main log), an old WriteLog=none reads as off,
// and LogBuffer offers 0 as Unlimited.
// Offline: the committed WebUI over synthetic RPC replies (details-fixture).
// Usage: node tests/webui/settings-logging.js http://127.0.0.1:17999/
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

(async () => {
  const base = fixtureURL(process.argv[2] || 'http://127.0.0.1:17999/');
  const browser = await chromium.launch(process.env.NZBGET_CHROME_CHANNEL === 'none' ? {} : { channel: process.env.NZBGET_CHROME_CHANNEL || 'chrome' })
    .catch(() => chromium.launch());
  const failures = [];
  let cases = 0;
  try {
    for (const theme of ['light', 'dark']) for (const [width, height] of [[1440, 900], [390, 844]]) {
      const context = await browser.newContext({ viewport: { width, height } });
      await context.addInitScript(t => localStorage.setItem('Theme', t), theme);
      await context.addInitScript(() => localStorage.setItem('SettingsMode', 'expert'));
      const page = await context.newPage();
      const errors = [], writes = [];
      page.on('pageerror', e => errors.push(e.message));
      await installDetailsFixture(page, base, writes, { patchReply: (method, result) => {
        if (method === 'config' || method === 'loadconfig') {
          // a configuration file from before the switch
          for (let i = result.length - 1; i >= 0; i--) if (/^(LogToFile|WriteLog|RotateLog|NzbLog)$/.test(result[i].Name)) result.splice(i, 1);
          result.push({ Name: 'WriteLog', Value: 'none' }, { Name: 'RotateLog', Value: '3' }, { Name: 'NzbLog', Value: 'yes' });
        }
        return result;
      } });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      await page.evaluate(() => $('a[href="#ConfigTab"]').first().click());
      await page.waitForFunction(() => document.querySelector('#ConfigMode'));
      await page.evaluate(() => Config.showSection('LOGGING', false));
      await page.waitForTimeout(200);
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${theme}/${width}/${label}: ${error.message}`); }
      };

      await check('one log file switch, no rotation or NZB log options', async () => {
        for (const name of ['WriteLog', 'RotateLog', 'NzbLog']) assert.equal(await page.locator('#S_' + name).count(), 0, name + ' is gone');
        const card = page.locator('#ConfigData .config-card.LOGGING', { has: page.locator('#S_LogToFile') });
        assert.equal((await card.locator('.config-card__title').textContent()).trim(), 'Log file');
        assert.match(await card.locator('.config-card__text').textContent(), /every 10 MB and at the end of each day/);
        assert.match(await card.textContent(), /Write a log file/);
        assert(await card.locator('#S_LogBuffer').count(), 'LogBuffer is in the same card');
      });

      await check('an old WriteLog=none reads as off', async () => {
        assert.equal(await page.evaluate(() => Config.config().values.find(v => v.Name === 'LogToFile').Value), 'no');
        assert.equal(await page.locator('#S_LogToFile').getAttribute('aria-checked'), 'false', 'the switch is off');
      });

      await check('LogBuffer offers Unlimited', async () => {
        const row = page.locator('#S_LogBuffer').locator('xpath=ancestor::div[contains(@class,"control-group")][1]');
        assert.match(await row.textContent(), /Unlimited/);
        assert.match(await row.textContent(), /Keeps every message/);
      });

      await check('saving writes LogToFile and drops the old names', async () => {
        await page.evaluate(() => { $('#S_TimeCorrection').val('1').trigger('change'); Config.saveChanges(); });
        await page.waitForFunction(n => n.some(w => w.method === 'saveconfig'), writes, { polling: 100, timeout: 5000 }).catch(() => {});
        await page.waitForTimeout(500);
        const save = writes.find(w => w.method === 'saveconfig');
        assert(save, 'saveconfig was sent');
        const names = save.params[0].map(o => o.Name);
        assert(names.includes('LogToFile'));
        for (const old of ['WriteLog', 'RotateLog', 'NzbLog']) assert(!names.includes(old), old + ' is not written');
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
  console.log(`settings-logging: ${cases} cases passed`);
})();
