// News server backbone: Group became Backbone, a menu of the bundled backbones
// with "Independent" and "Other (type a name)". The host suggests a backbone: a
// confirmed news host fills it in, a provider domain offers its backbones to
// click, an unknown host warns; a backbone picked by hand is never overwritten.
// Offline: the committed WebUI over synthetic RPC replies (details-fixture);
// backbones is intercepted.
// Usage: node tests/webui/settings-backbone.js http://127.0.0.1:17999/
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

const LIST = [
  { Name: 'Omicron', Providers: ['Newshosting', 'EasyNews'] },
  { Name: 'UsenetExpress', Providers: ['NewsDemon', 'NewsgroupDirect'] },
  { Name: 'Giganews', Providers: ['Giganews', 'NewsgroupDirect'] },
  { Name: 'Eweka Internet Services', Providers: ['Eweka'] }
];
const HOSTS = {
  'news.eweka.nl': { Match: 'confirmed', Provider: 'Eweka', Matches: ['Eweka Internet Services'] },
  'news.easynews.com': { Match: 'confirmed', Provider: 'EasyNews', Matches: ['Omicron'] },
  'news.newsgroupdirect.com': { Match: 'suggested', Provider: 'NewsgroupDirect', Matches: ['UsenetExpress', 'Giganews'] },
  'news.example.org': { Match: 'unknown', Provider: null, Matches: [] },
  '127.0.0.1': { Match: 'skipped', Provider: null, Matches: [] }
};

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
      await context.addInitScript(() => localStorage.setItem('SettingsMode', 'simple'));
      const page = await context.newPage();
      const errors = [], writes = [];
      const opened = { host: 'news.example.org', group: '3' };
      page.on('pageerror', e => errors.push(e.message));
      await installDetailsFixture(page, base, writes, { patchReply: (method, result) => {
        if (method === 'config' || method === 'loadconfig') {
          const set = (name, value) => { const o = result.find(x => x.Name === name); if (o) o.Value = value; else result.push({ Name: name, Value: value }); };
          // an old config: Group 3, no Backbone
          set('Server1.Host', opened.host);
          if (opened.group) set('Server1.Group', opened.group);
        }
        return result;
      } });
      await page.route(/\/jsonrpc/, async route => {
        const rpc = route.request().method() === 'POST' ? route.request().postDataJSON() : null;
        if (!rpc || rpc.method !== 'backbones') return route.fallback();
        const host = rpc.params[0];
        const known = HOSTS[host] || (host ? { Match: 'unknown', Provider: null, Matches: [] } : { Match: 'skipped', Provider: null, Matches: [] });
        return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: Object.assign({ Backbones: LIST, Host: host }, known) } });
      });
      const openServer = async () => {
        await page.goto(base.href);
        await page.locator('#DownloadsTable tbody tr').first().waitFor();
        await page.evaluate(() => $('a[href="#ConfigTab"]').first().click());
        await page.waitForFunction(() => document.querySelector('#ConfigMode'));
        await page.evaluate(() => Config.showSection('NEWS-SERVERS', false));
        await page.waitForTimeout(200);
        await page.locator('.config-list-row.NEWS-SERVERS.multiid1 .config-list-edit').click();
        await page.waitForTimeout(300);
      };
      await openServer();
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${theme}/${width}/${label}: ${error.message}`); }
      };
      const host = page.locator('#S_Server1_Host');
      const input = page.locator('#S_Server1_Backbone');
      const menu = input.locator('xpath=..').locator('select.config-backbone__menu');
      const hint = input.locator('xpath=..').locator('.config-backbone__hint');
      const value = () => page.evaluate(() => Config.getOptionValue(Config.findOptionByName('Server1.Backbone')));

      await check('Group is gone and Backbone shows in Simple mode', async () => {
        assert.equal(await page.locator('#S_Server1_Group').count(), 0);
        assert.equal(await menu.isVisible(), true);
        const labels = await menu.locator('option').allTextContents();
        assert.equal(labels[0], 'Independent (shares articles with no other server)');
        assert.deepEqual(labels.slice(1, 5), ['Omicron', 'UsenetExpress', 'Giganews', 'Eweka Internet Services']);
        assert.equal(labels[labels.length - 1], 'Other (type a name)…');
      });

      await check('an old Group 3 is the backbone "Group 3"', async () => {
        assert.equal(await value(), 'Group 3');
        assert.equal(await menu.inputValue(), 'Group 3');
        assert.equal(await input.isVisible(), false);
        // a picked (or kept) backbone gets no "unknown" warning
        await page.waitForTimeout(300);
        assert.doesNotMatch((await hint.textContent()) || '', /Unknown backbone/);
      });

      await check('a confirmed host does not overwrite a backbone the user has', async () => {
        await host.fill('news.eweka.nl');
        await page.waitForFunction(() => /not the backbone picked here/.test($('#S_Server1_Backbone').siblings('.config-backbone__hint').text()), null, { timeout: 5000 });
        assert.equal(await value(), 'Group 3');
        await hint.locator('button', { hasText: 'Use Eweka Internet Services' }).click();
        assert.equal(await value(), 'Eweka Internet Services');
        assert.equal(await menu.inputValue(), 'Eweka Internet Services');
      });

      await check('an empty backbone is filled in from a confirmed host, and follows the host', async () => {
        await menu.selectOption('');
        assert.equal(await value(), '');
        await host.fill('news.easynews.com');
        await page.waitForFunction(() => Config.getOptionValue(Config.findOptionByName('Server1.Backbone')) === 'Omicron', null, { timeout: 5000 });
        assert.match(await hint.textContent(), /news\.easynews\.com reads from Omicron\. Filled in for you\./);
        // still the auto value: a new confirmed host replaces it
        await host.fill('news.eweka.nl');
        await page.waitForFunction(() => Config.getOptionValue(Config.findOptionByName('Server1.Backbone')) === 'Eweka Internet Services', null, { timeout: 5000 });
      });

      await check('a reseller of several backbones is only suggested', async () => {
        await menu.selectOption('');
        await host.fill('news.newsgroupdirect.com');
        await page.waitForFunction(() => /sells access to several backbones/.test($('#S_Server1_Backbone').siblings('.config-backbone__hint').text()), null, { timeout: 5000 });
        assert.equal(await value(), '', 'a suggestion is not filled in');
        assert.match(await hint.textContent(), /UsenetExpress, Giganews\. Pick the one your plan uses\./);
        await hint.locator('button', { hasText: 'Use Giganews' }).click();
        assert.equal(await value(), 'Giganews');
        assert.equal(await hint.isVisible(), false);
      });

      await check('an unknown host warns, and Other takes a name of your own', async () => {
        await menu.selectOption('');
        await host.fill('news.example.org');
        await page.waitForFunction(() => /Unknown backbone for news\.example\.org/.test($('#S_Server1_Backbone').siblings('.config-backbone__hint').text()), null, { timeout: 5000 });
        await menu.selectOption({ label: 'Other (type a name)…' });
        assert.equal(await input.isVisible(), true);
        await input.fill('My Proxy Backbone');
        await input.blur();
        assert.equal(await value(), 'My Proxy Backbone');
        await page.evaluate(() => Config.syncControls && Config.syncControls());
      });

      await check('Other keeps the menu on Other and the current name to edit', async () => {
        await menu.selectOption({ label: 'Other (type a name)…' });
        assert.equal(await menu.inputValue(), '\u0001custom');
        assert.equal(await input.isVisible(), true);
        assert.equal(await input.inputValue(), 'My Proxy Backbone');
        assert.equal(await value(), 'My Proxy Backbone');
        await input.blur();
        assert.equal(await menu.inputValue(), 'My Proxy Backbone');
      });

      await check('an IP address (a local proxy) gets no suggestion', async () => {
        await host.fill('127.0.0.1');
        await page.waitForTimeout(1200);
        assert.equal(await hint.isVisible(), false);
        assert.equal(await value(), 'My Proxy Backbone');
      });

      await check('saving writes Backbone and no Group', async () => {
        await page.evaluate(() => Config.saveChanges());
        for (let i = 0; i < 50 && !writes.some(x => x.method === 'saveconfig'); i++) await page.waitForTimeout(100);
        const save = writes.filter(w => w.method === 'saveconfig').pop();
        assert.ok(save, 'saveconfig was sent');
        const names = save.params[0].map(o => o.Name);
        assert.ok(names.includes('Server1.Backbone'));
        assert.ok(!names.includes('Server1.Group'));
        assert.equal(save.params[0].find(o => o.Name === 'Server1.Backbone').Value, 'My Proxy Backbone');
      });

      await check('opening Settings never fills a backbone left Independent', async () => {
        opened.host = 'news.eweka.nl';
        opened.group = '';
        await openServer();
        await page.waitForFunction(() => /reads from Eweka Internet Services\. Use it\?/.test($('#S_Server1_Backbone').siblings('.config-backbone__hint').text()), null, { timeout: 5000 });
        assert.equal(await value(), '');
        assert.equal(await menu.inputValue(), '');
        await hint.locator('button', { hasText: 'Use Eweka Internet Services' }).click();
        assert.equal(await value(), 'Eweka Internet Services');
      });

      await check('no page errors', async () => assert.deepEqual(errors, []));
      await context.close();
    }
  } finally {
    await browser.close();
  }
  if (failures.length) {
    console.error(failures.join('\n'));
    process.exit(1);
  }
  console.log(`settings-backbone: ${cases} cases passed`);
})();
