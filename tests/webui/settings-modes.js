// Settings modes (Simple, Advanced, Expert) and the option controls: checkboxes,
// dropdowns, number boxes with +/- and preset sliders, and options greyed out until
// another option is changed.
// Offline: the committed WebUI with its real startup over synthetic RPC replies
// (details-fixture). saveconfig is intercepted, nothing reaches a daemon.
// Usage: node tests/webui/settings-modes.js http://127.0.0.1:17999/
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
      const page = await context.newPage();
      const errors = [], writes = [];
      page.on('pageerror', e => errors.push(e.message));
      await installDetailsFixture(page, base, writes, { patchReply: (method, result) => {
        if (method === 'config' || method === 'loadconfig') {
          const set = (name, value) => { const o = result.find(x => x.Name === name); if (o) o.Value = value; else result.push({ Name: name, Value: value }); };
          set('Unpack', 'no');
          // a category unpacks unless told not to: switch them all off
          [1, 2, 3, 4].forEach(n => set(`Category${n}.Unpack`, 'no'));
          set('ArticleRetries', '3');
          set('ParCheck', 'auto');
        }
        return result;
      } });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      await page.evaluate(() => $('a[href="#ConfigTab"]').first().click());
      await page.waitForFunction(() => document.querySelector('#ConfigMode'));
      let local = 0;
      const mode = async m => { await page.locator(`#ConfigMode button[data-mode="${m}"]`).click(); await page.waitForTimeout(150); };
      const check = async (label, action) => {
        cases++;
        try { if (++local > 4) await mode('expert'); await action(); } catch (error) { failures.push(`${theme}/${width}/${label}: ${error.message}`); }
      };
      const S = name => page.locator('#S_' + name);
      const row = name => S(name).locator('xpath=ancestor::div[contains(@class,"control-group")][1]');
      const show = async id => { await page.evaluate(i => Config.showSection(i, false), id); await page.waitForTimeout(150); };

      await check('simple is the default and hides the rest', async () => {
        assert.equal(await page.evaluate(() => Config.mode()), 'simple');
        assert.equal(await page.locator('#ConfigMode button[data-mode="simple"]').getAttribute('aria-checked'), 'true');
        assert.equal(await page.locator('.config-overview-tile[data-target="CHECK_AND_REPAIR"]').isVisible(), false,
          'Overview leaves out tiles for pages the mode hides');
        assert(await page.locator('.config-overview-tile[data-target="NEWS-SERVERS"]').isVisible());
        await show('CONNECTION');
        assert(await row('DownloadRate').isVisible(), 'Download rate is a Simple option');
        assert.equal(await row('ArticleRetries').isVisible(), false, 'Article retries waits for Advanced');
        assert.equal(await row('ArticleInterval').isVisible(), false, 'Article interval waits for Expert');
        assert.equal(await page.locator('#ConfigNav a[href="#LOGGING"]').evaluate(a => getComputedStyle(a.closest('li')).display), 'none',
          'A page with no Simple option leaves the menu');
      });

      await check('advanced and expert add options and tag them', async () => {
        await mode('advanced');
        assert(await row('ArticleRetries').isVisible());
        assert.equal(await row('ArticleRetries').locator('.config-mode-tag').textContent(), 'Advanced');
        assert.equal(await row('ArticleInterval').isVisible(), false);
        assert.notEqual(await page.locator('#ConfigNav a[href="#LOGGING"]').evaluate(a => getComputedStyle(a.closest('li')).display), 'none');
        await mode('expert');
        await show('Config-Info');
        assert(await page.locator('.config-overview-tile[data-target="CHECK_AND_REPAIR"]').isVisible());
        await show('CONNECTION');
        assert(await row('ArticleInterval').isVisible());
        assert.equal(await row('ArticleInterval').locator('.config-mode-tag').textContent(), 'Expert');
        assert.equal(await row('DownloadRate').locator('.config-mode-tag').count(), 0, 'Simple options carry no tag');
      });

      await check('the mode is remembered and moves by keyboard', async () => {
        await page.locator('#ConfigMode button[data-mode="expert"]').focus();
        await page.keyboard.press('ArrowLeft');
        assert.equal(await page.evaluate(() => localStorage.getItem('SettingsMode')), 'advanced');
        assert.equal(await page.evaluate(() => document.activeElement.getAttribute('data-mode')), 'advanced');
        await mode('expert');
      });

      await check('search still finds options outside the mode', async () => {
        await mode('simple');
        await page.locator('#ConfigTable_filter').fill('ArticleInterval');
        await page.waitForFunction(() => document.querySelector('#ConfigContent.search'));
        await page.waitForTimeout(400);
        assert(await row('ArticleInterval').isVisible(), 'A search result shows whatever the mode');
        // following the result to the option shows the mode it needs
        await page.locator('#label\\:S_ArticleInterval').click();
        await page.waitForTimeout(800);
        assert.equal(await page.evaluate(() => Config.mode()), 'expert', 'The mode rises to the option');
        assert(await row('ArticleInterval').isVisible(), 'The option stays in view');
        await page.locator('#ConfigTable_clearfilter').click();
        await page.waitForTimeout(300);
        await mode('expert');
      });

      await check('yes/no options are checkboxes', async () => {
        await show('DOWNLOAD_QUEUE');
        const box = S('NzbCleanupDisk');
        assert.equal(await box.getAttribute('role'), 'checkbox');
        const before = await box.getAttribute('aria-checked');
        await box.click();
        assert.notEqual(await box.getAttribute('aria-checked'), before);
        await box.click();
        assert.equal(await box.getAttribute('aria-checked'), before);
      });

      await check('short choices are a dropdown that drives the value', async () => {
        await show('CHECK_AND_REPAIR');
        const select = row('ParCheck').locator('select.config-select');
        assert(await select.isVisible(), 'ParCheck shows a menu');
        assert.equal(await row('ParCheck').locator('.btn-switch').isVisible(), false, 'Its buttons are hidden');
        assert.equal(await select.inputValue(), 'auto');
        await select.selectOption('force');
        assert.equal(await page.evaluate(() => $('#S_ParCheck .btn-primary').attr('data-value')), 'force');
        await select.selectOption('auto');
      });

      await check('numbers take digits only and step with +/-', async () => {
        await show('CONNECTION');
        const input = S('ArticleRetries');
        assert.equal(await input.getAttribute('inputmode'), 'numeric');
        await input.fill('');
        await input.pressSequentially('1a2b');
        assert.equal(await input.inputValue(), '12', 'Letters are dropped');
        await row('ArticleRetries').locator('.config-step[data-step="1"]').click();
        assert.equal(await input.inputValue(), '13');
        await row('ArticleRetries').locator('.config-step[data-step="-1"]').click();
        await row('ArticleRetries').locator('.config-step[data-step="-1"]').click();
        assert.equal(await input.inputValue(), '11');
        await input.focus();
        await page.keyboard.press('ArrowUp');
        assert.equal(await input.inputValue(), '12');
        await input.fill('500');
        await input.dispatchEvent('change');
        assert.equal(await input.inputValue(), '99', 'A number stops at the maximum the option allows');
        await input.fill('0');
        await row('ArticleRetries').locator('.config-step[data-step="-1"]').click();
        assert.equal(await input.inputValue(), '0', 'and at its minimum');
        // a reference to another option is a valid value and is kept
        await input.fill('');
        await input.pressSequentially('${UrlRetries}');
        await input.dispatchEvent('change');
        assert.equal(await input.inputValue(), '${UrlRetries}', 'Option references are kept');
        await input.fill('3');
        await input.dispatchEvent('input');
      });

      await check('the slider has sticky preset dots explained in the help', async () => {
        const r = row('ArticleRetries');
        const dots = r.locator('.config-slider__dot');
        assert((await dots.count()) >= 3 && (await dots.count()) <= 5, '3 to 5 presets');
        await dots.nth(1).click();
        const preset = await dots.nth(1).getAttribute('data-value');
        assert.equal(await S('ArticleRetries').inputValue(), preset, 'A dot sets its value');
        assert.equal(await dots.nth(1).getAttribute('class'), 'config-slider__dot is-on');
        assert((await r.locator('.config-number__preset').textContent()).length > 0, 'The preset name shows');
        // dragging near a dot lands on it
        const range = r.locator('.config-slider input[type="range"]');
        const near = await dots.nth(2).evaluate(d => Math.round(parseFloat(d.style.left) * 10) + 10);
        await range.evaluate((el, v) => { el.value = v; el.dispatchEvent(new Event('input', { bubbles: true })); }, near);
        assert.equal(await S('ArticleRetries').inputValue(), await dots.nth(2).getAttribute('data-value'), 'Sticky dot');
        // typing moves the slider; a value past its end pins it there
        await S('ArticleRetries').fill('40');
        await S('ArticleRetries').dispatchEvent('input');
        assert.equal(await range.inputValue(), '1000');
        // the keyboard moves a focused slider by one, and Page keys jump between presets
        await S('ArticleRetries').fill('3');
        await S('ArticleRetries').dispatchEvent('input');
        await range.focus();
        await page.keyboard.press('ArrowRight');
        assert.equal(await S('ArticleRetries').inputValue(), '4', 'ArrowRight steps by one');
        await page.keyboard.press('ArrowRight');
        assert.equal(await S('ArticleRetries').inputValue(), '5');
        await page.keyboard.press('ArrowLeft');
        assert.equal(await S('ArticleRetries').inputValue(), '4');
        await page.keyboard.press('PageUp');
        assert.equal(await S('ArticleRetries').inputValue(), '10', 'PageUp goes to the next preset');
        await page.keyboard.press('Home');
        assert.equal(await S('ArticleRetries').inputValue(), '0');
        const table = r.locator('.help-block table.config-presets');
        assert.equal(await table.locator('tr').count(), await dots.count(), 'The help lists each preset');
        assert((await table.locator('tr').first().locator('td').last().textContent()).length > 10, 'with what it means');
        await S('ArticleRetries').fill('3');
        await S('ArticleRetries').dispatchEvent('input');
      });

      await check('an option that needs another is greyed out until it is changed', async () => {
        await show('UNPACK');
        const r = row('DirectUnpack');
        assert(await r.evaluate(el => el.classList.contains('is-locked')), 'Locked while Unpack is off');
        assert.equal(await S('DirectUnpack').getAttribute('aria-disabled'), 'true');
        const before = await S('DirectUnpack').getAttribute('aria-checked');
        await S('DirectUnpack').click({ force: true });
        assert.equal(await S('DirectUnpack').getAttribute('aria-checked'), before, 'A locked checkbox does not change');
        assert.match(await r.locator('.config-lock-note').textContent(), /Turn on "Unpack"/);
        // a category that unpacks its own downloads unlocks the unpack options too
        await page.evaluate(() => { Config.switchClick($('#S_Category1_Unpack .btn[data-value="yes"]')[0]); Config.refreshChangeState(); });
        await page.waitForTimeout(250);
        assert.equal(await r.evaluate(el => el.classList.contains('is-locked')), false, 'Unlocked by a category with Unpack=yes');
        await page.evaluate(() => { Config.switchClick($('#S_Category1_Unpack .btn[data-value="no"]')[0]); Config.refreshChangeState(); });
        await page.waitForTimeout(250);
        assert(await r.evaluate(el => el.classList.contains('is-locked')));
        await S('Unpack').click();
        await page.waitForTimeout(300);
        assert.equal(await r.evaluate(el => el.classList.contains('is-locked')), false, 'Unlocked once Unpack is on');
        assert.equal(await r.locator('.config-lock-note').count(), 0);
        // article borrowing works without duplicate checking, so it stays editable
        await show('DUPE_FLEETS');
        await page.evaluate(() => { Config.switchClick($('#S_DupeCheck .btn[data-value="no"]')[0]); Config.refreshChangeState(); });
        await page.waitForTimeout(200);
        assert.equal(await row('DupeArticleFallback').evaluate(el => el.classList.contains('is-locked')), false);
        await page.evaluate(() => { Config.switchClick($('#S_DupeCheck .btn[data-value="yes"]')[0]); Config.refreshChangeState(); });
        // a value rule names the values it needs
        await page.evaluate(() => { const b = $('#S_DupeArticleFallback .btn[data-value="article"]')[0]; Config.switchClick(b); Config.refreshChangeState(); });
        await page.waitForTimeout(200);
        const stream = row('DupeStreamTimeout');
        assert(await stream.evaluate(el => el.classList.contains('is-locked')));
        assert.match(await stream.locator('.config-lock-note').textContent(), /stream or live/i);
      });

      await check('signed numbers and expression fields keep what is typed', async () => {
        await show('RSS_FEEDS');
        await page.locator('.config-add.RSS_FEEDS').click();
        await page.waitForTimeout(800);
        const prio = page.locator('[id^="S_Feed"][id$="_Priority"]').last();
        await prio.fill('');
        await prio.pressSequentially('-100');
        assert.equal(await prio.inputValue(), '-100', 'A feed priority may be negative');
        await prio.locator('xpath=ancestor::div[contains(@class,"config-number")][1]').locator('.config-step[data-step="-1"]').click();
        assert.equal(await prio.inputValue(), '-101');
        await show('SCHEDULER');
        await page.locator('.config-add.SCHEDULER').click();
        await page.waitForTimeout(800);
        const days = page.locator('[id^="S_Task"][id$="_WeekDays"]').last();
        assert.equal(await days.evaluate(el => !!el.closest('.config-number')), false, 'Weekdays are not a stepper');
        await days.fill('1-5, 7');
        await days.dispatchEvent('input');
        await days.dispatchEvent('change');
        assert.equal(await days.inputValue(), '1-5, 7');
      });

      await check('added list entries get the new controls', async () => {
        await show('NEWS-SERVERS');
        await page.locator('.config-add.NEWS-SERVERS').click();
        await page.waitForTimeout(800);
        const ids = await page.evaluate(() => $('[id^="S_Server"][id$="_Connections"]').map((i, e) => e.id).get());
        const last = ids[ids.length - 1];
        assert(await page.locator('#' + last).evaluate(el => !!el.closest('.config-number')), 'Connections has a stepper');
      });

      await check('values set by the controls are saved', async () => {
        await show('CONNECTION');
        await S('ArticleRetries').fill('7');
        await S('ArticleRetries').dispatchEvent('input');
        await S('ArticleRetries').dispatchEvent('change');
        await page.locator('#ConfigSaveBar').waitFor({ state: 'visible' });
        await page.evaluate(() => Config.saveChanges());
        await page.waitForFunction(n => n.some(w => w.method === 'saveconfig'), writes, { polling: 100, timeout: 5000 }).catch(() => {});
        await page.waitForTimeout(500);
        const save = writes.find(w => w.method === 'saveconfig');
        assert(save, 'saveconfig was sent');
        const values = Object.fromEntries(save.params[0].map(o => [o.Name, o.Value]));
        assert.equal(values.ArticleRetries, '7');
        assert.equal(values.Unpack, 'yes');
        assert.equal(values.DupeArticleFallback, 'article');
        assert.equal(values.ParCheck, 'auto');
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
  console.log(`settings-modes: ${cases} cases passed`);
})();
