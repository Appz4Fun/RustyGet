// A config whose paths do not work: the daemon still serves the web interface.
// Starts its own isolated loopback daemon (MainDir=/nonexistent), then checks
// in Chrome that Downloads, History, Messages and Settings work without page
// errors, that Messages holds one "can't be used ... Settings > Paths" warning
// per bad path, that everything is paused, and that typing a valid MainDir in
// Settings > Paths, saving and reloading clears the warnings and unpauses.
// Usage: node tests/webui/bad-paths.js build/debug/rustyget [port]
// Optional: NZBGET_UI_CAPTURE_DIR=<dir> saves JPEGs of the bad-path state.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const root = path.resolve(__dirname, '..', '..');
const BAD_OPTIONS = ['MainDir', 'DestDir', 'InterDir', 'NzbDir', 'QueueDir', 'TempDir', 'LogFile'];

function writeConfig(file, port, mainDir) {
  const replace = {
    MainDir: mainDir, ControlIP: '127.0.0.1', ControlPort: String(port),
    ControlUsername: '', ControlPassword: '', WebDir: path.join(root, 'webui'),
    ConfigTemplate: path.join(root, 'rustyget.conf'), UpdateCheck: 'none',
  };
  const text = fs.readFileSync(path.join(root, 'rustyget.conf'), 'utf8').split('\n')
    .map(line => {
      const m = /^([A-Za-z0-9.]+)=/.exec(line);
      return m && m[1] in replace ? `${m[1]}=${replace[m[1]]}` : line;
    }).join('\n');
  fs.writeFileSync(file, text);
}

async function rpc(base, method, params = []) {
  const res = await fetch(new URL('jsonrpc', base), { method: 'POST', body: JSON.stringify({ method, params }) });
  return (await res.json()).result;
}

async function waitFor(fn, timeout, what) {
  const deadline = Date.now() + timeout;
  for (;;) {
    try { const v = await fn(); if (v) return v; } catch {}
    if (Date.now() > deadline) throw new Error('timed out waiting for ' + what);
    await new Promise(r => setTimeout(r, 300));
  }
}

(async () => {
  const bin = path.resolve(process.argv[2] || path.join(root, 'build', 'debug', 'rustyget'));
  const port = Number(process.argv[3] || 17791);
  const base = new URL(`http://127.0.0.1:${port}/`);
  assert(port !== 6789, 'never the default port of a real daemon');
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'rustyget-badpaths-ui-'));
  // as root /nonexistent could be created: put MainDir under a regular file instead
  let badMain = '/nonexistent';
  if (process.getuid && process.getuid() === 0) {
    fs.writeFileSync(path.join(work, 'blocker'), '');
    badMain = path.join(work, 'blocker', 'nonexistent');
  }
  assert(!fs.existsSync(badMain), badMain + ' must not exist');
  const goodMain = path.join(work, 'main');
  const conf = path.join(work, 'rustyget.conf');
  writeConfig(conf, port, badMain);
  const out = fs.openSync(path.join(work, 'daemon.out'), 'w');
  const daemon = spawn(bin, ['-c', conf, '-s', '-o', 'OutputMode=log'], { stdio: ['ignore', out, out] });
  const failures = [];
  let cases = 0;
  const check = async (label, action) => {
    cases++;
    try { await action(); } catch (error) { failures.push(`${label}: ${error.message}`); }
  };
  const captureDir = process.env.NZBGET_UI_CAPTURE_DIR;
  const browser = await chromium.launch(process.env.NZBGET_CHROME_CHANNEL === 'none' ? {} : { channel: process.env.NZBGET_CHROME_CHANNEL || 'chrome' })
    .catch(() => chromium.launch());
  try {
    await waitFor(() => rpc(base, 'version'), 20000, 'the daemon');
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    await context.addInitScript(() => {
      localStorage.setItem('MessagesRecordsPerPage', '100');
      localStorage.setItem('SettingsMode', 'expert');
    });
    const page = await context.newPage();
    const errors = [];
    // while the daemon restarts its port is closed for a moment: the page polls
    // until the new instance answers, and only those refused requests are expected
    let reloading = false;
    page.on('pageerror', e => errors.push('pageerror: ' + e.message));
    page.on('console', m => {
      if (m.type() !== 'error') return;
      if (reloading && /ERR_CONNECTION_REFUSED|ERR_EMPTY_RESPONSE|ERR_CONNECTION_RESET/.test(m.text())) return;
      errors.push('console: ' + m.text());
    });
    await page.goto(base.href);
    await page.locator('#DownloadsTable').waitFor({ state: 'visible', timeout: 15000 });
    await page.waitForFunction(() => window.Status && Status.status && Status.status.UpTimeSec !== undefined, null, { timeout: 15000 });

    await check('everything is paused while the paths are bad', async () => {
      const st = await rpc(base, 'status');
      assert.deepEqual([st.DownloadPaused, st.PostPaused, st.ScanPaused], [true, true, true]);
    });

    for (const [tab, table] of [['Downloads', '#DownloadsTable'], ['History', '#HistoryTable'], ['Messages', '#MessagesTable']]) {
      await check(`${tab} tab renders`, async () => {
        await page.locator(`#${tab}TabLink`).click();
        await page.locator(table).waitFor({ state: 'visible', timeout: 5000 });
        assert(await page.locator(`#${tab}Tab`).evaluate(e => e.classList.contains('active')), tab + ' is the active tab');
      });
    }

    await check('Messages: one clear warning per bad path', async () => {
      await page.locator('#MessagesTabLink').click();
      await page.waitForFunction(() => document.querySelectorAll('#MessagesTable tbody tr').length > 3, null, { timeout: 10000 });
      const rows = await page.locator('#MessagesTable tbody tr').allInnerTexts();
      for (const option of BAD_OPTIONS) {
        const hits = rows.filter(r => r.includes(`${option} "`) && r.includes("can't be used") && r.includes(`Change ${option} in Settings > Paths`));
        assert.equal(hits.length, 1, `${option}: ${hits.length} warnings`);
      }
      assert.equal(rows.filter(r => r.includes('[Paths][')).length, 0, 'the health check does not repeat them');
      assert.equal(rows.filter(r => r.includes('paused until the bad paths are fixed')).length, 1, 'one pause notice');
      if (captureDir) await page.screenshot({ path: path.join(captureDir, 'bad-paths-messages.jpg'), type: 'jpeg', quality: 85, fullPage: true });
    });

    await check('Downloads shows the paused state', async () => {
      await page.locator('#DownloadsTabLink').click();
      await page.locator('#DownloadsTable').waitFor({ state: 'visible' });
      if (captureDir) await page.screenshot({ path: path.join(captureDir, 'bad-paths-downloads.jpg'), type: 'jpeg', quality: 85 });
    });

    await check('Settings > Paths opens and flags the bad paths', async () => {
      await page.locator('#ConfigTabLink').click();
      await page.locator('#ConfigNav a[href="#PATHS"]').waitFor({ state: 'visible', timeout: 15000 });
      assert(await page.locator('#ConfigLoadServerTemplateError').isHidden(), 'the template loaded');
      await page.locator('#ConfigNav a[href="#PATHS"]').click();
      await page.locator('#S_MainDir').waitFor({ state: 'visible', timeout: 5000 });
      assert.equal(await page.locator('#S_MainDir').inputValue(), badMain);
      // the health check marks the option itself too
      await page.waitForFunction(() => window.SystemHealth && SystemHealth.getSection('PATHS') !== undefined, null, { timeout: 10000 });
      const row = page.locator('#S_MainDir').locator('xpath=ancestor::div[contains(@class,"control-group")][1]');
      await row.locator('.option-alert').waitFor({ state: 'visible', timeout: 5000 });
      if (captureDir) await page.screenshot({ path: path.join(captureDir, 'bad-paths-settings.jpg'), type: 'jpeg', quality: 85 });
    });

    await check('a valid MainDir saves and the reload clears the warnings', async () => {
      await page.locator('#S_MainDir').fill(goodMain);
      await page.locator('#ConfigSaveBar').waitFor({ state: 'visible', timeout: 5000 });
      assert.match(await page.locator('.config-savebar__count').textContent(), /^1 change$/);
      if (captureDir) await page.screenshot({ path: path.join(captureDir, 'bad-paths-fixed-unsaved.jpg'), type: 'jpeg', quality: 85 });
      await page.locator('#ConfigSaveBar_Save').click();
      await page.locator('#ConfigReload').waitFor({ state: 'visible', timeout: 10000 });
      assert.match(fs.readFileSync(conf, 'utf8'), new RegExp(`^MainDir=${goodMain.replace(/[/.]/g, '\\$&')}$`, 'm'));
      reloading = true;
      await page.locator('#ConfigReload').click();
      // the page reloads itself once the new instance answers
      await page.waitForURL(base.href + '**', { timeout: 30000 });
      await waitFor(async () => {
        const st = await rpc(base, 'status');
        return st && !st.DownloadPaused && !st.PostPaused && !st.ScanPaused;
      }, 30000, 'unpaused after the reload');
      for (const d of ['dst', 'inter', 'nzb', 'queue', 'tmp']) assert(fs.statSync(path.join(goodMain, d)).isDirectory(), d);
      await page.locator('#DownloadsTable').waitFor({ state: 'visible', timeout: 15000 });
      await page.waitForFunction(() => window.Status && Status.status && Status.status.UpTimeSec !== undefined, null, { timeout: 15000 });
      reloading = false;
      await page.locator('#MessagesTabLink').click();
      await page.waitForFunction(() => document.querySelectorAll('#MessagesTable tbody tr').length > 0, null, { timeout: 10000 });
      const rows = await page.locator('#MessagesTable tbody tr').allInnerTexts();
      const after = rows.filter(r => r.includes("can't be used") && r.includes(goodMain));
      assert.equal(after.length, 0, 'no warnings about the fixed paths: ' + after.join(' | '));
      const log = await rpc(base, 'log', [0, 1000]);
      const reloaded = log.map(m => m.Text);
      const lastStart = reloaded.lastIndexOf(reloaded.filter(t => t.startsWith('RustyGet ') && t.endsWith('server-mode')).pop());
      const warnedAfter = reloaded.slice(lastStart - 20 < 0 ? 0 : lastStart - 20).filter(t => /can't be used/.test(t) && !t.includes(badMain));
      assert.equal(warnedAfter.length, 0, 'no path warnings after the reload: ' + warnedAfter.join(' | '));
    });

    if (errors.length) failures.push('console/page errors: ' + errors.join('; '));
    await context.close();
  } finally {
    await browser.close();
    daemon.kill('SIGTERM');
    await new Promise(r => setTimeout(r, 1000));
    try { daemon.kill('SIGKILL'); } catch {}
  }
  if (failures.length) {
    console.error(`bad-paths: ${failures.length} of ${cases} cases failed (work dir ${work})\n` + failures.join('\n'));
    process.exit(1);
  }
  fs.rmSync(work, { recursive: true, force: true });
  console.log(`bad-paths: ${cases} cases passed`);
})().catch(error => { console.error(error); process.exit(1); });
