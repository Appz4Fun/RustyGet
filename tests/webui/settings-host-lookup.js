// News server host lookup: typing a host lists its addresses with country, network
// and connect time. Auto (the fastest) is the default; picking an address writes it
// as Server.Address, keeping the name in Host for the certificate check. The login
// can be checked on each address in turn. An address typed as Host (the old pin)
// moves to Address with "Use the name again".
// Offline: the committed WebUI over synthetic RPC replies (details-fixture);
// hostlookup is intercepted.
// Usage: node tests/webui/settings-host-lookup.js http://127.0.0.1:17999/
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
      await context.addInitScript(() => localStorage.setItem('SettingsMode', 'expert'));
      const page = await context.newPage();
      const errors = [], lookups = [], tests = [];
      let geoip = true;
      page.on('pageerror', e => errors.push(e.message));
      await installDetailsFixture(page, base, [], { patchReply: (method, result) => {
        if (method === 'config' || method === 'loadconfig') {
          const set = (name, value) => { const o = result.find(x => x.Name === name); if (o) o.Value = value; else result.push({ Name: name, Value: value }); };
          set('Server1.Encryption', 'yes'); set('Server1.CertVerification', 'strict');
        }
        return result;
      } });
      await page.route(/\/jsonrpc/, async route => {
        const rpc = route.request().method() === 'POST' ? route.request().postDataJSON() : null;
        if (rpc && rpc.method === 'testserver') {
          tests.push(rpc.params);
          await new Promise(r => setTimeout(r, 50));
          return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: rpc.params[8] === '192.0.2.10' ? '' : 'Authentication failed for user' } });
        }
        if (!rpc || rpc.method !== 'hostlookup') return route.fallback();
        lookups.push(rpc.params);
        const [host, port] = rpc.params;
        if (host === 'slow.example.com') {
          await new Promise(r => setTimeout(r, 300));
          return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: { Host: host, Port: port, Error: '', GeoIP: geoip,
            Addresses: [{ IP: '198.51.100.7', Family: 4, Country: '', CountryName: '', ASN: 0, Org: '', LatencyMs: 5 }] } } });
        }
        if (host === 'mixed.example.com') {
          return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: { Host: host, Port: port, Error: '', GeoIP: geoip,
            Addresses: [{ IP: '::ffff:192.0.2.1', Family: 6, Country: '', CountryName: '', ASN: 0, Org: '', LatencyMs: 3 }] } } });
        }
        const result = host === 'nowhere.invalid' ? { Host: host, Port: port, Error: 'Could not resolve nowhere.invalid', GeoIP: geoip, Addresses: [] } :
          { Host: host, Port: port, Error: '', GeoIP: geoip, Addresses: [
            { IP: '192.0.2.10', Family: 4, Country: geoip ? 'NL' : '', CountryName: geoip ? 'Netherlands' : '', ASN: geoip ? 64500 : 0, Org: geoip ? 'Example Net' : '', LatencyMs: 12.4 },
            { IP: '2001:db8::10', Family: 6, Country: '', CountryName: '', ASN: 0, Org: '', LatencyMs: null }
          ].filter(a => host === 'news.example.com' || a.IP === host) };
        return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result } });
      });
      await page.goto(base.href);
      await page.locator('#DownloadsTable tbody tr').first().waitFor();
      await page.evaluate(() => $('a[href="#ConfigTab"]').first().click());
      await page.waitForFunction(() => document.querySelector('#ConfigMode'));
      await page.evaluate(() => Config.showSection('NEWS-SERVERS', false));
      await page.waitForTimeout(200);
      await page.locator('.config-list-row.NEWS-SERVERS.multiid1 .config-list-edit').click();
      await page.waitForTimeout(150);
      const check = async (label, action) => {
        cases++;
        try { await action(); } catch (error) { failures.push(`${theme}/${width}/${label}: ${error.message}`); }
      };
      const host = page.locator('#S_Server1_Host');
      const box = page.locator('.config-host-lookup[data-multiid="1"]');
      const address = (n = 1) => page.evaluate(n => Config.getOptionValue(Config.findOptionByName('Server' + n + '.Address')), n);
      const certValue = () => page.evaluate(() => String(Config.getOptionValue(Config.findOptionByName('Server1.CertVerification'))).toLowerCase());

      await check('typing a host lists its addresses', async () => {
        await host.fill('news.example.com');
        await page.waitForFunction(() => /2 addresses/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        assert.deepEqual(lookups.pop(), ['news.example.com', 563]);
        const text = await box.textContent();
        assert.match(text, /192\.0\.2\.10/);
        assert.match(text, /NL Netherlands · AS64500 Example Net · 12 ms/);
        assert.match(text, /2001:db8::10/);
        assert.match(text, /no answer/);
        assert.match(text, /IP location data by DB-IP/);
        assert.equal(await box.locator('input[type=radio]').count(), 3);
        assert(await box.locator('input[value=""]').isChecked(), 'Auto is the default');
        assert.match(text, /Auto: the fastest/);
        assert.equal(await box.locator('button', { hasText: 'Check login on each address' }).count(), 1);
      });

      await check('picking an address sets Address and keeps the name and the certificate check', async () => {
        await box.locator('input[value="192.0.2.10"]').click();
        await page.waitForFunction(() => Config.getOptionValue(Config.findOptionByName('Server1.Address')) === '192.0.2.10', null, { timeout: 5000 });
        await page.waitForFunction(() => /2 addresses for news/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        assert.equal(await host.inputValue(), 'news.example.com');
        assert.equal(await certValue(), 'strict');
        assert(await box.locator('input[value="192.0.2.10"]').isChecked(), 'the pinned address is shown as picked');
        await box.locator('input[value=""]').click();
        await page.waitForFunction(() => Config.getOptionValue(Config.findOptionByName('Server1.Address')) === '', null, { timeout: 5000 });
        assert.equal(await host.inputValue(), 'news.example.com');
      });

      await check('the login is checked on each address in turn', async () => {
        await page.waitForFunction(() => /2 addresses for news/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        tests.length = 0;
        await box.locator('button', { hasText: 'Check login on each address' }).click();
        await page.waitForFunction(() => $('.config-host-lookup[data-multiid="1"] .config-host-lookup__check[data-state="error"]').length === 1, null, { timeout: 5000 });
        assert.deepEqual(tests.map(t => t[8]), ['192.0.2.10', '2001:db8::10']);
        assert.equal(tests[0][0], 'news.example.com', 'the host name is kept for TLS');
        assert.match(await box.locator('[data-ip="192.0.2.10"] .config-host-lookup__check').textContent(), /login OK/);
        assert.match(await box.locator('[data-ip="2001:db8::10"] .config-host-lookup__check').textContent(), /Authentication failed/);
        assert.equal(await box.locator('button', { hasText: 'Check login on each address' }).isDisabled(), false);
      });

      await check('an address in Host (the old pin) moves to Address with "Use the name again"', async () => {
        await host.fill('192.0.2.10');
        await page.waitForFunction(() => /Host is an address \(news\.example\.com\)/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        await box.locator('input[value="\u0001name"]').click();
        await page.waitForFunction(() => $('#S_Server1_Host').val() === 'news.example.com', null, { timeout: 5000 });
        assert.equal(await address(), '192.0.2.10');
        await page.waitForFunction(() => /2 addresses for news/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        assert(await box.locator('input[value="192.0.2.10"]').isChecked());
        await box.locator('input[value=""]').click();
        await page.waitForFunction(() => Config.getOptionValue(Config.findOptionByName('Server1.Address')) === '', null, { timeout: 5000 });
      });

      await check('a pinned address the host no longer has is shown as gone', async () => {
        await page.evaluate(() => Config.setOptionValue(Config.findOptionByName('Server1.Address'), '203.0.113.9'));
        await host.fill('slow.example.com');
        await page.waitForFunction(() => /198\.51\.100\.7/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        await host.fill('news.example.com');
        await page.waitForFunction(() => /Not an address of this host now/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        assert(await box.locator('input[value="203.0.113.9"]').isChecked());
        await box.locator('input[value=""]').click();
        await page.waitForFunction(() => Config.getOptionValue(Config.findOptionByName('Server1.Address')) === '', null, { timeout: 5000 });
      });

      await check('a name that does not resolve says so, and no location files says so', async () => {
        await host.fill('nowhere.invalid');
        await page.waitForFunction(() => /Could not resolve/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        geoip = false;
        await host.fill('news.example.com');
        await page.waitForFunction(() => /DB-IP Lite files/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        assert.doesNotMatch(await box.textContent(), /AS64500/);
        geoip = true;
      });

      await check('a new port looks the host up again', async () => {
        await page.waitForFunction(() => /2 addresses/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        await page.locator('#S_Server1_Port').fill('119');
        await page.waitForTimeout(1000);
        assert.deepEqual(lookups[lookups.length - 1], ['news.example.com', 119]);
      });

      await check('typing again makes a pending reply stale at once', async () => {
        await host.fill('slow.example.com');
        await page.waitForFunction(() => /Looking up slow/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        await host.fill('news.example.com');
        await page.waitForTimeout(450);
        assert.doesNotMatch(await box.textContent(), /198\.51\.100\.7/, 'the slow reply is not shown');
        await page.waitForFunction(() => /2 addresses for news/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
      });

      await check('a one-word name is looked up once the field is left', async () => {
        const before = lookups.length;
        await host.fill('nntp-proxy');
        await page.waitForTimeout(900);
        assert.equal(lookups.length, before, 'not while it may still be typed');
        await host.dispatchEvent('change');
        await page.waitForFunction(n => n.length > 0 && n[n.length - 1][0] === 'nntp-proxy', lookups, { timeout: 5000 }).catch(() => {});
        assert.equal(lookups[lookups.length - 1][0], 'nntp-proxy');
        await host.fill('news.example.com');
        await page.waitForFunction(() => /2 addresses for news/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
      });

      const ipVersion = v => page.evaluate(v => Config.switchClick($('#S_Server1_IpVersion .btn').filter(function() { return String($(this).attr('data-value')).toLowerCase() === v; })[0]), v);
      await check('a server limited to IPv4 lists only IPv4 addresses', async () => {
        await ipVersion('ipv4');
        await page.waitForFunction(() => /1 address for news/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        assert.doesNotMatch(await box.textContent(), /2001:db8::10/);
        await page.evaluate(() => $('#S_Server1_IpVersion .btn').removeClass('btn-primary').filter(function() { return String($(this).attr('data-value')).toLowerCase() === 'ipv6'; }).addClass('btn-primary'));
        await page.evaluate(() => $('.config-host-lookup[data-multiid="1"]').find('input[value="192.0.2.10"]').prop('checked', true).trigger('change'));
        await page.waitForTimeout(300);
        assert.equal(await page.evaluate(() => String(Config.getOptionValue(Config.findOptionByName('Server1.IpVersion'))).toLowerCase()), 'auto',
          'an IPv4 address picked while the server is IPv6 only sets it to Automatic');
        assert.equal(await address(), '192.0.2.10');
        await page.evaluate(() => $('.config-host-lookup[data-multiid="1"]').find('input[value=""]').prop('checked', true).trigger('change'));
        await page.waitForFunction(() => Config.getOptionValue(Config.findOptionByName('Server1.Address')) === '', null, { timeout: 5000 });
        await page.waitForFunction(() => /2 addresses for news/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
      });

      await check('switching IP version while another host is looked up shows no old addresses', async () => {
        await host.fill('slow.example.com');
        await page.waitForFunction(() => /Looking up slow/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        await ipVersion('ipv4');
        assert.doesNotMatch(await box.textContent(), /192\.0\.2\.10/, 'the reply for news.example.com is not shown for slow.example.com');
        await page.waitForFunction(() => /198\.51\.100\.7/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        await ipVersion('auto');
        await host.fill('news.example.com');
        await page.waitForFunction(() => /2 addresses for news/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
      });

      await check('turning encryption on times the new port', async () => {
        const encryption = v => page.evaluate(v => Config.switchClick($('#S_Server1_Encryption .btn').filter(function() { return String($(this).attr('data-value')).toLowerCase() === v; })[0]), v);
        await page.locator('#S_Server1_Port').fill('119');
        await page.waitForFunction(n => n.length && n[n.length - 1][1] === 119, lookups, { timeout: 5000 }).catch(() => {});
        await encryption('no');
        await encryption('yes');
        assert.equal(await page.inputValue('#S_Server1_Port'), '563');
        await page.waitForFunction(n => n.length && n[n.length - 1][1] === 563, lookups, { timeout: 5000 }).catch(() => {});
        assert.deepEqual(lookups[lookups.length - 1], ['news.example.com', 563]);
        await page.waitForFunction(() => /2 addresses for news/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
      });

      await check('picking an IPv6 address with an IPv4 part sets it as Address', async () => {
        const cert = v => page.evaluate(v => Config.switchClick($('#S_Server1_CertVerification .btn').filter(function() { return String($(this).attr('data-value')).toLowerCase() === v; })[0]), v);
        await host.fill('mixed.example.com');
        await page.waitForFunction(() => /1 address for mixed/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        await cert('strict');
        await page.evaluate(() => $('.config-host-lookup[data-multiid="1"]').find('input[value="::ffff:192.0.2.1"]').prop('checked', true).trigger('change'));
        await page.waitForFunction(() => Config.getOptionValue(Config.findOptionByName('Server1.Address')) === '::ffff:192.0.2.1', null, { timeout: 5000 });
        assert.equal(await page.inputValue('#S_Server1_Host'), 'mixed.example.com');
        assert.equal(await certValue(), 'strict');
        await page.evaluate(() => $('.config-host-lookup[data-multiid="1"]').find('input[value=""]').prop('checked', true).trigger('change'));
        await page.waitForFunction(() => Config.getOptionValue(Config.findOptionByName('Server1.Address')) === '', null, { timeout: 5000 });
        await host.fill('news.example.com');
        await page.waitForFunction(() => /2 addresses for news/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
      });

      await check('a scoped IPv6 address typed directly counts as an address', async () => {
        await host.fill('fe80::1%eth0');
        await page.waitForFunction(() => /Host is an address/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
        await host.fill('news.example.com');
        await page.waitForFunction(() => /2 addresses for news/.test($('.config-host-lookup[data-multiid="1"]').text()), null, { timeout: 5000 });
      });

      await check('moving a server keeps its lookup with it', async () => {
        await page.evaluate(() => Config.moveSet($('<a data-multiid="1"></a>')[0], 'Server', 'NEWS-SERVERS', 'down'));
        await page.waitForTimeout(200);
        assert.equal(await page.inputValue('#S_Server2_Host'), 'news.example.com');
        assert.equal(await page.evaluate(() => $('#S_Server2_Host').siblings('.config-host-lookup').attr('data-multiid')), '2');
        await page.evaluate(() => $('#S_Server2_Host').siblings('.config-host-lookup').find('input[value="192.0.2.10"]').prop('checked', true).trigger('change'));
        await page.waitForTimeout(300);
        assert.equal(await address(2), '192.0.2.10');
        assert.equal(await page.inputValue('#S_Server2_Host'), 'news.example.com');
        assert.equal(await page.inputValue('#S_Server1_Host'), 'block.example.invalid', 'the other server is untouched');
        assert.equal(await address(1), '', 'the other server keeps Auto');
      });

      await check('an address typed directly shows its details', async () => {
        await page.locator('.config-list-row.NEWS-SERVERS.multiid1 .config-list-edit').click();
        await page.waitForTimeout(150);
        await host.fill('192.0.2.10');
        await page.waitForFunction(() => /Host is an address/.test($('#S_Server1_Host').siblings('.config-host-lookup').text()), null, { timeout: 5000 });
        assert.match(await page.evaluate(() => $('#S_Server1_Host').siblings('.config-host-lookup').text()), /NL Netherlands · AS64500 Example Net · 12 ms/);
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
  console.log(`settings-host-lookup: ${cases} cases passed`);
})();
