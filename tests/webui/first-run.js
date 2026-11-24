// RustyGet with no configuration file, and with a terrible one: the web
// interface works, the setup guide gets it ready to download, Settings refuses
// values it can't save (in the page and in the daemon), keeps backups that can
// be restored, and asks before destructive actions.
//
// Starts its own loopback daemons (a copy of the binary in a temporary folder,
// HOME pointed at an empty folder, a local test news server from --nserv) and
// never uses a real configuration. Sanitized example values only.
//
// Usage: node tests/webui/first-run.js build/debug/rustyget
// Optional: NZBGET_UI_CAPTURE_DIR=<dir> saves JPEGs of every step.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const net = require('node:net');
const os = require('node:os');
const path = require('node:path');

const captureDir = process.env.NZBGET_UI_CAPTURE_DIR;
const PHONE = { width: 390, height: 844 };
const DESKTOP = { width: 1440, height: 900 };

function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.listen(0, '127.0.0.1', () => { const p = srv.address().port; srv.close(() => resolve(p)); });
    srv.on('error', reject);
  });
}

async function waitFor(fn, timeout, what) {
  const deadline = Date.now() + timeout;
  for (;;) {
    try { const v = await fn(); if (v) return v; } catch {}
    if (Date.now() > deadline) throw new Error('timed out waiting for ' + what);
    await new Promise(r => setTimeout(r, 250));
  }
}

async function rpcRaw(base, method, params = [], auth) {
  const headers = auth ? { Authorization: 'Basic ' + Buffer.from(auth).toString('base64') } : {};
  const res = await fetch(new URL('jsonrpc', base), { method: 'POST', headers, body: JSON.stringify({ method, params }) });
  return res.json();
}
async function rpc(base, method, params = [], auth) { return (await rpcRaw(base, method, params, auth)).result; }

async function tab(page, name) {
  await page.evaluate(n => { $('#' + n + 'TabLink').tab('show'); }, name);
  // "in" ends Bootstrap's fade: a tab shown before the last fade ended waits
  // for a transition end that never comes
  await page.waitForFunction(n => { const e = document.getElementById(n + 'Tab'); return e && e.classList.contains('active') && (!e.classList.contains('fade') || e.classList.contains('in')); }, name, { timeout: 10000 });
}

async function invalidNames(page) {
  return page.evaluate(() => Array.from(document.querySelectorAll('.config-invalid')).map(e => e.id + ': ' + e.textContent).join(' | '));
}

async function ready(page, timeout = 20000) {
  await page.waitForFunction(() => window.Status && Status.status && Status.status.UpTimeSec !== undefined &&
    document.getElementById('MainTabContent') && document.getElementById('MainTabContent').style.display !== 'none', null, { timeout });
}

async function shot(page, name) {
  if (!captureDir) return;
  fs.mkdirSync(captureDir, { recursive: true });
  await page.waitForTimeout(150);
  await page.screenshot({ path: path.join(captureDir, name + '.jpg'), type: 'jpeg', quality: 85 });
}

class Run {
  constructor(name) {
    this.name = name;
    this.work = fs.mkdtempSync(path.join(os.tmpdir(), 'rustyget-ui-' + name + '-'));
    this.procs = [];
    this.failures = [];
    this.cases = 0;
  }
  spawn(bin, args, env, outName) {
    const out = fs.openSync(path.join(this.work, outName), 'w');
    const p = spawn(bin, args, { cwd: this.work, env: { ...process.env, ...env }, stdio: ['ignore', out, out] });
    this.procs.push(p);
    return p;
  }
  read(outName) { return fs.readFileSync(path.join(this.work, outName), 'utf8'); }
  async check(label, action) {
    this.cases++;
    try { await action(); } catch (error) {
      this.failures.push(`${this.name}: ${label}: ${error.message}`);
      if (this.page && captureDir) {
        await this.page.screenshot({ path: path.join(captureDir, `FAIL-${this.name}-${this.cases}.jpg`), type: 'jpeg', quality: 70 }).catch(() => {});
      }
    }
  }
  async stop() {
    for (const p of this.procs) { try { p.kill('SIGTERM'); } catch {} }
    await new Promise(r => setTimeout(r, 800));
    for (const p of this.procs) { try { p.kill('SIGKILL'); } catch {} }
  }
}

function watchPage(page, errors, state) {
  page.on('pageerror', e => errors.push('pageerror: ' + e.message));
  page.on('console', m => {
    if (m.type() !== 'error') return;
    // while the daemon restarts its port is closed for a moment; a new password answers 401 until sign-in
    if (state.reloading && /ERR_CONNECTION_REFUSED|ERR_EMPTY_RESPONSE|ERR_CONNECTION_RESET|401|Failed to load resource/.test(m.text())) return;
    errors.push('console: ' + m.text());
  });
}

async function newPage(browser, viewport, theme, errors, state) {
  const context = await browser.newContext({ viewport, colorScheme: theme });
  await context.addInitScript(t => {
    try { localStorage.setItem('Theme', t); localStorage.setItem('SettingsMode', 'expert'); } catch {}
  }, theme);
  const page = await context.newPage();
  watchPage(page, errors, state);
  return { context, page };
}

// ----------------------------------------------------------------------------
// 1. No configuration file: the guide, desktop and light
// ----------------------------------------------------------------------------
async function noConfig(browser, binSrc, viewport, theme, complete) {
  const run = new Run('noconfig-' + theme + (viewport.width < 500 ? '-phone' : ''));
  const bin = path.join(run.work, 'rustyget');
  fs.copyFileSync(binSrc, bin);
  fs.chmodSync(bin, 0o755);
  const home = path.join(run.work, 'home');
  fs.mkdirSync(home);
  const nntpPort = await freePort();
  const errors = [];
  const state = { reloading: false };
  const prefix = (viewport.width < 500 ? 'phone-' : 'desktop-') + theme + '-';
  try {
    fs.mkdirSync(path.join(run.work, 'nserv'));
    run.spawn(bin, ['--nserv', '-d', path.join(run.work, 'nserv'), '-p', String(nntpPort), '-i', '1', '-v', '0'], {}, 'nserv.out');
    run.spawn(bin, ['-s', '-o', 'OutputMode=log'], { HOME: home }, 'daemon.out');
    const url = await waitFor(() => (/Open the web interface to finish setting up RustyGet: (http:\/\/\S+)/.exec(run.read('daemon.out')) || [])[1], 20000, 'the address on stdout');
    const base = new URL(url);
    const conf = path.join(home, '.config', 'rustyget', 'rustyget.conf');
    const made = await waitFor(() => (/Sign in with user "rustyget" and password "([A-Za-z0-9]{20})"/.exec(run.read('daemon.out')) || [])[1], 20000, 'the password on stdout');
    const firstAuth = 'rustyget:' + made;
    await run.check('stdout says where the file is and the address', async () => {
      const out = run.read('daemon.out');
      assert(out.includes('RustyGet created a new configuration file: ' + conf), out);
      assert(fs.statSync(conf).isFile());
      assert.equal(fs.statSync(conf).mode & 0o777, 0o600);
      const text = fs.readFileSync(conf, 'utf8');
      assert.match(text, /^MainDir=~\/RustyGet$/m);
      assert.match(text, /^ControlIP=127\.0\.0\.1$/m);
      assert.match(text, new RegExp('^ControlPassword=' + made + '$', 'm'));
      assert.match(text, /^FormAuth=yes$/m);
      assert.match(text, new RegExp('^ControlPort=' + base.port + '$', 'm'));
      assert(base.port !== '6789' || !out.includes('is in use'));
      await waitFor(() => rpc(base, 'version', [], firstAuth), 20000, 'the daemon');
      const anon = await fetch(new URL('jsonrpc', base), { method: 'POST', body: JSON.stringify({ method: 'status', params: [] }) });
      assert.equal(anon.status, 401, 'a password from the first start');
      const log = (await rpc(base, 'log', [0, 200], firstAuth)).map(m => m.Text);
      assert(log.some(t => t.includes('created a new configuration file')), 'logged too');
      assert(!log.some(t => t.includes(made)), 'the password is not in the log');
      assert(fs.statSync(path.join(home, 'RustyGet', 'queue')).isDirectory(), 'MainDir made in HOME');
    });
    await run.check('downloads stay paused until setup is done', async () => {
      const st = await rpc(base, 'status', [], firstAuth);
      assert.equal(st.DownloadPaused, true);
      const s = await rpc(base, 'setupstatus', [], firstAuth);
      assert.equal(s.SetupComplete, false);
      assert.equal(s.DownloadsBlocked, true);
      assert.equal(s.ConfigCreated, true);
      assert.equal(s.Servers, 0);
      assert.equal(s.PasswordSet, true);
    });

    const { context, page } = await newPage(browser, viewport, theme, errors, state);
    run.page = page;
    state.reloading = true;
    await page.goto(base.href);
    const wizard = page.locator('#SetupWizard');
    await run.check('sign in with the password from the terminal', async () => {
      // (the sign-in form's first probe is answered 401: expected, not an error)
      await page.locator('#LoginDialog_Username').waitFor({ state: 'visible', timeout: 20000 });
      await shot(page, prefix + '00-first-sign-in');
      await page.locator('#LoginDialog_Username').fill('rustyget');
      await page.locator('#LoginDialog_Password').fill(made);
      await page.locator('#LoginDialog_Password').press('Enter');
    });
    await run.check('the setup guide opens by itself and shows the password', async () => {
      await wizard.waitFor({ state: 'visible', timeout: 20000 });
      state.reloading = false;
      assert.equal(await page.locator('#SetupWizard .setup-steps li.is-current').getAttribute('data-step'), '1');
      assert(!(await page.locator('#SetupNext').isDisabled()), 'the password RustyGet made can be kept');
      assert.equal(await page.evaluate(() => document.activeElement && document.activeElement.id), 'SetupUser');
      assert(!(await page.locator('#SetupCurrentPass').textContent()).includes(made), 'hidden until asked');
      await page.locator('#SetupRevealPass').click();
      assert.equal(await page.locator('#SetupCurrentPass').textContent(), made);
      await shot(page, prefix + '01-wizard-password');
      await page.locator('#SetupRevealPass').click();
    });

    if (!complete) {
      await run.check('step 1: keep the password RustyGet made', async () => {
        await page.locator('#SetupNext').click();
        await page.locator('#SetupWizard [data-page="2"]').waitFor({ state: 'visible' });
        await page.locator('#SetupBack').click();
        await page.locator('#SetupWizard [data-page="1"]').waitFor({ state: 'visible' });
      });
    }

    await run.check('step 1: a new password is checked as you type', async () => {
      await page.locator('#SetupPass').fill('short');
      await page.locator('#SetupPass_Msg').filter({ hasText: 'at least 8' }).waitFor({ timeout: 3000 });
      await page.locator('#SetupPass').fill('example-pass-1');
      await page.locator('#SetupPass2').fill('example-pass-2');
      await page.locator('#SetupPass2_Msg').filter({ hasText: 'not the same' }).waitFor({ timeout: 3000 });
      assert(await page.locator('#SetupNext').isDisabled());
      assert.match(await page.locator('#SetupWhy').textContent(), /not the same/);
      await shot(page, prefix + '02-wizard-password-mismatch');
      await page.locator('#SetupPass2').fill('example-pass-1');
      await page.waitForFunction(() => !document.getElementById('SetupNext').disabled);
      // keyboard: Enter goes on
      await page.locator('#SetupPass2').press('Enter');
      await page.locator('#SetupWizard [data-page="2"]').waitFor({ state: 'visible' });
    });

    await run.check('step 2: folders are checked live', async () => {
      await page.locator('#SetupMainDir_Msg.is-ok').waitFor({ timeout: 5000 });
      assert.match(await page.locator('#SetupMainDir_Msg').textContent(), /exists and RustyGet can write to it/);
      fs.writeFileSync(path.join(run.work, 'a-file'), 'x');
      await page.locator('#SetupDestDir').fill(path.join(run.work, 'a-file', 'downloads'));
      await page.locator('#SetupDestDir_Msg.is-bad').waitFor({ timeout: 5000 });
      assert.match(await page.locator('#SetupDestDir_Msg').textContent(), /is a file, so no folder can be created inside it/);
      assert(await page.locator('#SetupNext').isDisabled());
      await shot(page, prefix + '03-wizard-folder-bad');
      await page.locator('#SetupDestDir').fill('${MainDir}/finished');
      await page.locator('#SetupDestDir_Msg.is-ok').waitFor({ timeout: 5000 });
      assert.match(await page.locator('#SetupDestDir_Msg').textContent(), /doesn't exist yet. RustyGet will create it/);
      await shot(page, prefix + '04-wizard-folders-ok');
      await page.locator('#SetupNext').click();
      await page.locator('#SetupWizard [data-page="3"]').waitFor({ state: 'visible' });
    });

    await run.check('step 3: bad input and failed tests say what to do', async () => {
      assert(await page.locator('#SetupNext').isDisabled(), 'no server yet');
      await page.locator('#SetupHost').fill('http://news.example.com');
      await page.locator('#SetupHost_Msg').filter({ hasText: 'without http://' }).waitFor({ timeout: 3000 });
      await page.locator('#SetupHost').fill('news.example.invalid');
      await page.locator('#SetupPort').fill('70000');
      await page.locator('#SetupPort_Msg').filter({ hasText: '1 to 65535' }).waitFor({ timeout: 3000 });
      await page.locator('#SetupPort').fill('563');
      await page.locator('#SetupTest').click();
      await page.locator('#SetupTestResult.is-bad').waitFor({ timeout: 30000 });
      assert.match(await page.locator('#SetupTestResult').textContent(), /could not be found \(DNS\)/);
      await shot(page, prefix + '05-wizard-server-dns');
      const closed = await freePort();
      await page.locator('#SetupHost').fill('127.0.0.1');
      await page.locator('#SetupTls').uncheck();
      assert.equal(await page.locator('#SetupPort').inputValue(), '119', 'the port follows encryption');
      await page.locator('#SetupPort').fill(String(closed));
      await page.locator('#SetupTest').click();
      await page.locator('#SetupTestResult.is-bad').filter({ hasText: 'did not answer on this port' }).waitFor({ timeout: 30000 });
      assert(await page.locator('#SetupNext').isDisabled(), 'Next waits for a good test');
      await shot(page, prefix + '06-wizard-server-port');
    });

    await run.check('step 3: a good test lets you go on', async () => {
      await page.locator('#SetupPort').fill(String(nntpPort));
      await page.locator('#SetupSrvUser').fill('example-user');
      await page.locator('#SetupSrvPass').fill('example-secret');
      await page.locator('#SetupConns').fill('4');
      await page.locator('#SetupTest').click();
      await page.locator('#SetupTestResult.is-ok').waitFor({ timeout: 30000 });
      assert.match(await page.locator('#SetupTestResult').textContent(), /Connected to 127\.0\.0\.1 and signed in/);
      await page.waitForFunction(() => !document.getElementById('SetupNext').disabled);
      await shot(page, prefix + '07-wizard-server-ok');
      // changing a value after the test asks for a new test
      await page.locator('#SetupConns').fill('5');
      assert(!(await page.locator('#SetupNext').isDisabled()), 'connections do not need a new test');
      await page.locator('#SetupSrvPass').fill('example-secret2');
      await page.waitForFunction(() => document.getElementById('SetupNext').disabled);
      await page.locator('#SetupSrvPass').fill('example-secret');
      await page.locator('#SetupTest').click();
      await page.locator('#SetupTestResult.is-ok').waitFor({ timeout: 30000 });
      await page.locator('#SetupNext').click();
      await page.locator('#SetupWizard [data-page="4"]').waitFor({ state: 'visible' });
      const summary = await page.locator('#SetupSummary').textContent();
      assert(summary.includes('127.0.0.1:' + nntpPort) && summary.includes(path.join(home, 'RustyGet', 'finished')), summary);
      await shot(page, prefix + '08-wizard-summary');
    });

    if (!complete) {
      await run.check('the expert skip leaves the banner on', async () => {
        await page.locator('#SetupSkip').click();
        await wizard.waitFor({ state: 'detached' });
        await page.locator('#SetupBanner .setup-banner__box--todo').waitFor({ state: 'visible' });
        const text = await page.locator('#SetupBanner').textContent();
        assert(text.includes('Downloads are paused until RustyGet is set up') && text.includes('Add a news server'), text);
        await shot(page, prefix + '09-banner-after-skip');
        // resume explains instead of resuming
        await page.locator('#PlayPauseButton').click();
        await page.locator('#AlertDialog').waitFor({ state: 'visible' });
        assert.match(await page.locator('#AlertDialog').textContent(), /can't download yet/);
        await shot(page, prefix + '10-resume-blocked');
        await page.keyboard.press('Escape');
        assert.equal((await rpc(base, 'status', [], firstAuth)).DownloadPaused, true);
        // reloading the page does not reopen a skipped guide
        await page.reload();
        await page.locator('#SetupBanner .setup-banner__box--todo').waitFor({ state: 'visible', timeout: 15000 });
        assert(await wizard.count() === 0, 'skipped stays skipped');
      });
    } else {
      await run.check('save: the settings are written, backed up and RustyGet restarts', async () => {
        state.reloading = true;
        await page.locator('#SetupNext').click();
        await page.locator('.setup-restart').waitFor({ timeout: 10000 });
        await shot(page, prefix + '09-wizard-restarting');
        const text = fs.readFileSync(conf, 'utf8');
        assert.match(text, /^ControlPassword=example-pass-1$/m);
        assert.match(text, /^DestDir=\$\{MainDir\}\/finished$/m);
        assert.match(text, /^Server1\.Active=yes$/m);
        assert.match(text, new RegExp('^Server1\\.Port=' + nntpPort + '$', 'm'));
        const backups = fs.readdirSync(path.dirname(conf)).filter(n => n.startsWith('rustyget.conf.bak-'));
        assert.equal(backups.length, 1, backups.join());
        // this browser keeps its session; a new one gets the sign-in form
        await page.waitForTimeout(1500);
        await ready(page, 40000);
        state.reloading = false;
        const fresh = await newPage(browser, viewport, theme, errors, { reloading: true });
        await fresh.page.goto(base.href);
        await fresh.page.locator('#LoginDialog_Username').waitFor({ state: 'visible', timeout: 20000 });
        await shot(fresh.page, prefix + '10-sign-in');
        await fresh.page.locator('#LoginDialog_Username').fill('rustyget');
        await fresh.page.locator('#LoginDialog_Password').fill('wrong-password');
        await fresh.page.locator('#LoginDialog_Password').press('Enter');
        await fresh.page.locator('#LoginDialog_Error').waitFor({ state: 'visible', timeout: 10000 });
        await fresh.page.locator('#LoginDialog_Password').fill('example-pass-1');
        await fresh.page.locator('#LoginDialog_Password').press('Enter');
        await ready(fresh.page);
        await fresh.context.close();
      });
      await run.check('after setup: no banner, downloading is not paused', async () => {
        const auth = 'rustyget:example-pass-1';
        const s = await rpc(base, 'setupstatus', [], auth);
        assert.equal(s.SetupComplete, true, JSON.stringify(s));
        assert.equal((await rpc(base, 'status', [], auth)).DownloadPaused, false);
        await page.waitForTimeout(1500);
        assert(await page.locator('#SetupBanner').isHidden(), 'no banner');
        assert(await page.locator('#SetupWizard').count() === 0, 'no guide');
        const anon = await fetch(new URL('jsonrpc', base), { method: 'POST', body: JSON.stringify({ method: 'status', params: [] }) });
        assert.equal(anon.status, 401, 'the password is needed now');
        await shot(page, prefix + '11-ready');
      });
    }
    await context.close();
  } finally {
    await run.stop();
  }
  if (errors.length) run.failures.push(run.name + ': console/page errors: ' + errors.join('; '));
  return run;
}

// ----------------------------------------------------------------------------
// 2. A terrible configuration file
// ----------------------------------------------------------------------------
function terribleConfig(port, main, noServer) {
  const parts = [
    Buffer.from('# a terrible configuration file\n'),
    Buffer.from('ControlIP=127.0.0.1\nControlPort=' + port + '\nControlUsername=\nControlPassword=\n'),
    Buffer.from('MainDir=' + main + '\n'),
    Buffer.from(noServer ? '#\n#\n#\n#\n' : 'Server1.Host=127.0.0.1\nServer1.Port=1\nServer1.Encryption=no\nServer1.Connections=2\n'),
    Buffer.from('ArticleTimeout=soon\n'),               // a wrong type
    Buffer.from('ParCheck=sometimes\n'),                // an invalid choice
    Buffer.from('NotAnOption=1\n'),                     // an unknown option
    Buffer.from('this line is broken\n'),               // a broken line
    Buffer.from('DiskSpace=100\nDiskSpace=200\n'),      // a duplicate key
    Buffer.from([0x00, 0xff, 0xfe, 0x01, 0x02, 0x0a]),  // binary junk
    Buffer.from('Category1.Name=Caf\xe9\n', 'latin1'),  // not UTF-8
    Buffer.alloc(70000, 'x'), Buffer.from('\n'),        // a huge line
    Buffer.from('UpdateCheck=none\n'),
  ];
  return Buffer.concat(parts);
}

async function garbage(browser, bin, viewport, theme) {
  const run = new Run('garbage-' + theme + (viewport.width < 500 ? '-phone' : ''));
  const port = await freePort();
  const base = new URL(`http://127.0.0.1:${port}/`);
  const conf = path.join(run.work, 'rustyget.conf');
  const main = path.join(run.work, 'main');
  fs.writeFileSync(conf, terribleConfig(port, main));
  const errors = [];
  const state = { reloading: false };
  const prefix = 'garbage-' + (viewport.width < 500 ? 'phone-' : 'desktop-') + theme + '-';
  try {
    const daemon = run.spawn(bin, ['-c', conf, '-s', '-o', 'OutputMode=log'], { HOME: run.work }, 'daemon.out');
    await waitFor(() => rpc(base, 'version'), 20000, 'the daemon');
    await run.check('it starts and says once per bad line what was used instead', async () => {
      assert.equal(daemon.exitCode, null, 'still running');
      const out = run.read('daemon.out');
      const expect = [
        [11, 'Invalid value for option "ArticleTimeout": "soon" is not a whole number. Using the default "60" instead'],
        [12, 'Invalid value for option "ParCheck": "sometimes". Using "auto" instead'],
        [13, 'Invalid option "NotAnOption": RustyGet has no such option, the line is ignored'],
        [14, 'Invalid option "this line is broken": not a "Name=value" line, ignored'],
        [16, 'Option "DiskSpace" is set again (first on line 15); using the value from this line'],
        [19, 'Line is too long (70000 bytes'],
      ];
      const log = (await rpc(base, 'log', [0, 500])).map(m => m.Text);
      for (const [line, text] of expect) {
        const tag = conf + ':' + line + ': ';
        assert.equal(log.filter(t => t.startsWith(tag)).length, 1, `line ${line} once: ` + log.filter(t => t.includes(conf)).join(' | '));
        assert(log.some(t => t.startsWith(tag) && t.includes(text)), `line ${line}: ${text}`);
        assert(out.includes(tag), 'on stdout too: ' + tag);
      }
      const s = await rpc(base, 'setupstatus');
      assert(s.Warnings.length >= expect.length, JSON.stringify(s.Warnings));
    });
    const { context, page } = await newPage(browser, viewport, theme, errors, state);
    run.page = page;
    await page.goto(base.href);
    await ready(page);
    await run.check('the web interface shows the problems and what was used', async () => {
      // a news server is set: nothing blocks downloading, the guide stays shut,
      // and a missing password is only a gentle hint
      await page.locator('#SetupBanner .setup-banner__box--hint').waitFor({ state: 'visible', timeout: 15000 });
      assert.equal(await page.locator('#SetupWizard').count(), 0);
      assert.equal(await page.locator('#SetupBanner .setup-banner__box--todo').count(), 0, 'no red banner for a working setup');
      assert.match(await page.locator('#SetupBanner .setup-banner__box--hint').textContent(), /no password/);
      const notes = page.locator('#SetupBanner .setup-banner__box--notes');
      await notes.waitFor({ state: 'visible' });
      assert.match(await notes.textContent(), /The configuration file had \d+ problems/);
      await notes.locator('summary').click();
      assert.match(await notes.textContent(), /ArticleTimeout/);
      await shot(page, prefix + '01-banner-problems');
    });
    await run.check('Resume works for a working setup without a password', async () => {
      await rpc(base, 'pausedownload');
      await page.evaluate(() => Refresher.update());
      await page.waitForFunction(() => window.Status && Status.status && Status.status.DownloadPaused === true, null, { timeout: 10000 });
      await page.evaluate(() => Status.playClick());
      await page.waitForTimeout(800);
      assert(!(await page.locator('#AlertDialog').isVisible()), 'no "set up first" dialog');
      await waitFor(async () => (await rpc(base, 'status')).DownloadPaused === false, 10000, 'resumed');
      await shot(page, prefix + '01b-password-hint');
    });
    for (const name of ['Downloads', 'History', 'Messages']) {
      await run.check(name + ' renders', async () => {
        await tab(page, name);
      });
    }
    await run.check('Settings opens with the defaults that were used', async () => {
      await tab(page, 'Config');
      await page.locator('#ConfigNav a[href="#CONNECTION"]').waitFor({ state: 'visible', timeout: 15000 });
      await page.locator('#ConfigNav a[href="#CONNECTION"]').click();
      await page.locator('#S_ArticleTimeout').waitFor({ state: 'visible' });
      // the file still says "soon": the page flags it, and Save waits for a fix
      await page.locator('#S_ArticleTimeout_Invalid').waitFor({ state: 'visible', timeout: 5000 });
      assert.match(await page.locator('#S_ArticleTimeout_Invalid').textContent(), /whole number/);
      assert(await page.locator('#ConfigSaveBar_Save').isDisabled(), 'Save is off while invalid');
      await shot(page, prefix + '02-settings-invalid');
      await page.locator('#S_ArticleTimeout').fill('60');
      await page.waitForFunction(() => !document.getElementById('S_ArticleTimeout_Invalid'));
    });

    await run.check('invalid input is refused in the page', async () => {
      await page.locator('#ConfigNav a[href="#PATHS"]').click();
      await page.locator('#S_DestDir').waitFor({ state: 'visible' });
      await page.locator('#S_DestDir').fill('');
      await page.locator('#S_DestDir_Invalid').filter({ hasText: 'must not be empty' }).waitFor({ timeout: 5000 });
      fs.writeFileSync(path.join(run.work, 'plainfile'), 'x');
      await page.locator('#S_InterDir').fill(path.join(run.work, 'plainfile'));
      await page.locator('#S_InterDir_Invalid').filter({ hasText: 'is a file, not a folder' }).waitFor({ timeout: 5000 });
      assert(await page.locator('#ConfigSaveBar_Save').isDisabled());
      assert.match(await page.locator('.config-savebar__fix').textContent(), /Fix \d+ settings before saving/);
      await shot(page, prefix + '03-settings-paths-invalid');
      // the fix link goes to the first problem
      await page.locator('.config-savebar__fix').click();
      await page.waitForFunction(() => document.activeElement && /S_(DestDir|InterDir|ArticleTimeout)/.test(document.activeElement.id));
      await page.locator('#S_DestDir').fill('${MainDir}/dst');
      await page.locator('#S_InterDir').fill('${MainDir}/inter');
      // ParCheck still says "sometimes": the fix link leads there, and a choice fixes it
      await page.waitForFunction(() => document.querySelectorAll('.config-invalid').length === 1, null, { timeout: 5000 })
        .catch(async e => { throw new Error(e.message + ' invalid: ' + await invalidNames(page)); });
      assert.match(await page.locator('#S_ParCheck_Invalid').textContent(), /Pick one of: auto, always, force, manual/);
      await page.locator('.config-savebar__fix').click();
      const parRow = page.locator('#S_ParCheck').locator('xpath=ancestor::div[contains(@class,"control-group")][1]');
      await parRow.locator('select, .btn').first().waitFor({ state: 'visible' });
      await shot(page, prefix + '03b-settings-choice-invalid');
      if (await parRow.locator('select').count()) await parRow.locator('select').first().selectOption('auto');
      else await page.locator('#S_ParCheck .btn').first().click();
      await page.waitForFunction(() => !document.querySelector('.config-invalid'), null, { timeout: 5000 })
        .catch(async e => { throw new Error(e.message + ' still invalid: ' + await invalidNames(page)); });
      assert(!(await page.locator('#ConfigSaveBar_Save').isDisabled()));
    });

    await run.check('invalid input is refused by the daemon and the file is untouched', async () => {
      const before = fs.readFileSync(conf);
      const bad = await rpcRaw(base, 'saveconfig', [[{ Name: 'ControlPort', Value: 'abc' }, { Name: 'Server1.Port', Value: '70000' },
        { Name: 'DestDir', Value: '' }, { Name: 'Server1.Host', Value: 'http://bad host' }, { Name: 'NzbDirInterval', Value: '5\nControlPassword=x' }]]);
      assert(bad.error, JSON.stringify(bad));
      const msg = bad.error.message;
      for (const part of ['ControlPort: "abc" is not a whole number', 'News server 1: Port: 70000 is out of range',
        'DestDir: enter a folder', 'is not a host name or IP address', 'line break']) assert(msg.includes(part), msg);
      assert(fs.readFileSync(conf).equals(before), 'the file is unchanged');
      assert.equal(fs.readdirSync(run.work).filter(n => n.startsWith('rustyget.conf.bak-')).length, 0, 'no backup for a refused save');
    });

    await run.check('a save backs up first and writes a clean file', async () => {
      await page.locator('#ConfigNav a[href="#CONNECTION"]').click();
      await page.locator('#S_ArticleTimeout').fill('45');
      await page.locator('#ConfigSaveBar_Save').click();
      // the lines that are not settings are listed, and removed only on OK
      await page.locator('#ConfirmDialog_OK').waitFor({ state: 'visible', timeout: 10000 });
      assert.match(await page.locator('#ConfirmDialog_Title').textContent(), /Remove lines the file can't keep\?/);
      const listed = await page.locator('#ConfirmDialog_Text').textContent();
      assert(/NotAnOption/.test(listed) && /this line is broken/.test(listed), listed);
      await page.waitForTimeout(400);
      await page.locator('#ConfirmDialog_OK').scrollIntoViewIfNeeded();
      await shot(page, prefix + '03c-save-drop-confirm');
      await page.locator('#ConfirmDialog_OK').click();
      await page.locator('#ConfigReload').waitFor({ state: 'visible', timeout: 10000 });
      const backups = fs.readdirSync(run.work).filter(n => n.startsWith('rustyget.conf.bak-'));
      assert.equal(backups.length, 1);
      assert(fs.readFileSync(path.join(run.work, backups[0])).equals(terribleConfig(port, main)), 'the backup is the old file');
      const text = fs.readFileSync(conf, 'latin1');
      assert.match(text, /^ArticleTimeout=45$/m);
      assert(!/this line is broken|NotAnOption|xxxxxxxxxx/.test(text), 'junk is gone');
      assert.equal(fs.statSync(conf).mode & 0o777, fs.statSync(conf).mode & 0o777);
    });

    await run.check('restore previous settings asks, restores and restarts', async () => {
      state.reloading = true;
      await page.locator('#ConfigReload').click();
      await page.waitForTimeout(1500);
      await ready(page, 40000);
      state.reloading = false;
      await tab(page, 'Config');
      await page.locator('#ConfigNav a[href="#Config-System"]').waitFor({ state: 'visible', timeout: 15000 });
      await page.locator('#ConfigNav a[href="#Config-System"]').click();
      await page.locator('#Config_RestorePrevious').click();
      await page.locator('#ConfirmDialog').waitFor({ state: 'visible' });
      assert.match(await page.locator('#ConfirmDialog_Title').textContent(), /Restore previous settings\?/);
      assert.equal(await page.locator('#RestorePrevious_Choice option').count(), 1);
      await shot(page, prefix + '04-restore-confirm');
      // cancel first: nothing changes
      await page.locator('#ConfirmDialog [data-dismiss="modal"]').first().click();
      await page.locator('#ConfirmDialog').waitFor({ state: 'hidden' });
      assert.match(fs.readFileSync(conf, 'latin1'), /^ArticleTimeout=45$/m);
      state.reloading = true;
      await page.locator('#Config_RestorePrevious').click();
      await page.locator('#ConfirmDialog_OK').click();
      await page.waitForTimeout(1500);
      await ready(page, 40000);
      await waitFor(() => fs.readFileSync(conf).equals(terribleConfig(port, main)), 10000, 'the old file back');
      assert.equal(fs.readdirSync(run.work).filter(n => n.startsWith('rustyget.conf.bak-')).length, 2, 'the current file was backed up too');
      state.reloading = false;
    });

    await run.check('destructive actions ask first', async () => {
      await ready(page, 40000);
      await tab(page, 'Messages');
      await page.evaluate(() => Messages.clearClick ? Messages.clearClick() : null);
      const hasClear = await page.locator('#ConfirmDialog').isVisible();
      if (!hasClear) {
        await page.locator('#MessagesTab button:has-text("Clear"), #MessagesTab a:has-text("Clear")').first().click();
      }
      await page.locator('#ConfirmDialog').waitFor({ state: 'visible', timeout: 5000 });
      await shot(page, prefix + '05-clear-messages-confirm');
      await page.locator('#ConfirmDialog [data-dismiss="modal"]').first().click();
      await page.locator('#ConfirmDialog').waitFor({ state: 'hidden' });
      assert((await rpc(base, 'log', [0, 10])).length > 0, 'messages kept after cancel');
      // discarding unsaved Settings changes
      await tab(page, 'Config');
      await page.locator('#ConfigNav a[href="#CONNECTION"]').waitFor({ state: 'visible', timeout: 15000 });
      await page.locator('#ConfigNav a[href="#CONNECTION"]').click();
      await page.locator('#S_ArticleTimeout').fill('61');
      await page.locator('#ConfigSaveBar_Discard').click();
      await page.locator('#ConfirmDialog').waitFor({ state: 'visible' });
      assert.match(await page.locator('#ConfirmDialog_Title').textContent(), /Discard your changes\?/);
      await shot(page, prefix + '06-discard-confirm');
      await page.locator('#ConfirmDialog_OK').click();
      await page.locator('#ConfirmDialog').waitFor({ state: 'hidden' });
    });
    await context.close();
  } finally {
    await run.stop();
  }
  if (errors.length) run.failures.push(run.name + ': console/page errors: ' + errors.join('; '));
  return run;
}

async function garbageGuide(browser, bin) {
  const run = new Run('garbage-guide');
  const port = await freePort();
  const nntpPort = await freePort();
  const base = new URL(`http://127.0.0.1:${port}/`);
  const conf = path.join(run.work, 'rustyget.conf');
  const main = path.join(run.work, 'main');
  fs.writeFileSync(conf, terribleConfig(port, main, true));
  const errors = [];
  const state = { reloading: false };
  try {
    fs.mkdirSync(path.join(run.work, 'nserv'));
    run.spawn(bin, ['--nserv', '-d', path.join(run.work, 'nserv'), '-p', String(nntpPort), '-i', '1', '-v', '0'], {}, 'nserv.out');
    run.spawn(bin, ['-c', conf, '-s', '-o', 'OutputMode=log'], { HOME: run.work }, 'daemon.out');
    await waitFor(() => rpc(base, 'version'), 20000, 'the daemon');
    const { context, page } = await newPage(browser, DESKTOP, 'light', errors, state);
    run.page = page;
    await page.goto(base.href);
    await run.check('the guide opens for a terrible file with no news server', async () => {
      await page.locator('#SetupWizard').waitFor({ state: 'visible', timeout: 20000 });
      await page.locator('#SetupPass').fill('example-pass-9');
      await page.locator('#SetupPass2').fill('example-pass-9');
      await page.locator('#SetupNext').click();
      await page.locator('#SetupMainDir_Msg.is-ok').waitFor({ timeout: 5000 });
      await page.locator('#SetupDestDir_Msg.is-ok').waitFor({ timeout: 5000 });
      await page.locator('#SetupNext').click();
      await page.locator('#SetupHost').fill('127.0.0.1');
      await page.locator('#SetupTls').uncheck();
      await page.locator('#SetupPort').fill(String(nntpPort));
      await page.locator('#SetupConns').fill('2');
      await page.locator('#SetupTest').click();
      await page.locator('#SetupTestResult.is-ok').waitFor({ timeout: 30000 });
      await page.locator('#SetupNext').click();
      await page.locator('#SetupWizard [data-page="4"]').waitFor({ state: 'visible' });
    });
    await run.check('save lists the lines that can\'t be kept, drops them after a confirm, and succeeds', async () => {
      await page.locator('#SetupNext').click();
      await page.locator('#SetupDropOk').waitFor({ state: 'visible', timeout: 10000 });
      const list = await page.locator('#SetupSaveResult').textContent();
      assert(/NotAnOption/.test(list) && /ArticleTimeout/.test(list), list);
      await shot(page, 'garbage-guide-01-drop-confirm');
      // "Don't save" leaves the file alone
      await page.locator('#SetupDropCancel').click();
      assert(fs.readFileSync(conf).equals(terribleConfig(port, main, true)));
      state.reloading = true;
      await page.locator('#SetupNext').click();
      await page.locator('#SetupDropOk').waitFor({ state: 'visible', timeout: 10000 });
      await page.locator('#SetupDropOk').click();
      await page.locator('.setup-restart').waitFor({ timeout: 10000 });
      const text = fs.readFileSync(conf, 'latin1');
      assert.match(text, /^ControlPassword=example-pass-9$/m);
      assert.match(text, /^Server\d+\.Host=127\.0\.0\.1$/m);
      assert(!/NotAnOption|this line is broken|ArticleTimeout=soon|ParCheck=sometimes/.test(text), 'the bad lines are gone');
      const auth = 'rustyget:example-pass-9';
      await waitFor(async () => (await rpc(base, 'setupstatus', [], auth)).SetupComplete === true, 40000, 'set up after the restart');
      state.reloading = false;
    });
    await context.close();
  } finally {
    await run.stop();
  }
  if (errors.length) run.failures.push(run.name + ': console/page errors: ' + errors.join('; '));
  return run;
}

// ----------------------------------------------------------------------------
// 3. Files that are not usable as a configuration
// ----------------------------------------------------------------------------
async function oddFiles(bin) {
  const run = new Run('oddfiles');
  try {
    const cases = [
      ['missing', p => p, true],
      ['folder', p => { fs.mkdirSync(p); return p; }, true],
      ['empty', p => { fs.writeFileSync(p, ''); return p; }, false],
    ];
    if (!(process.getuid && process.getuid() === 0)) {
      await run.check('an existing file that can\'t be read stops RustyGet', async () => {
        const conf = path.join(run.work, 'unreadable.conf');
        fs.writeFileSync(conf, 'MainDir=/x\n');
        fs.chmodSync(conf, 0);
        const d = run.spawn(bin, ['-c', conf, '-s', '-o', 'OutputMode=log', '-o', 'ControlPort=' + await freePort()], { HOME: run.work }, 'unreadable.out');
        await waitFor(() => d.exitCode !== null, 20000, 'the exit');
        assert.notEqual(d.exitCode, 0);
        assert.match(run.read('unreadable.out'), /Could not read the configuration file .*fix the file's permissions/);
        fs.chmodSync(conf, 0o600);
      });
    }
    for (const [name, make, creates] of cases) {
      await run.check(name + ' configuration file', async () => {
        const port = await freePort();
        const conf = make(path.join(run.work, name + '.conf'));
        const home = path.join(run.work, 'home-' + name);
        fs.mkdirSync(home);
        const out = name + '.out';
        const extra = name === 'missing' || name === 'folder' ? [] : ['-o', 'ControlPort=' + port, '-o', 'MainDir=' + path.join(run.work, 'main-' + name)];
        const d = run.spawn(bin, ['-c', conf, '-s', '-o', 'OutputMode=log', ...extra], { HOME: home }, out);
        const url = name === 'missing' || name === 'folder'
          ? await waitFor(() => (/(http:\/\/127\.0\.0\.1:\d+\/)/.exec(run.read(out)) || [])[1], 20000, 'the address')
          : `http://127.0.0.1:${port}/`;
        const base = new URL(url);
        // an empty or unreadable file means the stock sign-in, which stdout tells;
        // a created one has its own password, told on stdout
        const auth = creates
          ? 'rustyget:' + await waitFor(() => (/password "([A-Za-z0-9]{20})"/.exec(run.read(out)) || [])[1], 20000, 'the password')
          : 'rustyget:tegbzn6789';
        await waitFor(() => rpc(base, 'version', [], auth), 20000, 'the daemon (' + name + ')');
        if (!creates) assert(run.read(out).includes('password "tegbzn6789", the stock password'), 'stdout tells the sign-in');
        assert.equal(d.exitCode, null, 'running');
        const page = await fetch(base.href, auth ? { headers: { Authorization: 'Basic ' + Buffer.from(auth).toString('base64') } } : {});
        assert.equal(page.status, 200, 'the web interface');
        assert.match(await page.text(), /RustyGet/);
        const s = await rpc(base, 'setupstatus', [], auth);
        if (creates) {
          const file = name === 'folder' ? path.join(conf, 'rustyget.conf') : conf;
          assert(fs.statSync(file).isFile(), file + ' created');
          assert.equal(s.ConfigWritable, true);
        }
        d.kill('SIGTERM');
        await new Promise(r => setTimeout(r, 500));
      });
    }
  } finally {
    await run.stop();
  }
  return run;
}

(async () => {
  const bin = path.resolve(process.argv[2] || path.join(__dirname, '..', '..', 'build', 'debug', 'rustyget'));
  assert(fs.existsSync(bin), bin);
  const browser = await chromium.launch(process.env.NZBGET_CHROME_CHANNEL === 'none' ? {} : { channel: process.env.NZBGET_CHROME_CHANNEL || 'chrome' })
    .catch(() => chromium.launch());
  const runs = [];
  try {
    const only = (process.env.FIRST_RUN_ONLY || 'noconfig,noconfig-phone,garbage,garbage-phone,garbage-guide,oddfiles').split(',');
    if (only.includes('noconfig')) runs.push(await noConfig(browser, bin, DESKTOP, 'light', true));
    if (only.includes('noconfig-phone')) runs.push(await noConfig(browser, bin, PHONE, 'dark', false));
    if (only.includes('garbage')) runs.push(await garbage(browser, bin, DESKTOP, 'dark'));
    if (only.includes('garbage-phone')) runs.push(await garbage(browser, bin, PHONE, 'light'));
    if (only.includes('garbage-guide')) runs.push(await garbageGuide(browser, bin));
    if (only.includes('oddfiles')) runs.push(await oddFiles(bin));
  } finally {
    await browser.close();
  }
  const failures = runs.flatMap(r => r.failures);
  const cases = runs.reduce((n, r) => n + r.cases, 0);
  if (failures.length) {
    console.error(`first-run: ${failures.length} of ${cases} cases failed\n` + failures.join('\n') + '\nwork dirs: ' + runs.map(r => r.work).join(' '));
    process.exit(1);
  }
  for (const r of runs) fs.rmSync(r.work, { recursive: true, force: true });
  console.log(`first-run: ${cases} cases passed`);
})().catch(error => { console.error(error); process.exit(1); });
