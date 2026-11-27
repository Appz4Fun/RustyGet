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
      let upTime = 86400, statusMisses = 0, hold = null, startBusy = 0, probeFails = false;
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
        if (rpc.method === 'tlscipherstart' && startBusy > 0) {
          startBusy--;
          return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, error: { name: 'JsonRpcError', code: 1,
            message: 'cipher testing is already running for other servers; wait for it to finish' } } });
        }
        if (rpc.method === 'tlscipherstart') result = { JobID: 'job1', State: 'probing', CompletedProbes: 1, TotalProbes: 9, Servers: [] };
        if (rpc.method === 'tlscipherstatus' && statusMisses > 0) {
          statusMisses--;
          return route.fulfill({ status: 500, body: 'busy' });
        }
        if (rpc.method === 'tlscipherstatus' && probeFails) {
          result = { JobID: 'job1', State: 'completed', Saved: false, Message: 'Tests completed; no cipher settings were changed.',
            Servers: [{ ID: 1, Outcome: 'unchanged', SelectedCipher: '', OldCipher: '', Message: 'DNS resolution failed.' }] };
        } else if (rpc.method === 'tlscipherstatus') {
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
        assert.equal(await page.evaluate(() => $('#S_Server1_Cipher').siblings('select').val()), 'TLS_AES_128_GCM_SHA256',
          'the cipher menu shows the found cipher');
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

      // A cipher found for one host says nothing about another: a changed host
      // clears it, tests again and applies; a failed test is shown and keeps the default.
      const saveHost = async host => {
        upTime = 86400;
        await page.evaluate(() => { Status.status.UpTimeSec = 86400; });
        await page.locator('#S_Server1_Host').fill(host);
        await page.locator('#S_Server1_Host').dispatchEvent('change');
        const starts = calls.filter(c => c.method === 'tlscipherstart').length;
        await page.locator('.config-server-save:visible').click();
        await page.waitForFunction(() => /in use|reload/i.test($('.config-server-status:visible').text()) &&
          !/Putting it to use/.test($('.config-server-status:visible').text()), null, { timeout: 8000 });
        const save = writes.filter(w => w.method === 'saveconfig').pop();
        return { values: Object.fromEntries(save.params[0].map(o => [o.Name, o.Value])),
          started: calls.filter(c => c.method === 'tlscipherstart').length - starts };
      };
      await check('a changed host tests the cipher again and applies it', async () => {
        assert.equal(await page.locator('#S_Server1_Cipher').inputValue(), 'TLS_AES_128_GCM_SHA256');
        const reloads = calls.filter(c => c.method === 'reload').length;
        const { values, started } = await saveHost('news3.example.invalid');
        assert.equal(values['Server1.Cipher'], '', 'the old host\'s cipher is not carried over');
        assert.equal(started, 1, 'the optimizer runs again for the moved server');
        assert.equal(calls.filter(c => c.method === 'reload').length, reloads + 1, 'and the result is put to use');
        assert.equal(await page.locator('#S_Server1_Cipher').inputValue(), 'TLS_AES_128_GCM_SHA256');
      });
      await check('a failed automatic test is shown and keeps the system default', async () => {
        probeFails = true;
        try {
          const { values, started } = await saveHost('news4.example.invalid');
          assert.equal(started, 1);
          assert.equal(values['Server1.Cipher'], '');
          assert.match(await page.locator('.config-server-status:visible').textContent(),
            /Cipher test failed: DNS resolution failed\. The system default cipher is used\./);
          assert.equal(await page.locator('#S_Server1_Cipher').inputValue(), '');
        } finally { probeFails = false; }
      });
      await check('a cipher chosen with the new host is kept as chosen', async () => {
        await page.evaluate(() => $('#S_Server1_Cipher').siblings('select').val('TLS_AES_256_GCM_SHA384').trigger('change'));
        await page.locator('#S_Server1_Host').fill('news5.example.invalid');
        await page.locator('#S_Server1_Host').dispatchEvent('change');
        const { values, started } = await saveHost('news5.example.invalid');
        assert.equal(values['Server1.Cipher'], 'TLS_AES_256_GCM_SHA384');
        assert.equal(started, 0, 'a cipher picked by hand is not tested over');
      });

      await check('Save and reload waits while a server is being saved', async () => {
        let release;
        hold = new Promise(resolve => { release = resolve; });
        upTime = 86400;
        await page.evaluate(() => { Status.status.UpTimeSec = 86400; });
        await page.evaluate(() => { $('#S_ArticleRetries').val('9').trigger('change'); });
        await page.locator('#S_Server1_Name').fill('Racing');
        await page.locator('#S_Server1_Name').dispatchEvent('change');
        await page.locator('.config-server-save:visible').click();
        await page.waitForTimeout(200);
        const saves = writes.filter(w => w.method === 'saveconfig').length;
        assert(await page.evaluate(() => $('#ConfigSaveBar_Save').prop('disabled')), 'the page-wide save is off');
        await page.evaluate(() => Config.saveChanges());
        await page.waitForTimeout(300);
        assert.equal(writes.filter(w => w.method === 'saveconfig').length, saves, 'no second write while the server saves');
        assert.match(await page.locator('.config-server-status:visible').textContent(), /still being saved/);
        hold = null;
        release();
        await page.waitForFunction(() => /in use/.test($('.config-server-status:visible').text()), null, { timeout: 8000 });
        assert.equal(await page.evaluate(() => $('#ConfigSaveBar_Save').prop('disabled')), false, 'the page-wide save is back');
        assert.match(await page.evaluate(() => $('#ConfigSaveBar .config-savebar__names').text()), /retries/i, 'the other change is still waiting');
        await page.evaluate(() => { $('#S_ArticleRetries').val(Config.config().values.find(v => v.Name === 'ArticleRetries').Value).trigger('change'); });
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

      await check('another browser\'s cipher test is waited for, not reloaded under', async () => {
        startBusy = 2;
        upTime = 86400;
        await page.evaluate(() => { Status.status.UpTimeSec = 86400; });
        const reloads = calls.filter(c => c.method === 'reload').length;
        const starts = calls.filter(c => c.method === 'tlscipherstart').length;
        await page.locator('.config-cipher__optimize:visible').click();
        await page.waitForFunction(() => /Waiting for another cipher test/.test($('.config-server-status:visible').text()), null, { timeout: 5000 });
        assert.equal(calls.filter(c => c.method === 'reload').length, reloads, 'no reload while the other test runs');
        await page.waitForFunction(() => /in use/.test($('.config-server-status:visible').text()), null, { timeout: 12000 });
        assert.equal(calls.filter(c => c.method === 'tlscipherstart').length, starts + 3, 'it tries again until the other test ends');
        const reload = calls.findIndex((c, i) => i > 0 && c.method === 'reload' && calls.slice(0, i).filter(x => x.method === 'reload').length === reloads);
        const lastStart = calls.map(c => c.method).lastIndexOf('tlscipherstart');
        assert(reload > lastStart, 'the reload comes after its own cipher test');
      });

      if (wide) await check('a search link to a server option scrolls the pane', async () => {
        await page.evaluate(() => $('.config-pane__body').scrollTop(0));
        await page.evaluate(() => {
          const a = $('<a data-optid="S_Server1_Connections"></a>')[0];
          Config.scrollToOption({ preventDefault() {} }, a);
        });
        await page.waitForTimeout(1200);
        const box = await page.evaluate(() => {
          const body = $('#ConfigServerPane .config-pane__body')[0].getBoundingClientRect();
          const el = $('#S_Server1_Connections')[0].getBoundingClientRect();
          return { top: el.top, bottom: el.bottom, paneTop: body.top, paneBottom: body.bottom, scrolled: $('#ConfigServerPane .config-pane__body').scrollTop() };
        });
        assert(box.scrolled > 0, 'the pane scrolled');
        assert(box.top >= box.paneTop && box.bottom <= box.paneBottom, `Connections is inside the pane (${JSON.stringify(box)})`);
      });

      await check('a link to Cipher scrolls to its menu', async () => {
        const scroller = wide ? '#ConfigServerPane .config-pane__body' : '.config__main';
        await page.evaluate(s => { $(s).scrollTop(0); $('html').scrollTop(0); }, scroller);
        await page.evaluate(() => Config.scrollToOption({ preventDefault() {} }, $('<a data-optid="S_Server1_Cipher"></a>')[0]));
        await page.waitForTimeout(1200);
        const box = await page.evaluate(() => {
          const el = $('#S_Server1_Cipher').siblings('select.config-cipher__menu')[0].getBoundingClientRect();
          return { top: el.top, bottom: el.bottom, height: window.innerHeight };
        });
        assert(box.top >= 0 && box.bottom <= box.height, `the cipher menu is on screen (${JSON.stringify(box)})`);
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

      await check('leaving Settings while a server saves still puts it to use', async () => {
        await edit(1).click();
        await page.waitForTimeout(150);
        let release;
        hold = new Promise(resolve => { release = resolve; });
        upTime = 86400;
        await page.evaluate(() => { Status.status.UpTimeSec = 86400; });
        const reloads = calls.filter(c => c.method === 'reload').length;
        await page.locator('#S_Server1_Name').fill('Leaving');
        await page.locator('#S_Server1_Name').dispatchEvent('change');
        await page.locator('.config-server-save:visible').click();
        await page.waitForTimeout(200);
        await page.evaluate(() => Config.hide());
        hold = null;
        release();
        await page.waitForTimeout(1500);
        assert.equal(calls.filter(c => c.method === 'reload').length, reloads + 1, 'the saved file is put to use');
        await page.evaluate(() => { Config.show(); Config.shown(); });
        await page.waitForFunction(() => Config.config() && document.querySelector('.config-list-row.NEWS-SERVERS'), null, { timeout: 8000 });
        await page.evaluate(() => Config.showSection('NEWS-SERVERS', false));
        await page.waitForTimeout(200);
        await edit(1).click();
        await page.waitForTimeout(150);
        assert.equal(await page.locator('.config-server-save:visible').isDisabled(), false, 'saving works again');
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
