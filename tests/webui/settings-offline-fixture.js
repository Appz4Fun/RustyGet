// Offline Settings model: real Config, Options, controls and RPC transport over the
// committed nzbget.conf template, with synthetic read replies. No daemon needed.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

async function installOfflineModel(page, base) {
  const root = path.resolve(__dirname, '../../webui');
  const template = fs.readFileSync(path.resolve(root, '../nzbget.conf'), 'utf8');
  const values = new Map(template.split('\n').filter(line => /^[A-Za-z0-9$].*=/.test(line))
    .map(line => { const eq = line.indexOf('='); return [line.slice(0, eq).trim(), line.slice(eq + 1).trim()]; }));
  for (const [name, value] of Object.entries({ 'Server1.Host': 'example.invalid', 'Server1.Name': 'Example provider',
    'Server1.Username': '', 'Server1.Password': '', ControlUsername: '', ControlPassword: '', DestDir: './fixture/dst' })) values.set(name, value);
  const options = [...values].map(([Name, Value]) => ({ Name, Value }));
  const html = fs.readFileSync(path.join(root, 'index.html'), 'utf8').replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '');
  await page.route('**/*', route => {
    const url = new URL(route.request().url());
    if (url.origin !== base.origin) return route.abort();
    if (url.pathname === '/') return route.fulfill({ contentType: 'text/html', body: html });
    if (url.pathname === '/jsonrpc') {
      const rpc = route.request().postDataJSON();
      const result = { loadconfig: options, config: options, configtemplates: [{ Name: '', Template: template }] }[rpc.method];
      assert.notEqual(result, undefined, 'Offline fixture must account for every read RPC: ' + rpc.method);
      return route.fulfill({ contentType: 'application/json', json: { result, id: rpc.id } });
    }
    if (url.pathname === '/combined.css') return route.fulfill({ contentType: 'text/css', body:
      ['lib/bootstrap.css', 'style.css', 'tokens.css', 'ds.css', 'skin.css', 'oxide.css', 'settings.css'].map(file => fs.readFileSync(path.join(root, file), 'utf8')).join('\n') });
    const file = path.resolve(root, '.' + decodeURIComponent(url.pathname));
    if (!file.startsWith(root + path.sep) || !fs.existsSync(file) || !fs.statSync(file).isFile()) return route.abort();
    return route.fulfill({ path: file });
  });
  const boot = async ({ theme, width }) => {
    for (const file of ['lib/jquery.min.js', 'lib/bootstrap.min.js', 'i18n.js']) await page.addScriptTag({ path: path.join(root, file) });
    await page.evaluate(() => new Promise(resolve => I18n.whenReady(resolve)));
    // Keep actual globals/helpers; omit only automatic application startup.
    const index = fs.readFileSync(path.join(root, 'index.js'), 'utf8');
    const first = index.indexOf('$(document).ready(function()');
    const last = index.indexOf('/*** FRONTEND MAIN PAGE', first);
    assert(first > 0 && last > first);
    await page.addScriptTag({ content: index.slice(0, first) + index.slice(last) });
    for (const file of ['util.js', 'fasttable.js', 'config.js', 'tlscipher.js', 'system-health.js']) await page.addScriptTag({ path: path.join(root, file) });
    await page.evaluate(({ options, theme, width }) => {
      document.documentElement.setAttribute('data-theme', theme);
      document.body.classList.toggle('phone', width < 768);
      UISettings.miniTheme = width < 768;
      Options.options = options;
      RPC.rpcUrl = './jsonrpc';
      Config.init({ updateTabInfo: () => {} });
      Config.show(); Config.shown();
      jQuery('#Navbar, #MainTabContent, #ConfigTab').show();
      jQuery('#ConfigTab').addClass('active');
    }, { options, theme, width });
  };
  boot.setSavedValue = (name, value) => {
    const option = options.find(option => option.Name === name);
    if (option) option.Value = value;
    else options.push({ Name: name, Value: value });
  };
  boot.removeSavedValue = name => {
    const index = options.findIndex(option => option.Name === name);
    if (index > -1) options.splice(index, 1);
  };
  return boot;
}

module.exports = { installOfflineModel };
