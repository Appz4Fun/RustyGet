// Settings rows, switches and the save bar on the offline Settings model.
// Usage: node tests/webui/settings-savebar.js http://127.0.0.1:17689/
// Covers desktop and phone widths in both themes: readable labels, switch keyboard
// access, the unsaved summary (option edits, an option missing from the config file,
// a confirmed list delete, a dialog-style programmatic edit), and Discard.
const assert = require('node:assert/strict');
const { chromium } = require('playwright');
const { fixtureURL } = require('./workspace-fixture');
const { installOfflineModel } = require('./settings-offline-fixture');

(async () => {
  const base = fixtureURL(process.argv[2]);
  const browser = await chromium.launch({ channel: 'chrome' });
  let cases = 0;
  try {
    for (const theme of ['light', 'dark']) for (const width of [1280, 390]) {
      const context = await browser.newContext({ viewport: { width, height: 900 } });
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      const boot = await installOfflineModel(page, base);
      // the TLS cipher optimizer asks for its status when Settings opens
      await page.route('**/jsonrpc', route => {
        const rpc = route.request().postDataJSON();
        if (rpc.method !== 'tlscipherstatus') return route.fallback();
        return route.fulfill({ contentType: 'application/json',
          json: { id: rpc.id, result: { Available: false, State: 'idle', JobID: '', Saved: false } } });
      });
      // ArticleCache is left out of the saved values: the page shows its default
      boot.removeSavedValue('ArticleCache');
      await page.goto(base.href);
      await boot({ theme, width });
      await page.evaluate(() => ConfirmDialog.init()); // the offline boot skips application startup
      await page.waitForFunction(() => document.querySelector('#ConfigNav a[href="#DOWNLOAD_QUEUE"]'));
      const bar = page.locator('#ConfigSaveBar');
      const count = page.locator('.config-savebar__count');

      // grouped sidebar, sentence case
      assert.deepEqual(await page.locator('#ConfigNav > li.config-nav-group').allTextContents(),
        ['Start here', 'Downloading', 'Automation', 'System']);
      assert.equal((await page.locator('#ConfigNav a[href="#NEWS-SERVERS"]').textContent()).trim(), 'News servers');

      await page.evaluate(() => Config.showSection('DOWNLOAD_QUEUE', false));
      assert.equal(await page.locator('#ConfigTitle').getAttribute('data-path'), 'rustyget::settings::download_queue');
      assert.equal((await page.locator('#ConfigTitle').textContent()).trim(), 'Download queue');
      const flush = page.locator('#label\\:S_FlushQueue');
      assert.equal((await flush.locator('.config-label').textContent()).trim(), 'Flush queue to disk');
      assert.equal((await flush.locator('.config-key').textContent()).trim(), 'FlushQueue');
      assert(await bar.isHidden(), 'No save bar before any change');

      // a switch toggles from the keyboard and is counted
      const toggle = page.locator('#S_ContinuePartial');
      assert.equal(await toggle.getAttribute('role'), 'switch');
      const before = await toggle.getAttribute('aria-checked');
      await toggle.focus();
      await page.keyboard.press('Space');
      assert.notEqual(await toggle.getAttribute('aria-checked'), before);
      await bar.waitFor({ state: 'visible' });
      assert.equal(await count.textContent(), '1 change');
      assert(await page.locator('.control-group.is-changed #S_ContinuePartial').count() === 1);

      // an option missing from the config file still counts once changed
      await page.locator('#S_ArticleCache').fill('777');
      await page.waitForFunction(() => document.querySelector('.config-savebar__count').textContent === '2 changes');
      if (width >= 768) {
        assert.match(await page.locator('.config-savebar__names').textContent(), /Continue partial downloads, Article cache/);
      }

      // Discard reloads the saved values and keeps the section
      await page.locator('#ConfigSaveBar_Discard').click();
      await page.waitForFunction(() => Config.currentSectionID === 'DOWNLOAD_QUEUE' && document.querySelector('#S_ContinuePartial'));
      await page.waitForFunction(() => document.getElementById('ConfigSaveBar').hidden);
      assert.equal(await page.locator('#S_ContinuePartial').getAttribute('aria-checked'), before);

      // a list delete confirmed in a dialog shows the save bar
      await page.evaluate(() => Config.showSection('CATEGORIES', false));
      const sets = await page.locator('#ConfigData [onclick*="deleteSet"]').count();
      assert(sets > 0, 'The template has categories to delete');
      // the delete button sits in the closed list editor; click it the way its handler expects
      await page.evaluate(() => document.querySelector('#ConfigData [onclick*="deleteSet"]').click());
      await page.locator('#ConfirmDialog_OK').waitFor({ state: 'visible' });
      await page.waitForTimeout(300);
      await page.locator('#ConfirmDialog_OK').click();
      await page.waitForFunction(() => !document.getElementById('ConfigSaveBar').hidden);
      assert.equal(await count.textContent(), 'List changes');
      await page.locator('#ConfigSaveBar_Discard').click();
      await page.waitForFunction(n => Config.currentSectionID === 'CATEGORIES' &&
        document.querySelectorAll('#ConfigData [onclick*="deleteSet"]').length === n &&
        document.getElementById('ConfigSaveBar').hidden, sets);

      // an editor dialog writes the control directly; the next click anywhere refreshes the bar
      await page.evaluate(() => Config.showSection('PATHS', false));
      await page.evaluate(() => { document.getElementById('S_DestDir').value = './fixture/dst/from-dialog'; });
      await page.mouse.click(5, 5);
      await page.waitForFunction(() => document.querySelector('.config-savebar__count').textContent === '1 change');

      assert.deepEqual(errors, [], 'No page errors');
      await context.close();
      cases++;
    }
  } finally {
    await browser.close();
  }
  process.stdout.write('Settings save bar: ' + cases + ' cases passed\n');
})().catch(error => { console.error(error); process.exit(1); });
