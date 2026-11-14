// Offline app fixture for the download details panel: the committed WebUI with its
// real startup, renderers and RPC transport, answering every RPC with synthetic
// replies. A dupe fleet (primary, twin, borrowable alt) sits in the queue and a
// failed copy of the same release sits in history. Nothing reaches a daemon.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '../../webui');
const MB = 1048576;

function sizes(prefix, mb) {
  const bytes = Math.round(mb * MB);
  return { [prefix + 'Lo']: bytes % 4294967296, [prefix + 'Hi']: Math.floor(bytes / 4294967296), [prefix + 'MB']: Math.round(mb) };
}

function params(map) {
  return Object.entries(map).map(([Name, Value]) => ({ Name, Value }));
}

const RELEASE = 'The.Wheel.of.Time.S02E02.2160p.HDR10Plus.DV.WEBRip.6CH.x265.HEVC-PSA';
const PRINT = 'a3f91c02d4e8b711-30';

function group(id, copy, extra) {
  const now = Math.floor(Date.now() / 1000);
  const fileMB = 1730;
  const g = {
    NZBID: id, FirstID: id * 10, LastID: id * 10 + 29, Kind: 'NZB', URL: '',
    NZBName: `${RELEASE} [NZBDAV copy ${copy}]`, NZBNicename: `${RELEASE} [NZBDAV copy ${copy}]`,
    NZBFilename: `${RELEASE}.copy${copy}.nzb`, DestDir: `/mnt/nvme/rustyget/inter/${RELEASE}.#${id}`, FinalDir: '',
    Category: 'TV', Status: 'QUEUED', ActiveDownloads: 0, MinPriority: 0, MaxPriority: 0,
    ParStatus: 'NONE', ExParStatus: 'NONE', UnpackStatus: 'NONE', MoveStatus: 'NONE', ScriptStatus: 'NONE',
    DeleteStatus: 'NONE', MarkStatus: 'NONE', UrlStatus: 'NONE',
    ...sizes('FileSize', fileMB), ...sizes('RemainingSize', fileMB), ...sizes('PausedSize', 0), ...sizes('DownloadedSize', 0),
    FileCount: 30, RemainingFileCount: 30, RemainingParCount: 4, MinPostTime: now - 86400 * 3, MaxPostTime: now - 86400 * 3,
    TotalArticles: 2466, SuccessArticles: 0, FailedArticles: 0, Health: 1000, CriticalHealth: 942,
    DupeKey: 'tvdb=355730-S02E02', DupeScore: 2160, DupeMode: 'SCORE', Deleted: false,
    DownloadTimeSec: 0, PostTotalTimeSec: 0, ParTimeSec: 0, RepairTimeSec: 0, UnpackTimeSec: 0,
    MessageCount: 4, ExtraParBlocks: 0, ScriptStatuses: [], ServerStats: [], PostInfoText: 'NONE',
    PostStageProgress: 0, PostStageTimeSec: 0, Log: [],
    DupeRecoveredArticles: 0, DupeRecoveredBytesLo: 0, DupeRecoveredBytesHi: 0, DupeRecoveredHoles: 0,
    Parameters: params({ '*Unpack:': 'yes' })
  };
  return Object.assign(g, extra);
}

function fleetGroups() {
  return [
    group(41, '01', {
      Status: 'DOWNLOADING', ActiveDownloads: 12, LiveRepairing: true, ...sizes('RemainingSize', 612), ...sizes('DownloadedSize', 1118),
      RemainingFileCount: 11, SuccessArticles: 1544, FailedArticles: 49, Health: 970, DownloadTimeSec: 211,
      DupeRecoveredArticles: 41, ...sizes('DupeRecoveredBytes', 29.4), DupeRecoveredHoles: 3,
      ServerStats: [{ ServerID: 1, SuccessArticles: 1290, FailedArticles: 31 }, { ServerID: 2, SuccessArticles: 254, FailedArticles: 18 }],
      Parameters: params({ '*Unpack:': 'yes', DupeFiles: PRINT, DupeCopyKind: 'twin', DupeHealth: '97', DupeFleet: '1' })
    }),
    group(42, '02', {
      Status: 'PAUSED', ...sizes('PausedSize', 1730), Health: 994,
      Parameters: params({ '*Unpack:': 'yes', DupeFiles: PRINT, DupeKind: 'twin', DupeCopyKind: 'twin', DupeHealth: '99.4', DupeFleet: '2' })
    }),
    group(43, '03', {
      NZBName: `${RELEASE.replace('WEBRip', 'WEB-DL')} [NZBDAV copy 03]`, NZBNicename: `${RELEASE.replace('WEBRip', 'WEB-DL')} [NZBDAV copy 03]`,
      Status: 'PAUSED', ...sizes('FileSize', 1702), ...sizes('RemainingSize', 1702), ...sizes('PausedSize', 1702), FileCount: 36,
      RemainingFileCount: 36, TotalArticles: 2431, Health: 881,
      Parameters: params({ '*Unpack:': 'yes', DupeFiles: 'c07be15a99d2f310-36', DupeKind: 'alt', DupeCopyKind: 'borrowable', DupeAlive: '88.1', DupeFleet: '3' })
    }),
    group(44, '', {
      NZBName: 'Example.Documentary.2160p', NZBNicename: 'Example.Documentary.2160p', Category: 'Movies',
      DupeKey: '', DupeScore: 0, Status: 'QUEUED', TotalArticles: 812, FileCount: 12, RemainingFileCount: 12,
      ...sizes('FileSize', 640), ...sizes('RemainingSize', 640), Parameters: params({ '*Unpack:': 'yes' })
    })
  ];
}

function historyItems() {
  const now = Math.floor(Date.now() / 1000);
  const failed = group(31, '00', {
    Status: 'FAILURE/PAR', ParStatus: 'FAILURE', UnpackStatus: 'NONE', ...sizes('RemainingSize', 0),
    ...sizes('DownloadedSize', 1690), RemainingFileCount: 1, SuccessArticles: 2392, FailedArticles: 74, Health: 970,
    DownloadTimeSec: 402, PostTotalTimeSec: 95, ParTimeSec: 95, DupeRecoveredArticles: 12, ...sizes('DupeRecoveredBytes', 8.6),
    Parameters: params({ '*Unpack:': 'yes', DupeFiles: PRINT, DupeCopyKind: 'twin', DupeHealth: '97' })
  });
  failed.NZBName = `${RELEASE} [NZBDAV copy 01]`; failed.NZBNicename = failed.NZBName;
  failed.DupeKey = 'TVDB=355730-s02e02'; // the daemon matches dupe keys ignoring ASCII case
  const done = group(32, '', {
    NZBName: 'Example.Completed.Release', NZBNicename: 'Example.Completed.Release', Category: 'Movies', DupeKey: '',
    Status: 'SUCCESS/ALL', ParStatus: 'SUCCESS', UnpackStatus: 'SUCCESS', ...sizes('RemainingSize', 0), RemainingFileCount: 0,
    SuccessArticles: 2466, Parameters: params({ '*Unpack:': 'yes' }), DestDir: '/mnt/20tb/rustyget/dst/Movies/Example.Completed.Release'
  });
  return [failed, done].map((h, i) => Object.assign(h, { ID: h.NZBID, Name: h.NZBName, HistoryTime: now - 600 * (i + 1), RetryData: false }));
}

function status() {
  return {
    ...sizes('RemainingSize', 4684), ...sizes('ForcedSize', 0), ...sizes('DownloadedSize', 1118), ...sizes('MonthSize', 98000),
    ...sizes('DaySize', 3100), ...sizes('ArticleCache', 120), DownloadRate: 41943040, DownloadRateLo: 41943040, DownloadRateHi: 0,
    AverageDownloadRate: 39845888, AverageDownloadRateLo: 39845888, AverageDownloadRateHi: 0, DownloadLimit: 0,
    ThreadCount: 52, PostJobCount: 0, UrlCount: 0, UpTimeSec: 86400, DownloadTimeSec: 3600, ServerPaused: false,
    DownloadPaused: false, Download2Paused: false, ServerStandBy: false, PostPaused: false, ScanPaused: false,
    QuotaReached: false, FreeDiskSpaceLo: 0, FreeDiskSpaceHi: 400, FreeDiskSpaceMB: 1677721, ServerTime: Math.floor(Date.now() / 1000),
    ResumeTime: 0, FeedActive: false, QueueScriptCount: 0,
    NewsServers: [{ ID: 1, Active: true }, { ID: 2, Active: true }]
  };
}

function configOptions() {
  const template = fs.readFileSync(path.resolve(root, '../nzbget.conf'), 'utf8');
  const values = new Map(template.split('\n').filter(line => /^[A-Za-z0-9$].*=/.test(line))
    .map(line => { const eq = line.indexOf('='); return [line.slice(0, eq).trim(), line.slice(eq + 1).trim()]; }));
  for (const [name, value] of Object.entries({
    'Server1.Host': 'news.example.invalid', 'Server1.Name': 'Primary provider', 'Server1.Active': 'yes', 'Server1.Level': '0',
    'Server2.Host': 'block.example.invalid', 'Server2.Name': 'Block account', 'Server2.Active': 'yes', 'Server2.Level': '1',
    'Category1.Name': 'Movies', 'Category2.Name': 'TV', ControlUsername: '', ControlPassword: '',
    DestDir: '/mnt/20tb/rustyget/dst', InterDir: '/mnt/nvme/rustyget/inter', Unpack: 'yes', UpdateCheck: 'none'
  })) values.set(name, value);
  return { template, options: [...values].map(([Name, Value]) => ({ Name, Value })) };
}

// Route every request of `page` to committed files and synthetic RPC replies.
async function installDetailsFixture(page, base, writes = [], hooks = {}) {
  const { template, options } = configOptions();
  const groups = fleetGroups();
  const history = historyItems();
  const replies = {
    config: options, loadconfig: options, configtemplates: [{ Name: '', Template: template }],
    status: status(), listgroups: groups, history, log: [], loadlog: [], servervolumes: [],
    version: '26.3', sysinfo: {}, systemhealth: { Alerts: [], Sections: [] }, tlscipherstatus: {},
    readurl: JSON.stringify({ 'stable-version': '26.3', 'testing-version': '26.3' }), listfiles: [], viewfeed: []
  };
  await page.route('**/*', async route => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.origin !== base.origin) return route.abort();
    if (url.pathname === '/' || url.pathname === '/index.html') {
      return route.fulfill({ contentType: 'text/html', body: fs.readFileSync(path.join(root, 'index.html'), 'utf8') });
    }
    if (/\/jsonrpc(\/|$)/.test(url.pathname)) {
      const rpc = request.method() === 'POST' ? request.postDataJSON() : { id: 1, method: url.pathname.split('/').pop() };
      if (['editqueue', 'saveconfig', 'append', 'writelog', 'pausedownload', 'resumedownload', 'scan', 'rate'].includes(rpc.method)) {
        writes.push({ method: rpc.method, params: rpc.params });
        if (hooks.holdMutation) await hooks.holdMutation(rpc);
        return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: true } });
      }
      const result = replies[rpc.method];
      assert.notEqual(result, undefined, 'Details fixture must answer every read RPC: ' + rpc.method);
      return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: JSON.parse(JSON.stringify(result)) } });
    }
    if (url.pathname === '/combined.css' || url.pathname === '/combined.js') {
      const files = decodeURIComponent(url.search.slice(1)).split('+');
      return route.fulfill({ contentType: url.pathname.endsWith('.css') ? 'text/css' : 'application/javascript',
        body: files.map(file => fs.readFileSync(path.join(root, file), 'utf8')).join('\n') });
    }
    const file = path.resolve(root, '.' + decodeURIComponent(url.pathname));
    if (!file.startsWith(root + path.sep) || !fs.existsSync(file) || !fs.statSync(file).isFile()) return route.abort();
    return route.fulfill({ path: file });
  });
}

module.exports = { installDetailsFixture, RELEASE };
