// Settings after the option clean-up: removed options are gone, news server Level
// became a 1-5 Priority menu, Check and repair is one card with a Renaming menu,
// Listen address has an "Allow other computers" checkbox, and updates are off.
// Offline: the committed WebUI with its real startup over synthetic RPC replies
// (details-fixture). saveconfig is intercepted, nothing reaches a daemon.
// Usage: node tests/webui/settings-removals.js http://127.0.0.1:17999/
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

const REMOVED = ['ContinuePartial', 'PropagationDelay', 'ParQuick', 'CrcCheck', 'UpdateCheck', 'Server1_JoinGroup', 'Server1_Level'];

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
      await context.addInitScript(() => localStorage.setItem('SettingsMode', 'advanced'));
      const page = await context.newPage();
      const errors = [], writes = [], rpcs = [];
      page.on('pageerror', e => errors.push(e.message));
      page.on('request', r => { if (/jsonrpc/.test(r.url())) rpcs.push(r.url() + ' ' + (r.postData() || '')); });
      await installDetailsFixture(page, base, writes, { patchReply: (method, result) => {
        if (method === 'config' || method === 'loadconfig') {
          const set = (name, value) => { const o = result.find(x => x.Name === name); if (o) o.Value = value; else result.push({ Name: name, Value: value }); };
          set('ControlIP', '127.0.0.1');
          set('DirectRename', 'yes'); set('ParRename', 'yes'); set('RarRename', 'yes');
          set('HardLinking', 'no');
        }
        return result;
      } });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      await page.evaluate(() => $('a[href="#ConfigTab"]').first().click());
      await page.waitForFunction(() => document.querySelector('#ConfigMode'));
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${theme}/${width}/${label}: ${error.message}`); }
      };
      const mode = async m => { await page.locator(`#ConfigMode button[data-mode="${m}"]`).click(); await page.waitForTimeout(150); };
      const show = async id => { await page.evaluate(i => Config.showSection(i, false), id); await page.waitForTimeout(150); };
      const S = name => page.locator('#S_' + name);
      const row = name => S(name).locator('xpath=ancestor::div[contains(@class,"control-group")][1]');

      await check('removed options are gone', async () => {
        for (const name of REMOVED) assert.equal(await S(name).count(), 0, name + ' has no row');
      });

      await check('server priority is a 1-5 menu, and an old Level is read as a priority', async () => {
        await show('NEWS-SERVERS');
        await page.locator('.config-list-row.NEWS-SERVERS.multiid1 .config-list-edit').click();
        const menu = row('Server1_Priority').locator('select.config-select');
        assert.deepEqual(await menu.locator('option').evaluateAll(o => o.map(x => x.value)), ['1', '2', '3', '4', '5']);
        assert.equal(await menu.inputValue(), '1');
        await page.evaluate(() => Config.closeServerEditor());
        await page.locator('.config-list-row.NEWS-SERVERS.multiid2 .config-list-edit').click();
        assert.equal(await row('Server2_Priority').locator('select.config-select').inputValue(), '2', 'Level 1 is priority 2');
      });

      await check('check and repair is one card with a Renaming menu', async () => {
        await show('CHECK_AND_REPAIR');
        const cards = page.locator('#ConfigData .config-card.CHECK_AND_REPAIR');
        assert.equal(await cards.count(), 1);
        assert.equal((await cards.locator('.config-card__title').textContent()).trim(), 'Check and repair');
        const menu = page.locator('#ConfigRenaming');
        assert(await menu.isVisible(), 'Advanced shows the Renaming menu');
        assert.equal(await menu.inputValue(), 'during');
        assert.equal(await row('DirectRename').isVisible(), false, 'and not the three options');
        await menu.selectOption('after');
        assert.equal(await page.evaluate(() => $('#S_DirectRename .btn-primary').attr('data-value')), 'no');
        assert.equal(await page.evaluate(() => $('#S_ParRename .btn-primary').attr('data-value')), 'yes');
        await page.waitForTimeout(250);
        assert(await row('HardLinking').evaluate(el => el.classList.contains('is-locked')), 'Hard linking needs renaming while downloading');
        assert.match(await row('HardLinking').locator('.config-lock-note').textContent(), /Renaming/);
        await mode('expert');
        assert.equal(await menu.isVisible(), false, 'Expert shows the options one by one');
        assert(await row('DirectRename').isVisible());
        await S('ParRename').click();
        await mode('advanced');
        await page.waitForTimeout(250);
        assert.equal(await menu.inputValue(), 'custom', 'A mix the menu has no name for');
        await menu.selectOption('during');
        await page.waitForTimeout(250);
        assert.equal(await row('HardLinking').evaluate(el => el.classList.contains('is-locked')), false);
      });

      await check('listen address can open to other computers', async () => {
        await show('SECURITY');
        const box = page.locator('#ConfigListenAll');
        assert.equal(await box.isChecked(), false);
        await box.check();
        assert.equal(await S('ControlIP').inputValue(), '0.0.0.0');
        await box.uncheck();
        assert.equal(await S('ControlIP').inputValue(), '127.0.0.1');
        await S('ControlIP').fill('0.0.0.0');
        await S('ControlIP').dispatchEvent('input');
        await page.waitForTimeout(250);
        assert.equal(await box.isChecked(), true, 'Typing 0.0.0.0 ticks it');
      });

      await check('updates are off', async () => {
        assert.equal(await page.locator('#ConfigData [onclick*="checkUpdates"]').count(), 0, 'No update button under System');
        assert(!rpcs.some(r => /checkupdates|readurl|update/i.test(r)), 'No update check was made');
      });

      await check('the save uses Priority and drops Level and removed options', async () => {
        await page.locator('#ConfigSaveBar').waitFor({ state: 'visible' });
        await page.evaluate(() => Config.saveChanges());
        await page.waitForFunction(n => n.some(w => w.method === 'saveconfig'), writes, { polling: 100, timeout: 5000 }).catch(() => {});
        await page.waitForTimeout(500);
        const save = writes.find(w => w.method === 'saveconfig');
        assert(save, 'saveconfig was sent');
        const values = Object.fromEntries(save.params[0].map(o => [o.Name, o.Value]));
        assert.equal(values['Server1.Priority'], '1');
        assert.equal(values['Server2.Priority'], '2');
        for (const name of ['Server1.Level', 'Server2.Level', 'Server1.JoinGroup', 'ContinuePartial', 'PropagationDelay', 'ParQuick', 'CrcCheck', 'UpdateCheck']) {
          assert.equal(values[name], undefined, name + ' is not saved');
        }
        assert.equal(values.ControlIP, '0.0.0.0');
        assert.equal(values.DirectRename, 'yes');
        assert.equal(values.ParRename, 'yes');
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
  console.log(`settings-removals: ${cases} cases passed`);
})();
