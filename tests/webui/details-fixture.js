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
      Parameters: params({ '*Unpack:': 'yes', DupeFiles: PRINT, DupeCopyKind: 'twin', DupeHealth: '97', DupeFleet: '1',
        DupeVideo: 'same', DupeParArticles: '6', DupeParFiles: '4112:4,4117:2' })
    }),
    group(42, '02', {
      Status: 'PAUSED', ...sizes('PausedSize', 1730), Health: 994, MessageCount: 0, // nothing logged yet
      Parameters: params({ '*Unpack:': 'yes', DupeFiles: PRINT, DupeKind: 'twin', DupeCopyKind: 'twin', DupeHealth: '99.4', DupeFleet: '2' })
    }),
    group(43, '03', {
      NZBName: `${RELEASE.replace('WEBRip', 'WEB-DL')} [NZBDAV copy 03]`, NZBNicename: `${RELEASE.replace('WEBRip', 'WEB-DL')} [NZBDAV copy 03]`,
      Status: 'PAUSED', ...sizes('FileSize', 1702), ...sizes('RemainingSize', 1702), ...sizes('PausedSize', 1702), FileCount: 36,
      RemainingFileCount: 36, TotalArticles: 2431, Health: 881,
      Parameters: params({ '*Unpack:': 'yes', DupeFiles: 'c07be15a99d2f310-36', DupeKind: 'alt', DupeCopyKind: 'borrowable', DupeAlive: '88.1', DupeFleet: '3', DupeVideo: 'same' })
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
  failed.NZBFilename = `${RELEASE}.copy01.nzb`; // an earlier try of copy 01's own post
  failed.DupeKey = 'TVDB=355730-s02e02'; // the daemon matches dupe keys ignoring ASCII case
  const done = group(32, '', {
    NZBName: 'Example.Completed.Release', NZBNicename: 'Example.Completed.Release', Category: 'Movies', DupeKey: '',
    Status: 'SUCCESS/ALL', ParStatus: 'SUCCESS', UnpackStatus: 'SUCCESS', ...sizes('RemainingSize', 0), RemainingFileCount: 0,
    SuccessArticles: 2466, MessageCount: 0, // history items can report 0 yet still have a log
    Parameters: params({ '*Unpack:': 'yes' }), DestDir: '/mnt/20tb/rustyget/dst/Movies/Example.Completed.Release'
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
  const template = fs.readFileSync(path.resolve(root, '../rustyget.conf'), 'utf8');
  const values = new Map(template.split('\n').filter(line => /^[A-Za-z0-9$].*=/.test(line))
    .map(line => { const eq = line.indexOf('='); return [line.slice(0, eq).trim(), line.slice(eq + 1).trim()]; }));
  for (const [name, value] of Object.entries({
    'Server1.Host': 'news.example.invalid', 'Server1.Name': 'Primary provider', 'Server1.Active': 'yes', 'Server1.Priority': '1',
    'Server2.Host': 'block.example.invalid', 'Server2.Name': 'Block account', 'Server2.Active': 'yes', 'Server2.Level': '1',
    'Category1.Name': 'Movies', 'Category2.Name': 'TV', ControlUsername: '', ControlPassword: '',
    DestDir: '/mnt/20tb/rustyget/dst', InterDir: '/mnt/nvme/rustyget/inter', Unpack: 'yes'
  })) values.set(name, value);
  return { template, options: [...values].map(([Name, Value]) => ({ Name, Value })) };
}

// A `systemstats` reply: a 16-CPU machine and an hour of 5 second samples
// with a download burst, so every system chart has a shape.
function systemStats(rangeSec = 3600) {
  const GiB = 1073741824;
  const now = Math.floor(Date.now() / 1000);
  const interval = rangeSec > 3600 ? 60 : 5;
  const count = Math.min(Math.floor(rangeSec / interval), interval === 5 ? 720 : 1440);
  const samples = [];
  for (let i = count - 1; i >= 0; i--) {
    const t = now - i * interval;
    const phase = (count - i) / count;
    const busy = phase > 0.25 && phase < 0.8 ? 1 : 0.08;
    const wave = 0.85 + 0.15 * Math.sin(phase * 40);
    samples.push([t, Math.round(busy * wave * 118 * MB), Math.round(busy * 58 * wave),
      +(busy * 6.2 * wave + 0.4).toFixed(2), +(busy * 5.1 + 0.6).toFixed(2), +(busy * 3.4 + 0.8).toFixed(2),
      +(busy * 46 * wave + 4).toFixed(2), +(busy * 31 * wave + 0.5).toFixed(2),
      Math.round((9 + busy * 6 * wave) * GiB), Math.round((0.6 + busy * 1.4) * GiB)]);
  }
  const lastRow = samples[samples.length - 1];
  return {
    Time: now, Load: [lastRow[3], lastRow[4], lastRow[5]], CpuCount: 16, CpuPercent: lastRow[6], ProcessCpuPercent: lastRow[7],
    Cpu: { Vendor: 'AMD', Model: 'AMD Ryzen 9 5950X 16-Core Processor', Generation: 'Ryzen 5000 (Zen 3)', Arch: 'x86_64' },
    Memory: { Total: 64 * GiB, Free: 21 * GiB, Available: 64 * GiB - lastRow[8], Used: lastRow[8], Process: lastRow[9] },
    Swap: { Total: 8 * GiB, Free: 7.5 * GiB }, Zswap: true, Zram: false, MemoryCompression: 'zswap',
    Speed: lastRow[1], Connections: lastRow[2],
    History: { Interval: interval, Fields: ['Time', 'Speed', 'Connections', 'Load1', 'Load5', 'Load15', 'Cpu', 'ProcessCpu', 'MemUsed', 'ProcessMem'],
      Samples: samples }
  };
}

const SYSINFO = {
  OS: { Name: 'Debian GNU/Linux', Version: '13' }, CPU: { Model: 'AMD Ryzen 9 5950X 16-Core Processor', Arch: 'x86_64' },
  Network: { PublicIP: '', PrivateIP: '10.0.0.2' }, Tools: [],
  Libraries: [['zipfast', '0.1.0'], ['zipview', '0.1.0'], ['par-rs', '0.2.0'], ['nntp-rs', '0.4.0'], ['flate2', '1.1.11'],
    ['zlib-rs', '0.6.8'], ['zstd', '0.14.3'], ['bzip2', '0.6.1'], ['crc32fast', '1.5.2'], ['quick-xml', '0.41.0'],
    ['md-5', '0.11.0'], ['libc', '0.2.190'], ['openssl', '0.10.82'], ['OpenSSL', '3.5.4']].map(([Name, Version]) => ({ Name, Version }))
};

// Per-download log messages that `loadlog` answers with, by NZBID.
const NZB_LOGS = {
  41: [['INFO', 'Downloading copy 01'], ['WARNING', 'Article 1544 missing on Primary provider, borrowed from twin copy 02']],
  31: [['INFO', 'Collection copy 01 downloaded'], ['ERROR', 'PAR: FAILURE, 74 blocks short']],
  32: [['INFO', 'Collection Example.Completed.Release downloaded'], ['INFO', 'Unpack for Example.Completed.Release successful'],
    ['INFO', 'Moved Example.Completed.Release to /mnt/20tb/rustyget/dst/Movies']]
};

// Files that `listfiles` answers with, by NZBID.
const NZB_FILES = { 41: ['copy01.part01.rar', 'copy01.part02.rar'], 42: ['copy02.7z.001'] };

// Route every request of `page` to committed files and synthetic RPC replies.
async function installDetailsFixture(page, base, writes = [], hooks = {}) {
  const { template, options } = configOptions();
  const groups = fleetGroups();
  const history = historyItems();
  const replies = {
    config: options, loadconfig: options, configtemplates: [{ Name: '', Template: template }],
    status: status(), listgroups: groups, history, log: [], loadlog: [], servervolumes: [],
    version: '26.3', sysinfo: SYSINFO, systemstats: systemStats(), systemhealth: { Alerts: [], Sections: [] }, tlscipherstatus: {},
    hostlookup: { Host: '', Port: 563, Error: '', GeoIP: false, Addresses: [] },
    backbones: { Backbones: [{ Name: 'Omicron', Providers: ['Newshosting'] }, { Name: 'UsenetExpress', Providers: ['NewsDemon'] }],
      Host: '', Match: 'skipped', Provider: null, Matches: [] },
    parpresetdetect: { Running: false, CPUs: 12, MemTotalMB: 40960, MemAvailableMB: 30000, InterFreeMB: 500000, DiskWriteMBps: 1000, DiskReadMBps: 2400,
      Presets: { low: { ParBuffer: 64, ParThreads: 1, ParPauseQueue: true }, medium: { ParBuffer: 256, ParThreads: 6, ParPauseQueue: false },
        large: { ParBuffer: 1024, ParThreads: 0, ParPauseQueue: false }, uber: { ParBuffer: 4096, ParThreads: 14, ParPauseQueue: false },
        detect: { ParBuffer: 4096, ParThreads: 0, ParPauseQueue: false } } },
    readurl: JSON.stringify({ 'stable-version': '26.3', 'testing-version': '26.3' }), listfiles: [], viewfeed: [],
    // set up: no guide and no banner in these checks
    setupstatus: { ConfigFile: './fixture/rustyget.conf', ConfigCreated: false, ConfigWritable: true, ConfigProblem: '',
      PasswordSet: true, PasswordIsStock: false, ControlUsername: 'rustyget', Servers: 1, PathProblems: [], Warnings: [],
      MoreWarnings: 0, SetupComplete: true },
    checkpath: { Path: './fixture', State: 'ok', Message: 'This folder exists and RustyGet can write to it.' },
    // nothing in the fixture file would be dropped by a save
    checkconfig: []
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
      // a test answers a method itself (appendfleet, for one) by returning a result
      if (hooks.answer) {
        const answer = hooks.answer(rpc);
        if (answer !== undefined) {
          writes.push({ method: rpc.method, params: rpc.params });
          return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: answer } });
        }
      }
      if (['editqueue', 'saveconfig', 'append', 'writelog', 'pausedownload', 'resumedownload', 'scan', 'rate'].includes(rpc.method)) {
        writes.push({ method: rpc.method, params: rpc.params });
        if (hooks.holdMutation) await hooks.holdMutation(rpc);
        return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: true } });
      }
      if (rpc.method === 'loadlog') {
        const id = +(rpc.params ? rpc.params[0] : url.searchParams.get(''));
        replies.loadlog = (NZB_LOGS[id] || []).map((m, i) => ({ ID: i + 1, Kind: m[0], Time: Math.floor(Date.now() / 1000) - 600 + i, Text: m[1] }));
      }
      if (rpc.method === 'listfiles') {
        const id = +(rpc.params ? rpc.params[2] : url.searchParams.getAll('')[2]);
        replies.listfiles = (NZB_FILES[id] || []).map((name, i) => ({ ID: id * 100 + i, NZBID: id, Filename: name, Subject: name,
          FileSizeLo: 52428800, FileSizeHi: 0, RemainingSizeLo: 52428800, RemainingSizeHi: 0, PostTime: Math.floor(Date.now() / 1000) - 86400,
          Paused: false, ActiveDownloads: 0, Progress: 0 }));
      }
      if (rpc.method === 'listfiles' && hooks.holdListfiles) {
        const held = replies.listfiles;
        await hooks.holdListfiles(rpc);
        return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: held } });
      }
      if (rpc.method === 'loadlog' && hooks.holdLoadlog) {
        const held = replies.loadlog;
        await hooks.holdLoadlog(rpc);
        return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: held } });
      }
      if (rpc.method === 'systemstats') {
        const range = +(rpc.params ? rpc.params[0] : new URL(request.url()).searchParams.get('')) || 3600;
        replies.systemstats = systemStats(range);
      }
      const result = replies[rpc.method];
      assert.notEqual(result, undefined, 'Details fixture must answer every read RPC: ' + rpc.method);
      const copy = JSON.parse(JSON.stringify(result));
      return route.fulfill({ contentType: 'application/json', json: { id: rpc.id, result: hooks.patchReply ? hooks.patchReply(rpc.method, copy) : copy } });
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

module.exports = { installDetailsFixture, RELEASE, systemStats };
