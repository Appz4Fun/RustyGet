// Offline candidate assets and intercepted optimizer RPC; never a live target.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { fixtureURL } = require('./workspace-fixture');

(async () => {
  const base = fixtureURL(process.argv[2]);
  const root = path.resolve(__dirname, '../../webui');
  assert(fs.existsSync(path.join(root, 'tlscipher.js')), 'WebUI must provide the TLS cipher dialog module');
  const { chromium } = require('playwright');
  const html = fs.readFileSync(path.join(root, 'index.html'), 'utf8')
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '');
  const browser = await chromium.launch({ channel: 'chrome' });
  try {
    for (const theme of ['light', 'dark']) for (const width of [1280, 390, 320]) {
      const context = await browser.newContext({ viewport: { width, height: 900 } });
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.route('**/*', route => {
        const url = new URL(route.request().url());
        if (url.origin !== base.origin) return route.abort();
        if (url.pathname === '/') return route.fulfill({ contentType: 'text/html', body: html });
        if (url.pathname === '/combined.css') {
          return route.fulfill({ contentType: 'text/css', body: ['lib/bootstrap.css', 'style.css', 'tokens.css', 'ds.css', 'skin.css']
            .map(file => fs.readFileSync(path.join(root, file), 'utf8')).join('\n') });
        }
        const file = path.resolve(root, '.' + decodeURIComponent(url.pathname));
        if (!file.startsWith(root + path.sep) || !fs.existsSync(file) || !fs.statSync(file).isFile()) return route.abort();
        return route.fulfill({ path: file });
      });
      await page.goto(base.href);
      for (const file of ['lib/jquery.min.js', 'lib/bootstrap.min.js', 'i18n.js']) await page.addScriptTag({ path: path.join(root, file) });
      await page.evaluate(() => new Promise(resolve => I18n.whenReady(resolve)));
      await page.addScriptTag({ path: path.join(root, 'util.js') });
      await page.addScriptTag({ path: path.join(root, 'tlscipher.js') });
      await page.evaluate(({ theme, width }) => {
        document.documentElement.setAttribute('data-theme', theme);
        document.body.classList.toggle('phone', width < 768);
        window.UISettings = { setFocus: true };
        window.cipherCalls = [];
        window.appliedCipherResults = [];
        window.allowCipherStart = true;
        window.cipherSnapshot = 'unchanged browser server values';
        window.Config = {
          cipherSnapshot: () => window.allowCipherStart ? window.cipherSnapshot : null,
          applyCipherResults: (result, snapshot) => {
            window.appliedCipherResults.push({ result, snapshot });
            return window.cipherSnapshot === snapshot;
          }
        };
        window.cipherState = { Available: true, JobID: '', State: 'idle', Algorithms: [], Servers: [], Saved: false, ReloadRequired: false };
        window.Status = { status: { Instance: 1, UpTimeSec: 100, PostJobCount: 0 } };
        window.statusReplies = [];
        window.nextStart = null;
        RPC.call = (method, params, done, fail) => {
          window.cipherCalls.push({ method, params });
          if (method === 'reload') return done(true);
          if (method === 'status') return done(window.statusReplies.length > 1 ? window.statusReplies.shift() : window.statusReplies[0]);
          if (method === 'tlscipherstart' && window.nextStart) window.cipherState = window.nextStart(params);
          else if (method === 'tlscipherstart') window.cipherState = { Available: true, JobID: 'example-job', State: 'measuring',
            Message: '', Saved: false, ReloadRequired: false, Algorithms: [], Servers: [], CommonSupported: null,
            CurrentAlgorithm: 'AES-128-GCM', ExcludedSuites: ['PSK-EXAMPLE'] };
          if (method === 'tlsciphercancel') window.cipherState = { ...window.cipherState, State: 'cancelled' };
          done(structuredClone(window.cipherState));
        };
        TlsCipherDialog.init();
      }, { theme, width });

      // Initial action starts once; reopening the active job only observes it.
      await page.evaluate(() => { TlsCipherDialog.showModal(); TlsCipherDialog.showModal(); });
      const dialog = page.locator('#TlsCipherDialog');
      await dialog.waitFor({ state: 'visible' });
      assert.equal(await page.evaluate(() => cipherCalls.filter(x => x.method === 'tlscipherstart').length), 1);
      assert(await page.locator('#TlsCipherCancel').isEnabled());
      assert.match(await page.locator('#TlsCipherProgress').innerText(), /AES-128-GCM/);
      await page.locator('#TlsCipherExcluded summary').click();
      assert.match(await page.locator('#TlsCipherExcluded p').innerText(), /PSK-EXAMPLE/);
      await page.locator('#TlsCipherClose').click();
      await dialog.waitFor({ state: 'hidden' });
      await page.evaluate(() => TlsCipherDialog.showModal());
      await dialog.waitFor({ state: 'visible' });
      assert.equal(await page.evaluate(() => cipherCalls.filter(x => x.method === 'tlscipherstart').length), 1);

      await page.evaluate(() => {
        window.cipherState = { Available: true, JobID: 'example-job', State: 'completed',
          Message: 'Saved the fastest supported cipher for each tested server.', Saved: true, ReloadRequired: true,
          Algorithms: [{ Algorithm: 'AES-128-GCM', BytesPerSecond: 3e9, Minimum: 2.95e9, Maximum: 3.05e9 },
            { Algorithm: 'CHACHA20-POLY1305', BytesPerSecond: 2e9, Minimum: 1.95e9, Maximum: 2.05e9 }],
          CommonSupported: null,
          CipherSuites: ['TLS_AES_128_GCM_SHA256', 'TLS_CHACHA20_POLY1305_SHA256'],
          Servers: [
            { ID: 1, Name: 'Example Primary <img src=x onerror=alert(1)>', OldCipher: '', SelectedCipher: 'TLS_AES_128_GCM_SHA256', Outcome: 'saved', Message: '', CipherSupport: [true, false] },
            { ID: 2, Name: 'Example Backup', OldCipher: '', SelectedCipher: 'TLS_CHACHA20_POLY1305_SHA256', Outcome: 'saved', Message: '' },
            { ID: 3, Name: 'Example Unavailable', OldCipher: 'Existing cipher', SelectedCipher: '', Outcome: 'unchanged', Message: 'Certificate <script>untrusted</script>' }
          ] };
      });
      await page.waitForFunction(() => document.querySelector('#TlsCipherState').textContent.includes('Saved'));
      // Partial measurements remain diagnostics, never a displayed speed range.
      await page.evaluate(() => {
        cipherState.Algorithms.push({ Algorithm: 'Failed example', BytesPerSecond: null,
          Minimum: 1e9, Maximum: 2e9, Message: 'Authenticated record validation failed.' });
        TlsCipherDialog.showModal();
      });
      await page.waitForFunction(() => document.querySelector('#TlsCipherAlgorithms').textContent.includes('Failed example'));
      const failed = page.locator('#TlsCipherAlgorithms tbody tr').last();
      assert.match(await failed.innerText(), /Authenticated record validation failed/);
      assert.doesNotMatch(await failed.innerText(), /1000\.0|2000\.0/);
      assert.equal(await dialog.locator('img,script').count(), 0, 'Provider data stays text');
      assert.match(await page.locator('#TlsCipherServers').innerText(), /Example Unavailable/);
      assert.match(await page.locator('#TlsCipherReload').innerText(), /reload/i);
      assert.equal(await page.evaluate(() => appliedCipherResults.length), 1, 'Apply committed results once');
      await page.locator('#TlsCipherServers details summary').first().click();
      assert.match(await page.locator('#TlsCipherServers details ul').first().innerText(), /TLS_AES_128_GCM_SHA256: Supported/);
      assert.match(await page.locator('#TlsCipherServers details ul').first().innerText(), /TLS_CHACHA20_POLY1305_SHA256: Unsupported/);
      await page.locator('#TlsCipherServers').evaluate(element => { element.scrollLeft = element.scrollWidth; });
      const bounds = await dialog.boundingBox();
      assert(bounds.x >= -1 && bounds.x + bounds.width <= width + 1, 'Dialog stays in the viewport');
      const last = dialog.locator('#TlsCipherServers tbody tr').last().locator('td').last();
      await last.scrollIntoViewIfNeeded();
      assert(await last.isVisible(), 'Last outcome remains accessible');
      if (width === 1280) {
        // A long outcome wraps, so its ending is readable without scrolling.
        await page.evaluate(() => {
          cipherState.Servers[2].Message = 'Certificate verification failed. Check the CertCheck and CertStore options.';
          TlsCipherDialog.showModal();
        });
        await page.waitForFunction(() => document.querySelector('#TlsCipherServers').textContent.includes('CertStore options'));
        assert(await page.locator('#TlsCipherServers').evaluate(element => element.scrollWidth <= element.clientWidth), 'Outcome text wraps');
      }
      if (process.env.NZBGET_UI_CAPTURE_DIR && [1280, 390].includes(width)) {
        // Keep injection assertions above; use ordinary sanitized labels in pictures.
        await page.evaluate(() => {
          cipherState.Servers[0].Name = 'Example Primary';
          cipherState.Servers[2].Message = 'Certificate verification failed; settings retained.';
          TlsCipherDialog.showModal();
          document.querySelector('#TlsCipherServers').scrollLeft = 0;
          document.querySelector('#TlsCipherAlgorithms').scrollLeft = 0;
        });
        await page.evaluate(() => document.fonts.ready);
        await page.screenshot({ path: path.join(process.env.NZBGET_UI_CAPTURE_DIR, `tls-cipher-results-${theme}-${width}.jpg`), type: 'jpeg', quality: 90 });
      }

      // A failed (inconclusive) server offers Retry; a saved one does not. The
      // retried server's single-server job is merged into the earlier rows.
      await page.evaluate(() => { cipherState.Servers[0].Name = 'Example Primary'; TlsCipherDialog.showModal(); });
      await page.waitForFunction(() => document.querySelectorAll('#TlsCipherServers .tls-cipher-retry').length === 1);
      const retry = page.locator('#TlsCipherServers tbody tr').nth(2).locator('.tls-cipher-retry');
      assert.match(await retry.getAttribute('aria-label'), /Retry: Example Unavailable/);
      await page.evaluate(() => {
        nextStart = params => ({ Available: true, JobID: 'retry-job', State: 'completed', Message: 'Saved.', Saved: true,
          ReloadRequired: true, Algorithms: [], CommonSupported: ['TLS_AES_128_GCM_SHA256'], CipherSuites: [],
          Servers: [{ ID: Number(params[0]), Name: 'Example Unavailable', OldCipher: 'Existing cipher',
            SelectedCipher: 'TLS_AES_128_GCM_SHA256', Outcome: 'saved', Message: 'Saved; reload required.' }] });
      });
      await retry.scrollIntoViewIfNeeded();
      await retry.click();
      await page.waitForFunction(() => /Saved; reload required/.test(document.querySelector('#TlsCipherServers tbody tr:nth-child(3)').textContent));
      assert.deepEqual(await page.evaluate(() => cipherCalls.filter(x => x.method === 'tlscipherstart').slice(-1)[0].params), ['3']);
      assert.equal(await page.locator('#TlsCipherServers tbody tr').count(), 3, 'Retried row merged into the earlier results');
      assert.match(await page.locator('#TlsCipherServers tbody tr').first().innerText(), /Example Primary/);
      assert.equal(await page.locator('#TlsCipherServers .tls-cipher-retry').count(), 0);
      assert.match(await page.locator('#TlsCipherCommon').innerText(), /not fully established/, 'One server is no common set');
      await page.evaluate(() => { nextStart = null; });

      // Apply now asks first, then reloads and reports the unpack wait and the result.
      const apply = page.locator('#TlsCipherApply');
      assert(await apply.isVisible(), 'Saved ciphers can be applied from the dialog');
      await apply.click();
      assert.match(await apply.textContent(), /Reload now/);
      assert.match(await page.locator('#TlsCipherReload').innerText(), /unpack is running, the reload waits/);
      assert.equal(await page.evaluate(() => cipherCalls.filter(x => x.method === 'reload').length), 0, 'Nothing reloads before confirming');
      await page.evaluate(() => {
        statusReplies = [{ Instance: 1, UpTimeSec: 101, PostJobCount: 1 }, { Instance: 1, UpTimeSec: 102, PostJobCount: 1 },
          { Instance: 2, UpTimeSec: 1, PostJobCount: 0 }];
      });
      await apply.click();
      await page.waitForFunction(() => /current unpack to finish/.test(document.querySelector('#TlsCipherReload').textContent));
      assert(await apply.isHidden());
      await page.waitForFunction(() => /Applied/.test(document.querySelector('#TlsCipherReload').textContent), null, { timeout: 15000 });
      assert.equal(await page.evaluate(() => cipherCalls.filter(x => x.method === 'reload').length), 1);
      assert(await apply.isHidden(), 'Applied ciphers need no second reload');
      if (process.env.NZBGET_UI_CAPTURE_DIR && [1280, 390].includes(width)) {
        await page.screenshot({ path: path.join(process.env.NZBGET_UI_CAPTURE_DIR, `tls-cipher-applied-${theme}-${width}.jpg`), type: 'jpeg', quality: 90 });
      }

      // Ten servers in a short window: the body scrolls, the footer stays on screen.
      await page.setViewportSize({ width, height: 560 });
      await page.evaluate(() => {
        cipherState = { ...cipherState, JobID: 'ten-job', State: 'completed', Saved: true, ReloadRequired: true,
          Servers: Array.from({ length: 10 }, (_, i) => ({ ID: i + 1, Name: 'Example Server ' + (i + 1), OldCipher: '',
            SelectedCipher: i === 4 ? '' : 'TLS_AES_128_GCM_SHA256', Outcome: i === 4 ? 'unchanged' : 'saved',
            Message: i === 4 ? 'DNS resolution failed.' : 'Saved; reload required.' })) };
        TlsCipherDialog.showModal();
      });
      await page.waitForFunction(() => document.querySelectorAll('#TlsCipherServers tbody tr').length === 10);
      for (const id of ['#TlsCipherApply', '#TlsCipherClose']) {
        const box = await page.locator(id).boundingBox();
        assert(box && box.y >= 0 && box.y + box.height <= 560 + 1, id + ' stays inside a short window');
      }
      const frame = await dialog.boundingBox();
      assert(frame.y >= -1 && frame.y + frame.height <= 560 + 1, 'Dialog fits a short window');
      assert(await page.locator('#TlsCipherDialog .modal-body').evaluate(body => body.scrollHeight > body.clientHeight), 'The body scrolls');
      const lastRow = dialog.locator('#TlsCipherServers tbody tr').last();
      await lastRow.scrollIntoViewIfNeeded();
      assert(await lastRow.isVisible());
      if (process.env.NZBGET_UI_CAPTURE_DIR && [1280, 390].includes(width)) {
        await page.evaluate(() => { document.querySelector('#TlsCipherDialog .modal-body').scrollTop = 0; });
        await page.screenshot({ path: path.join(process.env.NZBGET_UI_CAPTURE_DIR, `tls-cipher-ten-servers-${theme}-${width}.jpg`), type: 'jpeg', quality: 90 });
      }
      await page.setViewportSize({ width, height: 900 });

      // A start guard must prevent any work when server edits are unsaved.
      await page.locator('#TlsCipherClose').click();
      await dialog.waitFor({ state: 'hidden' });
      await page.evaluate(() => { window.allowCipherStart = false; window.cipherState.State = 'idle'; TlsCipherDialog.showModal(); });
      await dialog.waitFor({ state: 'visible' });
      assert.match(await page.locator('#TlsCipherState').innerText(), /save.*server/i);
      assert.equal(await page.evaluate(() => cipherCalls.filter(x => x.method === 'tlscipherstart').length), 2, 'the earlier Retry was the second start');

      // A fresh run can be cancelled, and an unavailable backend cannot start one.
      await page.locator('#TlsCipherClose').press('Enter');
      await dialog.waitFor({ state: 'hidden' });
      await page.evaluate(() => { allowCipherStart = true; cipherState.State = 'idle'; TlsCipherDialog.showModal(); });
      await dialog.waitFor({ state: 'visible' });
      await page.locator('#TlsCipherCancel').click();
      await page.waitForFunction(() => document.querySelector('#TlsCipherState').textContent.toLowerCase().includes('cancelled'));
      assert.equal(await page.evaluate(() => cipherCalls.filter(x => x.method === 'tlsciphercancel').length), 1);
      assert(await page.locator('#TlsCipherCancel').isDisabled());
      await page.locator('#TlsCipherClose').click();
      await dialog.waitFor({ state: 'hidden' });
      await page.evaluate(() => {
        cipherState = { Available: false, State: 'unavailable', Message: 'TLS cipher optimization requires a TLS-enabled Rust backend.' };
        TlsCipherDialog.showModal();
        jQuery('<button id="Config_OptimizeTls">').appendTo(document.body);
        TlsCipherDialog.refreshAvailability();
      });
      await dialog.waitFor({ state: 'visible' });
      assert(await page.locator('#Config_OptimizeTls').isDisabled());
      assert.match(await page.locator('#TlsCipherState').innerText(), /requires a TLS-enabled Rust backend/);
      assert.equal(await page.evaluate(() => cipherCalls.filter(x => x.method === 'tlscipherstart').length), 3);
      assert.deepEqual(errors, []);
      await context.close();
    }
    process.stdout.write('TLS cipher dialog: six viewport/theme cases passed\n');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
