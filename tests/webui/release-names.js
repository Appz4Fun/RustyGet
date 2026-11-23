// Downloads show clean release names with tags and the original name underneath;
// the logo easter egg. Offline (details-fixture), nothing reaches a daemon.
// Usage: NZBGET_CHROME_CHANNEL=none node tests/webui/release-names.js http://127.0.0.1:17999/
// Optional screenshots: NZBGET_UI_CAPTURE_DIR=/some/dir
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const { fixtureURL } = require('./workspace-fixture');
const { installDetailsFixture, RELEASE } = require('./details-fixture');

const PARSE_CASES = [
  ['The.Wheel.of.Time.S02E02.2160p.HDR10Plus.DV.WEBRip.6CH.x265.HEVC-PSA [NZBDAV copy 01]', 'The Wheel of Time', 'S02E02', '', '2160p WEBRip HDR10+ DV x265 PSA'],
  ['Blade.Runner.2049.2017.1080p.BluRay.x264-SPARKS', 'Blade Runner 2049', '', '2017', '1080p BluRay x264 SPARKS'],
  ['1917.2019.2160p.UHD.BluRay.REMUX.HDR.HEVC.Atmos-EPSiLON', '1917', '', '2019', '2160p Remux HDR HEVC EPSiLON'],
  ['The.Daily.Show.2024.03.14.720p.WEB.h264-EDITH', 'The Daily Show', '2024-03-14', '', '720p WEB H.264 EDITH'],
  ['Dune.Part.Two.2024.2160p.AMZN.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-FLUX', 'Dune Part Two', '', '2024', '2160p WEB-DL DV HDR H.265 FLUX'],
  ['show.name.1x05.hdtv.xvid-lol.nzb', 'Show Name', 'S01E05', '', 'HDTV XviD lol'],
  ['Movie.Title.2010.REPACK.1080p.BluRay.x264-GRP', 'Movie Title', '', '2010', '1080p BluRay x264 REPACK GRP'],
  // title words that are also metadata (Max, Uncut) stay in the title
  ['Mad.Max.Fury.Road.2015.1080p.BluRay.x264-GRP', 'Mad Max Fury Road', '', '2015', '1080p BluRay x264 GRP'],
  ['Uncut.Gems.2019.1080p.NF.WEB-DL.x264-GRP', 'Uncut Gems', '', '2019', '1080p WEB-DL x264 GRP'],
  ['Show.Name.REPACK.S01E02.720p.HDTV.x264-GRP', 'Show Name', 'S01E02', '', '720p HDTV x264 REPACK GRP'],
  // season and episode in separate tokens
  ['Show.Name.S01.E02.1080p.WEB-DL.x264-GRP', 'Show Name', 'S01E02', '', '1080p WEB-DL x264 GRP'],
  ['Show Name S03 E10 720p HDTV x264-GRP', 'Show Name', 'S03E10', '', '720p HDTV x264 GRP'],
  // a trailing episode range is not a release group
  ['Show.Name.S01E01-E02', 'Show Name', 'S01E01-E02', '', ''],
  ['Show.Name.S01E01-E02.720p.HDTV.x264-GRP', 'Show Name', 'S01E01-E02', '', '720p HDTV x264 GRP'],
  // a copy suffix after the group never hides it
  ['Dune.Part.Two.2024.2160p.BluRay.REMUX-FraMeSToR_3434', 'Dune Part Two', '', '2024', '2160p Remux FraMeSToR'],
  ['Dune.Part.Two.2024.2160p.BluRay.REMUX-FraMeSToR (12)', 'Dune Part Two', '', '2024', '2160p Remux FraMeSToR'],
  ['Dune.Part.Two.2024.2160p.BluRay.REMUX-FraMeSToR_1.nzb.2.queued', 'Dune Part Two', '', '2024', '2160p Remux FraMeSToR'],
];

(async () => {
  const base = fixtureURL(process.argv[2] || 'http://127.0.0.1:17999/');
  const capture = process.env.NZBGET_UI_CAPTURE_DIR;
  const browser = await chromium.launch(process.env.NZBGET_CHROME_CHANNEL === 'none' ? {} : { channel: process.env.NZBGET_CHROME_CHANNEL || 'chrome' })
    .catch(() => chromium.launch());
  const failures = [];
  let cases = 0;
  try {
    for (const theme of ['light', 'dark']) for (const [width, height] of [[1440, 900], [390, 844]]) {
      const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: capture ? 2 : 1 });
      await context.addInitScript(t => localStorage.setItem('Theme', t), theme);
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', e => errors.push(e.message));
      await installDetailsFixture(page, base, [], { patchReply: (method, result) => {
        if (method === 'listgroups') {
          // an obfuscated NZB name whose file name carries the release
          result[3].NZBName = 'a3f91c02d4e8b711deadbeefcafe0011';
          result[3].NZBFilename = '/nzb/Example.Documentary.2023.2160p.WEB-DL.DV.H.265-NOGRP.nzb';
        }
        return result;
      } });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      const tag = `${theme}-${width}`;
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${tag}/${label}: ${error.message}`); }
      };

      if (theme === 'light' && width === 1440) await check('parser', async () => {
        const got = await page.evaluate(cases => cases.map(([name]) => {
          const r = ReleaseName.parse(name);
          return [r.title, r.episode || r.date, r.year, r.tags.join(' ')];
        }), PARSE_CASES);
        PARSE_CASES.forEach(([name, ...want], i) => assert.deepEqual(got[i], want, name));
        assert(await page.evaluate(() => ReleaseName.isObfuscated('a3f91c02d4e8b711deadbeefcafe0011')));
        assert(!(await page.evaluate(() => ReleaseName.isObfuscated('Some.Movie.2001.1080p'))));
        assert.deepEqual(await page.evaluate(() => ReleaseName.parse('Mad.Max.Fury.Road.2015.MAX.WEB-DL-GRP').edition), ['MAX']);
        // a readable API name without release metadata is shown instead of the hash
        const fallback = await page.evaluate(() => {
          const div = document.createElement('div');
          div.innerHTML = ReleaseName.nameHtml({ NZBID: 1, NZBName: 'a3f91c02d4e8b711deadbeefcafe0011',
            Parameters: [{ Name: 'Title', Value: 'Example Documentary' }] }, '<a href="#">');
          return [div.querySelector('a').textContent, (div.querySelector('.release-original') || {}).textContent];
        });
        assert.deepEqual(fallback, ['Example Documentary', 'NZB a3f91c02d4e8b711deadbeefcafe0011']);
        // a candidate that parses wins over one that is only readable
        assert.equal(await page.evaluate(() => ReleaseName.bestName({ NZBName: 'a3f91c02d4e8b711deadbeefcafe0011',
          Parameters: [{ Name: 'Title', Value: 'Example Documentary' }],
          NZBFilename: 'Example.Documentary.2023.1080p.WEB-DL.x264-GRP.nzb' })), 'Example.Documentary.2023.1080p.WEB-DL.x264-GRP.nzb');
      });

      await check('download rows', async () => {
        const first = page.locator('#DownloadsTable tbody tr').first();
        const link = await first.locator('a[data-nzbid]').first().textContent();
        assert.equal(link, 'The Wheel of Time S02E02');
        const tags = await first.locator('.release-tag').allTextContents();
        for (const want of ['2160p', 'WEBRip', 'HDR10+', 'DV', 'x265', 'PSA']) assert(tags.includes(want), `tag ${want}: ${tags}`);
        assert.equal(await first.locator('.release-original').textContent(), `${RELEASE} [NZBDAV copy 01]`);
        const obf = page.locator('#DownloadsTable tbody tr').nth(3);
        assert.equal(await obf.locator('a[data-nzbid]').first().textContent(), 'Example Documentary (2023)');
        const under = await obf.locator('.release-original').textContent();
        assert(under.includes('Example.Documentary.2023.2160p.WEB-DL.DV.H.265-NOGRP') && under.includes('a3f91c02d4e8b711deadbeefcafe0011'), under);
        // search still finds the original and the clean name
        await page.evaluate(() => { $('#DownloadsTable_filter').val('Documentary').trigger('keyup'); });
        await page.waitForFunction(() => $('#DownloadsTable tbody tr').length === 1, null, { timeout: 5000 });
        await page.evaluate(() => { $('#DownloadsTable_filter').val('').trigger('keyup'); });
        await page.waitForFunction(() => $('#DownloadsTable tbody tr').length === 4, null, { timeout: 5000 });
        const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
        assert(overflow <= 0, `no horizontal scroll (${overflow})`);
        if (capture) await page.screenshot({ path: path.join(capture, `downloads-${tag}.jpg`), quality: 85 });
      });

      await check('logo easter egg', async () => {
        await page.clock.install();
        if (await page.locator('#Logo').isVisible()) await page.locator('#Logo').click();
        else await page.evaluate(() => EasterEgg.fire(document.querySelector('#Logo')));
        assert(await page.evaluate(() => EasterEgg.isRunning()));
        assert.equal(await page.locator('#EasterEggCanvas').evaluate(c => getComputedStyle(c).pointerEvents), 'none');
        await page.clock.runFor(300);
        const before = await page.evaluate(() => EasterEgg.particleCount());
        assert(before > 100, `confetti (${before})`);
        await page.clock.runFor(4000);
        const settled = await page.evaluate(() => EasterEgg.particleCount());
        for (let i = 0; i < 6; i++) { await page.mouse.move(200 + i * 60, 300 + i * 20); await page.clock.runFor(200); }
        const sparks = await page.evaluate(() => EasterEgg.particleCount());
        assert(sparks > settled + 40, `fireworks follow the mouse (${settled} -> ${sparks})`);
        if (capture) await page.screenshot({ path: path.join(capture, `easteregg-${tag}.jpg`), quality: 85 });
        await page.clock.runFor(31000);
        for (let i = 0; i < 3; i++) { await page.mouse.move(500, 500 + i * 10); await page.clock.runFor(200); }
        await page.clock.runFor(8000);
        assert(!(await page.evaluate(() => EasterEgg.isRunning())), 'show ends after 30 seconds');
        assert.equal(await page.locator('#EasterEggCanvas').count(), 0);
      });

      await check('no page errors', async () => assert.deepEqual(errors, []));
      await context.close();
    }
  } finally {
    await browser.close();
  }
  if (failures.length) {
    console.error(failures.join('\n'));
    console.error(`FAIL ${failures.length} of ${cases}`);
    process.exit(1);
  }
  console.log(`PASS ${cases} cases`);
})();
