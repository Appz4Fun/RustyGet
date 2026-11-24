// Settings overview, titled cards, Advanced folds and the Dupe fleets page.
// Offline: the committed WebUI with its real startup over synthetic RPC replies
// (details-fixture). saveconfig is intercepted, nothing reaches a daemon.
// Usage: node tests/webui/settings-cards.js http://127.0.0.1:17999/
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture } = require('./details-fixture');

const DUPE_OPTIONS = ['DupeCheck', 'DupeWatch', 'DupeArticleFallback', 'DupeStreamDecompress', 'DupeStreamTimeout', 'DupeBorrowFirst',
  'DupeHealthPreset', 'DupeHealthPercent', 'DupeHealthMin', 'DupeHealthMax', 'DupeBodyChecks', 'DupeHealthBudget', 'DupeMinAlive',
  'DupeBodyPercent', 'DupeCheckConcurrency', 'DupeCheckConnections', 'HealthCheck'];

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
      // these checks cover every option, which Expert shows
      await context.addInitScript(() => localStorage.setItem('SettingsMode', 'expert'));
      const page = await context.newPage();
      const errors = [], writes = [];
      page.on('pageerror', e => errors.push(e.message));
      // an option the layout doesn't list (a newer or customized template) still shows, first in its section
      await installDetailsFixture(page, base, writes, { patchReply: (method, result) => {
        if (method === 'configtemplates') {
          result[0].Template = result[0].Template.replace(/^(#?FlushQueue=.*)$/m, '$1\n\n# Fixture option the layout does not list.\n#\n# Value (yes, no).\nFixtureUnlisted=no');
        } else if (method === 'config' || method === 'loadconfig') {
          result.push({ Name: 'FixtureUnlisted', Value: 'yes' });
          // dark runs use the older "yes" spelling, which the daemon reads as article
          if (theme === 'dark') result.find(o => o.Name === 'DupeArticleFallback').Value = 'yes';
          // dark runs load a config written before ParPreset existed: it runs as custom
          if (theme === 'dark') result.splice(result.findIndex(o => o.Name === 'ParPreset'), 1);
        }
        return result;
      } });
      // health: an error on a folded dupe option and on a NZB folder option, both stored in Incoming NZBs
      await page.route(/\/jsonrpc(?:\/|$)/, route => {
        const rpc = route.request().method() === 'POST' ? route.request().postDataJSON() : null;
        if (!rpc || rpc.method !== 'systemhealth') return route.fallback();
        return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: { Alerts: [], Sections: [
          { Name: 'IncomingNzbs', Issues: [], Subsections: [], Options: [
            { Name: 'DupeCheckConnections', Severity: 'Error', Message: 'Fixture: too many check connections' },
            { Name: 'NzbDirInterval', Severity: 'Error', Message: 'Fixture: NZB folder notice' }] }] } } });
      });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      await page.evaluate(() => $('a[href="#ConfigTab"]').first().click());
      await page.waitForFunction(() => document.querySelector('#ConfigNav a[href="#DUPE_FLEETS"]'));
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${theme}/${width}/${label}: ${error.message}`); }
      };
      const S = name => page.locator('#S_' + name);
      const row = name => S(name).locator('xpath=ancestor::div[contains(@class,"control-group")][1]');

      await check('overview first', async () => {
        assert.equal((await page.locator('#ConfigTitle').textContent()).trim(), 'Overview');
        const nav = await page.locator('#ConfigNav > li').allTextContents();
        assert.equal(nav[0].trim(), 'Start here');
        assert.equal(nav[1].trim(), 'Overview');
        const tiles = page.locator('#ConfigOverview .config-overview-tile');
        assert.equal(await tiles.count(), 6);
        const text = await page.locator('#ConfigOverview').innerText();
        assert.match(text, /2 \/ 2 active[\s\S]*40 connections/i);
        // the tile's own text (its accessible name) keeps the parts apart
        const servers = await page.locator('#ConfigOverview .config-overview-tile[data-target="NEWS-SERVERS"]').textContent();
        assert.match(servers, /2 \/ 2 active\s+40 connections/i);
        assert.match(text, /\/mnt\/20tb\/rustyget\/dst/);
        assert.match(text, theme === 'dark' ? /Article swap/ : /Live repair/);
        assert.match(text, /While downloading/);
        assert.match(text, /Unlimited/);
        await page.locator('#ConfigOverview .config-overview-tile[data-target="DUPE_FLEETS"]').click();
        await page.waitForFunction(() => Config.currentSectionID === 'DUPE_FLEETS');
        assert.equal((await page.locator('#ConfigTitle').textContent()).trim(), 'Dupe fleets');
      });

      await check('dupe fleets page holds every dupe option', async () => {
        const cards = await page.locator('#ConfigData .config-card.DUPE_FLEETS:visible .config-card__title').allTextContents();
        assert.deepEqual(cards.map(c => c.toLowerCase()), ['fleet repair', 'health sampling']);
        for (const name of DUPE_OPTIONS) {
          assert.equal(await S(name).count(), 1, name + ' exists');
          assert.equal(await row(name).locator('xpath=ancestor::section[contains(@class,"DUPE_FLEETS")]').count(), 1, name + ' sits on Dupe fleets');
        }
        assert.equal((await row('DupeBorrowFirst').locator('.config-label').textContent()).trim(), 'Borrow first');
        assert(await row('DupeCheck').isVisible());
        assert(await row('DupeHealthPreset').isVisible(), 'The sampling preset is on the card');
        assert.equal(await row('DupeHealthPercent').locator('xpath=ancestor::details[contains(@class,"config-card__more")]').count(), 1,
          'The raw sampling values are folded under Custom sampling');
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, 'No horizontal page scroll');
      });

      await check('incoming NZBs keeps only its own options', async () => {
        await page.evaluate(() => Config.showSection('INCOMING_NZBS', false));
        const keys = await page.locator('#ConfigData .control-group:visible .config-key').allTextContents();
        assert.deepEqual(keys.map(k => k.trim()).sort(), ['AppendCategoryDir', 'NzbDirArchiveAction', 'NzbDirArchiveScan', 'NzbDirFileAge', 'NzbDirInterval']);
        await page.evaluate(() => Config.showSection('CHECK_AND_REPAIR', false));
        assert.equal(await row('HealthCheck').isVisible(), false);
        assert(await row('ParCheck').isVisible());
      });

      await check('every option is still on exactly one page', async () => {
        // sections with a card layout leave no row outside a card
        const orphans = await page.evaluate(() => ['PATHS', 'SECURITY', 'INCOMING_NZBS', 'DOWNLOAD_QUEUE', 'CONNECTION', 'LOGGING', 'CHECK_AND_REPAIR', 'UNPACK']
          .flatMap(id => $('#ConfigData > .control-group.' + id).map(function() { return $('.config-key', this).text(); }).get()));
        assert.deepEqual(orphans, [], 'Ungrouped option rows: ' + orphans.join(', '));
      });

      await check('health follows the moved option', async () => {
        const badge = id => page.locator(`#ConfigNav a[href="#${id}"] .config-health-badges .badge`);
        await page.waitForFunction(() => document.querySelector('#ConfigNav a[href="#DUPE_FLEETS"] .badge'), null, { timeout: 5000 });
        assert.equal((await badge('DUPE_FLEETS').textContent()).trim(), '1');
        assert.equal((await badge('INCOMING_NZBS').textContent()).trim(), '1', 'Incoming NZBs counts only its own option');
        assert.equal(await page.locator('.config-card.DUPE_FLEETS[data-card="health"] details.config-card__more').evaluate(d => d.open), true,
          'A fold with a health error opens');
        assert.match(await row('DupeCheckConnections').innerText(), /too many check connections/);
      });

      await check('an unlisted option lands in an untitled first card', async () => {
        await page.evaluate(() => Config.showSection('DOWNLOAD_QUEUE', false));
        const cards = page.locator('#ConfigData .config-card.DOWNLOAD_QUEUE');
        assert.equal(await cards.first().getAttribute('data-card'), 'other');
        assert.equal(await cards.first().locator('.config-card__head').count(), 0);
        assert(await row('FixtureUnlisted').isVisible());
        assert.deepEqual(await cards.evaluateAll(list => list.map(c => c.getAttribute('data-card'))), ['other', 'queue', 'disk', 'names']);
      });

      await check('advanced fold', async () => {
        await page.evaluate(() => Config.showSection('DOWNLOAD_QUEUE', false));
        const fold = page.locator('.config-card.DOWNLOAD_QUEUE[data-card="disk"] details.config-card__more');
        assert.equal(await fold.evaluate(d => d.open), false);
        assert.match(await fold.locator('summary').textContent(), /Advanced \(2\)/);
        assert.equal(await row('RawArticle').isVisible(), false);
        await fold.locator('summary').click();
        assert(await row('RawArticle').isVisible());
        await fold.locator('summary').click();
      });

      await check('search reaches folded options', async () => {
        await page.locator('#ConfigTable_filter').fill('DupeBodyPercent');
        await page.waitForFunction(() => document.querySelector('#ConfigContent.search'));
        await page.waitForTimeout(300);
        assert(await row('DupeBodyPercent').isVisible(), 'A folded match is shown');
        assert.equal(await row('DupeMinAlive').isVisible(), false, 'Non-matching rows in the same card stay hidden');
        await page.locator('#ConfigTable_clearfilter').click();
        await page.waitForTimeout(300);
      });

      await check('par resource level buttons', async () => {
        await page.evaluate(() => Config.showSection('CHECK_AND_REPAIR', false));
        const group = row('ParPreset').locator('.config-preset-buttons');
        assert.equal(await group.locator('.btn').count(), 6, 'low, medium, large, uber, detect, custom');
        assert.equal(await row('ParPreset').locator('select.config-select').count(), 0, 'stays buttons, not a menu');
        const summary = row('ParPreset').locator('.config-preset-summary');
        if (theme === 'dark') {
          assert.equal(await group.locator('.btn-primary').getAttribute('data-value'), 'custom', 'a config without ParPreset shows Custom');
          assert.match(await summary.innerText(), /Custom resources/);
        } else {
          await page.waitForFunction(() => /1024 MB/.test($('.config-preset-summary').text()));
        }
        await group.locator('.btn[data-value="uber"]').click();
        assert.match(await summary.innerText(), /4096 MB, 14 threads/);
        assert(!(await row('ParPreset').locator('.config-preset-detect').isVisible()), 'Measure again only for Detect');
        await group.locator('.btn[data-value="detect"]').click();
        assert.match(await summary.innerText(), /12 CPUs.*1000 MB\/s write.*4096 MB, all CPUs/);
        assert(await row('ParPreset').locator('.config-preset-detect').isVisible());
        await group.locator('.btn[data-value="custom"]').click();
        assert.match(await summary.innerText(), /Custom resources/);
        const box = await group.boundingBox();
        assert(box.width <= width, 'buttons fit the width');
        await group.locator('.btn[data-value="large"]').click();
      });

      await check('a moved option still saves', async () => {
        await page.evaluate(() => Config.showSection('DUPE_FLEETS', false));
        await S('DupeCheckConcurrency').fill('9');
        await S('DupeCheckConcurrency').dispatchEvent('input');
        await S('DupeCheckConcurrency').dispatchEvent('change');
        await page.locator('#ConfigSaveBar').waitFor({ state: 'visible' });
        await page.evaluate(() => Config.saveChanges());
        await page.waitForFunction(n => n.some(w => w.method === 'saveconfig'), writes, { polling: 100, timeout: 5000 }).catch(() => {});
        await page.waitForTimeout(500);
        const save = writes.find(w => w.method === 'saveconfig');
        assert(save, 'saveconfig was sent; settings the page refuses: ' + await page.evaluate(() =>
          Array.from(document.querySelectorAll('.config-invalid')).map(e => e.id + ' ' + e.textContent).join(' | ')));
        const values = Object.fromEntries(save.params[0].map(o => [o.Name, o.Value]));
        assert.equal(values.DupeCheckConcurrency, '9');
        assert.equal(values.DupeCheck, 'yes');
        assert.equal(values.NzbDirInterval, '5');
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
  console.log(`settings-cards: ${cases} cases passed`);
})();
