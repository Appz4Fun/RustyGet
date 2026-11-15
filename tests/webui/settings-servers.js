// News servers: a wide window edits a server in a right-hand pane beside the list,
// a narrow one gives it the page. "Add server" is only offered from the list. Save
// writes just the servers at once, finds a cipher when Cipher is Automatic, and
// reloads. Cipher is a menu with Optimize beside it and no "Server1." key.
// Offline: the committed WebUI over synthetic RPC replies (details-fixture);
// saveconfig, reload and the cipher optimizer are intercepted.
// Usage: node tests/webui/settings-servers.js http://127.0.0.1:17999/
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
      const wide = width >= 1180;
      const context = await browser.newContext({ viewport: { width, height } });
      await context.addInitScript(t => localStorage.setItem('Theme', t), theme);
      await context.addInitScript(() => localStorage.setItem('SettingsMode', 'expert'));
      const page = await context.newPage();
      const errors = [], writes = [], calls = [], configReads = [];
      let upTime = 86400, statusMisses = 0, hold = null;
      page.on('pageerror', e => errors.push(e.message));
      await installDetailsFixture(page, base, writes, { patchReply: (method, result) => {
        if (method === 'config' || method === 'loadconfig') {
          const set = (name, value) => { const o = result.find(x => x.Name === name); if (o) o.Value = value; else result.push({ Name: name, Value: value }); };
          set('Server1.Cipher', ''); set('Server1.Encryption', 'yes');
          set('Server2.Cipher', 'AES128-SHA');
        }
        if (method === 'status') result.UpTimeSec = upTime;
        if (method === 'config') configReads.push(Date.now());
        return result;
      }, holdMutation: async rpc => { if (rpc.method === 'saveconfig' && hold) await hold; } });
      await page.route(/\/jsonrpc/, async route => {
        const request = route.request();
        const rpc = request.method() === 'POST' ? request.postDataJSON() : null;
        if (!rpc || !['reload', 'tlscipherstart', 'tlscipherstatus'].includes(rpc.method)) return route.fallback();
        calls.push({ method: rpc.method, params: rpc.params });
        let result = true;
        if (rpc.method === 'reload') upTime = 3;
        if (rpc.method === 'tlscipherstart') result = { JobID: 'job1', State: 'probing', CompletedProbes: 1, TotalProbes: 9, Servers: [] };
        if (rpc.method === 'tlscipherstatus' && statusMisses > 0) {
          statusMisses--;
          return route.fulfill({ status: 500, body: 'busy' });
        }
        if (rpc.method === 'tlscipherstatus') {
          result = rpc.params && rpc.params[0] === 'job1' ? { JobID: 'job1', State: 'completed', Saved: true, Servers: [{
            ID: 1, Outcome: 'saved', SelectedCipher: 'TLS_AES_128_GCM_SHA256', OldCipher: '',
            SavedHost: await page.evaluate(() => $('#S_Server1_Host').val()), SavedPort: '563' }] } : {};
        }
        return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result } });
      });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      await page.evaluate(() => $('a[href="#ConfigTab"]').first().click());
      await page.waitForFunction(() => document.querySelector('#ConfigMode'));
      await page.evaluate(() => Config.showSection('NEWS-SERVERS', false));
      await page.waitForTimeout(200);
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${theme}/${width}/${label}: ${error.message}`); }
      };
      const edit = n => page.locator(`.config-list-row.NEWS-SERVERS.multiid${n} .config-list-edit`);
      const add = page.locator('#ConfigData .config-add.NEWS-SERVERS');
      const pane = page.locator('#ConfigServerPane');

      await check(wide ? 'a server opens in a pane beside the list' : 'a server gets the page', async () => {
        assert(await add.isVisible(), 'Add server is offered from the list');
        await edit(1).click();
        await page.waitForTimeout(150);
        assert.equal(await add.isVisible(), false, 'but not while a server is open');
        if (wide) {
          assert(await pane.isVisible());
          assert.equal((await pane.locator('.config-pane__title').textContent()).trim(), 'Primary provider');
          assert.equal(await pane.locator('#S_Server1_Host').count(), 1, 'the editor is in the pane');
          assert(await edit(2).isVisible(), 'the list stays in view');
          const box = await pane.boundingBox(), row = await edit(2).boundingBox();
          assert(box.x > row.x + row.width - 1, 'the pane is to the right of the list');
        } else {
          assert.equal(await pane.count(), 0);
          assert.equal(await edit(2).isVisible(), false, 'other servers wait on the list');
          assert(await page.locator('#S_Server1_Host').isVisible());
          assert.equal((await edit(1).textContent()).trim(), 'Back to servers');
        }
      });

      await check('cipher is a menu with Optimize and no Server1 key', async () => {
        const label = page.locator('#label\\:S_Server1_Cipher');
        assert.equal((await label.textContent()).trim(), 'Cipher');
        assert.equal(await label.locator('.config-key').count(), 0);
        const menu = page.locator('#S_Server1_Cipher').locator('xpath=..').locator('select.config-cipher__menu');
        assert.equal(await menu.inputValue(), '');
        const values = await menu.locator('option').evaluateAll(o => o.map(x => x.value));
        assert(values.includes('TLS_AES_128_GCM_SHA256') && values.includes('ECDHE-RSA-AES128-GCM-SHA256'));
        assert(await page.locator('#S_Server1_Cipher').locator('xpath=..').locator('.config-cipher__optimize').isVisible());
        await menu.selectOption('TLS_CHACHA20_POLY1305_SHA256');
        assert.equal(await page.locator('#S_Server1_Cipher').inputValue(), 'TLS_CHACHA20_POLY1305_SHA256');
        await menu.selectOption('');
        const other = await page.evaluate(() => $('#S_Server2_Cipher').siblings('select').val());
        assert.equal(other, 'AES128-SHA', 'a saved cipher outside the list is kept');
      });

      await check('save writes only the servers, finds a cipher, then reloads', async () => {
        await page.evaluate(() => { $('#S_ArticleRetries').val('7').trigger('change'); });
        await page.evaluate(() => Config.addSet('Category', 'CATEGORIES'));
        await page.waitForTimeout(700);
        statusMisses = 2;
        const readsBefore = configReads.length;
        await page.locator('#S_Server1_Host').fill('news2.example.invalid');
        await page.locator('#S_Server1_Host').dispatchEvent('change');
        await page.locator('.config-server-save:visible').click();
        await page.waitForFunction(() => /in use/.test($('.config-server-status:visible').text()), null, { timeout: 8000 });
        const save = writes.filter(w => w.method === 'saveconfig').pop();
        const values = Object.fromEntries(save.params[0].map(o => [o.Name, o.Value]));
        assert.equal(values['Server1.Host'], 'news2.example.invalid');
        assert.notEqual(values.ArticleRetries, '7', 'an unsaved change elsewhere is not written');
        const categories = Object.keys(values).filter(n => /^Category\d+\.Name$/.test(n)).length;
        const saved = await page.evaluate(() => Config.config().values.filter(v => /^Category\d+\.Name$/.test(v.Name)).length);
        assert.equal(categories, saved, 'an unsaved new category is not written');
        assert(configReads.length > readsBefore, 'the running configuration is read again after the reload');
        assert(values._TlsCipherBaseline, 'the cipher baseline guards the write');
        const start = calls.find(c => c.method === 'tlscipherstart');
        assert.deepEqual(start && start.params, ['1'], 'the optimizer runs for this server only');
        assert(calls.findIndex(c => c.method === 'reload') > calls.findIndex(c => c.method === 'tlscipherstart'), 'the reload comes after the cipher');
        assert.equal(await page.locator('#S_Server1_Cipher').inputValue(), 'TLS_AES_128_GCM_SHA256');
        assert.match(await page.locator('.config-server-status:visible').textContent(), /TLS_AES_128_GCM_SHA256/);
        assert.equal(await page.evaluate(() => $('#ConfigSaveBar .config-savebar__names').text()).then(t => /Host|Cipher/.test(t)), false,
          'the saved server is no longer an unsaved change');
        assert(await page.locator('#ConfigSaveBar').isVisible(), 'the other change is still waiting');
      });

      await check('an edit made while saving stays unsaved', async () => {
        let release;
        hold = new Promise(resolve => { release = resolve; });
        upTime = 86400;
        await page.evaluate(() => { Status.status.UpTimeSec = 86400; });
        await page.locator('#S_Server1_Connections').fill('11');
        await page.locator('#S_Server1_Connections').dispatchEvent('change');
        await page.locator('.config-server-save:visible').click();
        await page.waitForTimeout(200);
        await page.locator('#S_Server1_Connections').fill('12');
        await page.locator('#S_Server1_Connections').dispatchEvent('change');
        hold = null;
        release();
        await page.waitForFunction(() => /in use/.test($('.config-server-status:visible').text()), null, { timeout: 8000 });
        assert.match(await page.evaluate(() => $('#ConfigSaveBar .config-savebar__names').text()), /Connections/, 'the later value is still unsaved');
        assert.equal(await page.evaluate(() => Config.config().values.find(v => v.Name === 'Server1.Connections').Value), '11');
        await page.locator('#S_Server1_Connections').fill('11');
        await page.locator('#S_Server1_Connections').dispatchEvent('change');
      });

      await check('a server with a cipher saves without testing', async () => {
        const before = calls.filter(c => c.method === 'tlscipherstart').length;
        upTime = 86400;
        await page.evaluate(() => { Status.status.UpTimeSec = 86400; });
        await page.locator('#S_Server1_Name').fill('Main');
        await page.locator('#S_Server1_Name').dispatchEvent('change');
        await page.locator('.config-server-save:visible').click();
        await page.waitForFunction(() => /in use/.test($('.config-server-status:visible').text()), null, { timeout: 8000 });
        assert.equal(calls.filter(c => c.method === 'tlscipherstart').length, before);
        if (wide) assert.equal((await pane.locator('.config-pane__title').textContent()).trim(), 'Main');
      });

      await check('closing returns the server to the list, and Add opens the new one', async () => {
        if (wide) await pane.locator('.config-pane__close').click();
        else await edit(1).click();
        await page.waitForTimeout(150);
        assert.equal(await pane.count(), 0);
        assert(await add.isVisible());
        assert.equal(await page.locator('#S_Server1_Host').isVisible(), false);
        assert.equal(await page.evaluate(() => $('.config-list-row.NEWS-SERVERS.multiid1').next().is('.config-list-detail.multiid1')), true,
          'the editor is back after its row');
        await add.click();
        await page.waitForTimeout(700);
        assert.equal(await add.isVisible(), false);
        if (wide) assert.equal(await pane.locator('#S_Server3_Host').count(), 1, 'the new server opens in the pane');
        else assert(await page.locator('#S_Server3_Host').isVisible());
        await page.evaluate(() => Config.closeServerEditor());
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
  console.log(`settings-servers: ${cases} cases passed`);
})();
