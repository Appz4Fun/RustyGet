// HTTPS and certificates: HTTPS shares the web port (no HTTPS port option), the
// certificate and key show only while "Use HTTPS" is on, a self-signed certificate
// can be made for the browsing host, a pasted certificate and key are saved, the
// pair is checked, Browse lists the daemon's own files, and the CA store for news
// servers sits in System with Upload, Paste and Check.
// Offline: the committed WebUI over synthetic RPC replies (details-fixture); the
// certificate RPCs are intercepted.
// Usage: node tests/webui/settings-https.js http://127.0.0.1:17999/
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

const MAIN = '/var/lib/rustyget';
const PAIR = { Ok: true, Subject: '127.0.0.1', Names: ['127.0.0.1'], NotAfter: 'Feb 17 04:35:00 2029 GMT', Expired: false,
  NotYetValid: false, SelfSigned: true, Chain: 1, KeyMatches: true, Error: '' };

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
      const errors = [], writes = [], calls = [];
      page.on('pageerror', e => errors.push(e.message));
      await installDetailsFixture(page, base, writes, { patchReply: (method, result) => {
        if (method === 'config' || method === 'loadconfig') {
          const set = (name, value) => { const o = result.find(x => x.Name === name); if (o) o.Value = value; else result.push({ Name: name, Value: value }); };
          set('SecureControl', 'no'); set('SecureCert', ''); set('SecureKey', ''); set('CertStore', '');
        }
        return result;
      } });
      await page.route(/\/jsonrpc/, async route => {
        const rpc = route.request().method() === 'POST' ? route.request().postDataJSON() : null;
        const reply = result => route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result } });
        if (!rpc) return route.fallback();
        const [a, b, c] = rpc.params || [];
        switch (rpc.method) {
          case 'checkcert':
            calls.push(['checkcert', a, b]);
            return reply(a.endsWith('bad.crt') ? { ...PAIR, Ok: false, KeyMatches: false } : PAIR);
          case 'makecert':
            calls.push(['makecert', a]);
            return reply({ Cert: MAIN + '/https/rustyget.crt', Key: MAIN + '/https/rustyget.key', Check: PAIR, Error: '' });
          case 'savepem':
            calls.push(['savepem', a, b, c]);
            if (a === 'ca') return reply({ Path: MAIN + '/certs/' + b.replace(/\.pem$/, '') + '.pem', Count: 2, Error: '' });
            return reply({ Cert: MAIN + '/https/server.crt', Key: MAIN + '/https/server.key', Check: PAIR, Error: '' });
          case 'checkcastore':
            calls.push(['checkcastore', a]);
            return reply(a ? { Count: 3, Files: 1, System: false, Error: '' } : { Count: 0, System: true, Error: '' });
          case 'browsefiles': {
            calls.push(['browsefiles', a]);
            const dir = a === '${MainDir}' || !a ? MAIN : a.replace(/\/[^/]*\.(crt|key)$/, '');
            const entries = dir === MAIN ? [{ Name: 'https', Dir: true, Size: 0, Readable: true }, { Name: 'secret', Dir: true, Size: 0, Readable: false }] :
              [{ Name: 'mine.crt', Dir: false, Size: 2048, Readable: true }, { Name: 'root.key', Dir: false, Size: 1700, Readable: false }];
            return reply({ Path: dir, Parent: dir === MAIN ? '/var/lib' : MAIN, Entries: entries, Truncated: false, Error: '' });
          }
          default: return route.fallback();
        }
      });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      await page.evaluate(() => $('a[href="#ConfigTab"]').first().click());
      await page.waitForFunction(() => document.querySelector('#ConfigMode'));
      await page.evaluate(() => Config.showSection('SECURITY', false));
      await page.waitForTimeout(200);
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${theme}/${width}/${label}: ${error.message}`); }
      };
      const S = name => page.locator('#S_' + name);
      const row = name => S(name).locator('xpath=ancestor::div[contains(@class,"control-group")][1]');
      const status = page.locator('#ConfigHttpsStatus');

      await check('no HTTPS port, and the certificate waits for Use HTTPS', async () => {
        assert.equal(await S('SecurePort').count(), 0, 'SecurePort is gone');
        const card = page.locator('#ConfigData .config-card.SECURITY', { has: S('SecureControl') });
        assert.match(await card.locator('.config-card__title').textContent(), /HTTPS/);
        assert.equal(await row('SecureCert').isVisible(), false);
        assert.equal(await row('SecureKey').isVisible(), false);
        assert.equal(await page.locator('#ConfigHttpsTools').isVisible(), false);
        await S('SecureControl').click();
        await page.waitForTimeout(200);
        assert(await row('SecureCert').isVisible(), 'certificate shows once HTTPS is on');
        assert(await row('SecureKey').isVisible());
        assert(await page.locator('#ConfigHttpsTools').isVisible());
        assert.match(await status.textContent(), /needs a certificate and its key/);
      });

      await check('a self-signed certificate is made for the browsing host', async () => {
        await page.locator('#ConfigHttpsMakeBtn').click();
        assert.equal(await page.locator('#ConfigHttpsNames').inputValue(), new URL(base.href).hostname);
        await page.locator('#ConfigHttpsNames').fill('127.0.0.1, nas.local');
        await page.locator('#ConfigHttpsMakeGo').click();
        await page.waitForFunction(() => /Save the settings/.test($('#ConfigHttpsStatus').text()), null, { timeout: 5000 });
        assert.deepEqual(calls.find(c => c[0] === 'makecert'), ['makecert', '127.0.0.1, nas.local']);
        assert.equal(await S('SecureCert').inputValue(), MAIN + '/https/rustyget.crt');
        assert.equal(await S('SecureKey').inputValue(), MAIN + '/https/rustyget.key');
        assert.equal(await status.getAttribute('data-state'), 'ok');
        assert.match(await status.textContent(), /self-signed/);
        assert.match(await status.textContent(), /key belongs/);
        assert.equal(await page.locator('#ConfigHttpsMake').isVisible(), false, 'the form closes');
      });

      await check('a pasted certificate and key are saved and used', async () => {
        await page.locator('#ConfigHttpsPasteBtn').click();
        await page.locator('#ConfigHttpsPem').fill('-----BEGIN CERTIFICATE-----\nAAA\n-----END CERTIFICATE-----\n');
        await page.locator('#ConfigHttpsPasteGo').click();
        await page.waitForFunction(() => /server\.crt$/.test($('#S_SecureCert').val()), null, { timeout: 5000 });
        const save = calls.find(c => c[0] === 'savepem' && c[1] === 'https');
        assert.match(save[3], /BEGIN CERTIFICATE/);
        assert.equal(await S('SecureKey').inputValue(), MAIN + '/https/server.key');
        assert.equal(await page.locator('#ConfigHttpsPem').inputValue(), '');
      });

      await check('typing a path checks the pair again', async () => {
        await S('SecureCert').fill('/etc/ssl/bad.crt');
        await page.waitForFunction(() => /does not belong/.test($('#ConfigHttpsStatus').text()), null, { timeout: 5000 });
        assert.equal(await status.getAttribute('data-state'), 'error');
        assert.deepEqual(calls[calls.length - 1], ['checkcert', '/etc/ssl/bad.crt', MAIN + '/https/server.key']);
      });

      await check('Browse lists the daemon\'s files and picks one', async () => {
        await row('SecureCert').locator('.config-cert-browse').click();
        const dialog = page.locator('#FileBrowserDialog');
        await dialog.waitFor({ state: 'visible' });
        assert.deepEqual(calls[calls.length - 1], ['browsefiles', '/etc/ssl/bad.crt']);
        await page.locator('#FileBrowserDialog_Up').click();
        await page.waitForFunction(m => $('#FileBrowserDialog_Path').val() === m, MAIN, { timeout: 5000 });
        assert.equal(await dialog.locator('li', { hasText: 'secret' }).getAttribute('aria-disabled'), 'true', 'unreadable folder');
        await dialog.locator('li', { hasText: 'https' }).click();
        await page.waitForFunction(() => $('#FileBrowserDialog_List li').text().indexOf('mine.crt') > -1, null, { timeout: 5000 });
        assert.equal(await dialog.locator('li', { hasText: 'root.key' }).getAttribute('aria-disabled'), 'true', 'unreadable file');
        await dialog.locator('li', { hasText: 'mine.crt' }).click();
        await page.locator('#FileBrowserDialog_Choose').click();
        await dialog.waitFor({ state: 'hidden' });
        assert.equal(await S('SecureCert').inputValue(), MAIN + '/https/mine.crt');
      });

      await check('certificate check and the CA store are in System', async () => {
        const card = page.locator('#ConfigData .config-card.SECURITY', { has: S('CertCheck') });
        assert.equal((await card.locator('.config-card__title').textContent()).trim(), 'System');
        assert.equal(await card.locator('#S_CertStore').count(), 1);
        await page.locator('#ConfigCaCheckBtn').click();
        await page.waitForFunction(() => /system's own/.test($('#ConfigCaStatus').text()), null, { timeout: 5000 });
        await page.locator('#ConfigCaFile').setInputFiles({ name: 'My CA.pem', mimeType: 'text/plain',
          buffer: Buffer.from('-----BEGIN CERTIFICATE-----\nBBB\n-----END CERTIFICATE-----\n') });
        await page.waitForFunction(() => /certs\//.test($('#S_CertStore').val()), null, { timeout: 5000 });
        assert.deepEqual(calls.find(c => c[0] === 'savepem' && c[1] === 'ca').slice(0, 3), ['savepem', 'ca', 'My CA.pem']);
        assert.match(await page.locator('#ConfigCaStatus').textContent(), /2 certificates saved/);
        await page.locator('#ConfigCaCheckBtn').click();
        await page.waitForFunction(() => /3 certificates found/.test($('#ConfigCaStatus').text()), null, { timeout: 5000 });
      });

      await check('turning HTTPS off hides the certificate again, and the save has no HTTPS port', async () => {
        await S('SecureControl').click();
        await page.waitForTimeout(200);
        assert.equal(await row('SecureCert').isVisible(), false);
        await page.evaluate(() => Config.saveChanges());
        await page.waitForFunction(n => n.some(w => w.method === 'saveconfig'), writes, { polling: 100, timeout: 5000 }).catch(() => {});
        await page.waitForTimeout(500);
        const save = writes.find(w => w.method === 'saveconfig');
        assert(save, 'saveconfig was sent');
        const values = Object.fromEntries(save.params[0].map(o => [o.Name, o.Value]));
        assert.equal(values.SecurePort, undefined);
        assert.equal(values.SecureCert, MAIN + '/https/mine.crt', 'a hidden value is still saved');
        assert.match(values.CertStore, /certs\/My CA\.pem$/);
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
  console.log(`settings-https: ${cases} cases passed`);
})();
