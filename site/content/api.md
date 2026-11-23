+++
title = "API and compatibility"
description = "RustyGet's JSON-RPC and XML-RPC API: compatibility with media managers, the new appendfleet method and fields, security changes, and the plan for SABnzbd compatibility."
eyebrow = "Reference"
lede = "RustyGet answers the widely supported NZBGet-compatible API, so media managers and the Kodi add-on work unchanged. The API may grow, and RustyGet may add a SABnzbd-compatible API next to it."
+++

## Compatibility

> [!IMPORTANT] Compatibility isn't guaranteed
> Today RustyGet answers every method of the NZBGet-compatible API. Future versions may change or drop parts of it where the old behavior is unsafe, and may add a SABnzbd-compatible API. Release notes list every change that breaks a client.

| Interface | Today | Plan |
|---|---|---|
| JSON-RPC and XML-RPC (NZBGet-compatible) | Every method, with the changes on this page | Kept while clients depend on it. Methods may change where the old behavior is unsafe. |
| JSON-P | Off unless option `JsonP` turns it on; then the methods that read, with the restricted user's access | Kept as an option for old clients |
| `appendfleet` | Available | Kept and extended |
| SABnzbd API | Not available | Under consideration, so that clients that only speak SABnzbd can use RustyGet |
| `rustyget.conf` (an existing `nzbget.conf` is read too) | The classic `Name=value` format, plus new options | Kept |
| Queue and history files | The classic queue formats, with RustyGet's data in a trailer | Kept for now |

## Connecting

The API listens on the web interface's port, 6789 by default, at three paths:

| Protocol | Path |
|---|---|
| JSON-RPC | `/jsonrpc` |
| XML-RPC | `/xmlrpc` |
| JSON-P | `/jsonprpc`, only with option `JsonP=yes` |

Send credentials with HTTP basic authentication, or as the first part of the path, as in `http://host:6789/user:pass/jsonrpc`. Three accounts set what a client may do:

| Account | Options | Access |
|---|---|---|
| Control | `ControlUsername`, `ControlPassword` | Everything |
| Restricted | `RestrictedUsername`, `RestrictedPassword` | The web interface, but no changes to settings, no TLS cipher test, and no passwords |
| Add | `AddUsername`, `AddPassword` | `append`, `appendurl`, `appendfleet`, and `version` only |

> [!WARNING] Use HTTPS on untrusted networks
> Basic authentication only encodes the password; it doesn't encrypt it. Turn on the HTTPS port in **SECURITY** if clients connect over a network you don't control.

JSON-RPC takes positional parameters only: names are ignored, and the order matters. Send every required parameter; a method's optional trailing parameters may be left out. Integers wider than 32 bits are split into `Lo` and `Hi` fields, such as `FileSizeLo` and `FileSizeHi`.

## Methods

This is every method that RustyGet answers. The repository's [API reference](https://github.com/Appz4Fun/RustyGet/blob/HEAD/docs/api/API.md) documents most of them in detail.

| Area | Methods |
|---|---|
| Program | `version`, `shutdown`, `reload`, `dump` |
| Queue | `listgroups`, `listfiles`, `postqueue`, `urlqueue`, `append`, `appendurl`, `appendfleet`, `editqueue`, `scan` |
| History | `history` |
| Status and logs | `status`, `sysinfo`, `systemhealth`, `log`, `clearlog`, `writelog`, `loadlog`, `logupdate`, `servervolumes`, `resetservervolume` |
| Pause and speed | `rate`, `pausedownload`, `resumedownload`, `pausepost`, `resumepost`, `pausescan`, `resumescan`, `scheduleresume`, and the older names `pause`, `pausedownload2`, `resume`, and `resumedownload2` |
| Configuration | `config`, `loadconfig`, `saveconfig`, `configtemplates`, `editserver` |
| RSS feeds | `fetchfeed`, `viewfeed`, `previewfeed` |
| Updates | `checkupdates`, `startupdate` |
| Tests and tools | `testserver`, `testserverspeed`, `testdiskspeed`, `testnetworkspeed`, `readurl` |
| TLS cipher test | `tlscipherstart`, `tlscipherstatus`, `tlsciphercancel`: the **Optimize TLS ciphers** test in the news-server settings, for the control account only |
| XML-RPC only | `system.multicall` |

## appendfleet

`appendfleet` adds every copy of one release in a single call. RustyGet checks each copy on your news servers, queues the best, and keeps the rest as backups. [Duplicate fleets](duplicate-fleets.md#fleet-submission) explains how it ranks them.

```text
appendfleet(DupeKey, Category, Priority, Timeout, Name1, Content1, Name2, Content2, ...)
```

| Parameter | Type | Meaning |
|---|---|---|
| `DupeKey` | string | The key shared by every copy. Empty means RustyGet derives one from the first name. |
| `Category` | string | The category for every member. |
| `Priority` | int | The priority for every member. |
| `Timeout` | int | Required. Seconds allowed for checking; RustyGet keeps the value between 5 and 120. The Kodi add-on sends 45. |
| `NameN` | string | The member's filename, such as `Release.Name.nzb`. |
| `ContentN` | string | The NZB file in base64, or an `http://` or `https://` URL to fetch it from. |

Up to 50 members. Call it over JSON-RPC with POST. XML-RPC callers get an error, and JSON-P, like any GET, can't call it.

The result:

```json
{
  "Chosen": 4182,
  "Complete": true,
  "Reason": "",
  "Members": [
    { "NZBID": 4182, "Name": "Release.Name [copy 01].nzb", "Rank": 1, "Alive": 99, "Status": "QUEUED", "SameAs": 0, "SameAsRank": 0, "Reason": "" },
    { "NZBID": 4183, "Name": "Release.Name [copy 02].nzb", "Rank": 2, "Alive": 97, "Status": "BACKUP", "SameAs": 0, "SameAsRank": 0, "Reason": "" },
    { "NZBID": 0, "Name": "Release.Name [copy 03].nzb", "Rank": 3, "Alive": 99, "Status": "SAME_POSTING", "SameAs": 4182, "SameAsRank": 1, "Reason": "" }
  ]
}
```

| Field | Meaning |
|---|---|
| `Chosen` | The ID of the download that runs: a new member, or a download of the key that was already running. `0` if none. |
| `Complete` | `false` when the time limit cut the check short, so some members are less well measured. |
| `Reason` | Why nothing new is downloading, when that's the case: `ALREADY_QUEUED`, `ALREADY_DOWNLOADED`, `ALL_DEAD`, `INCOMPLETE`, `NO_USABLE_MEMBERS`, `NO_MEMBERS`, `NOT_QUEUED`, `KEY_BUSY`, or `SHUTDOWN`. Retry after `KEY_BUSY` and `SHUTDOWN`. |
| `Members` | One entry per member, in rank order. |
| `Members[].Status` | `QUEUED` (downloading), `BACKUP` (kept in history), `SAME_POSTING` (a copy of another member or of an existing item), `COPY` (the same NZB file as an existing item), `DEAD`, `SKIPPED`, or `ERROR` (unreadable, or holding another video than the fleet: `Reason` says which). |
| `Members[].Alive` | Percent of the sampled articles found, or `-1` if unknown. |
| `Members[].SameAs`, `SameAsRank` | For `SAME_POSTING`: the ID of the item it duplicates, and that member's rank. |

## New fields and parameters

`listgroups` and `history` items have these extra fields:

| Field | Meaning |
|---|---|
| `DupeRecoveredArticles` | Articles recovered from duplicates. |
| `DupeRecoveredBytesLo`, `DupeRecoveredBytesHi` | Bytes recovered by stream repair. |
| `DupeRecoveredHoles` | Holes filled from decompressed donors. |
| `LiveRepairing` | `true` while live repair runs. |

The `listgroups` status and the `postqueue` stage can be `STREAM_REPAIRING`.

RustyGet also stores its findings as NZB parameters, which the API returns in `Parameters`:

| Parameter | Meaning |
|---|---|
| `DupeFiles` | The fingerprint of the posting's par2 file list, or `none` or `lost`. |
| `DupeKind` | `twin`, `twin:N/M`, or `alt`. |
| `DupeSampled`, `DupeSampledOf` | A twin or alt verdict from sampled articles, for postings without par2, and the primary it refers to. |
| `DupeAlive` | Percent of the posting found alive. `0` means dead, and failover skips it. Missing means not measured (unverified). |
| `DupeAliveSample` | `found/asked`: the sampled articles `DupeAlive` rests on, such as `0/40`. |
| `DupeAliveCut` | `yes` when the copy's check ran out of its time budget (60 s per copy) before every sampled article answered: `DupeAlive` is the share measured so far. |
| `DupeHealth` | The measured health of a backup. |
| `DupeProjected`, `DupeParked` | The projected health in percent of a download judged from a sample (dead or doomed), and the share of its tried articles that arrived when it was kept as a donor. |
| `DupeFleet` | The member's rank in its `appendfleet` call. |
| `DupeHeld`, `DupeProbing` | `yes` on a copy of a fleet held in Downloads (paused, priority -100), and while the fleet's probe has not classified it yet. |
| `DupeFleetOrder`, `DupeFleetPrimary` | The copy's place in its fleet (`0` the primary, `1` the copy failover takes next; a copy still being probed is never `1`), and the primary's NZBID. |
| `DupeEase` | How easy the copy is to borrow into: `0` bare or split files, `1` stored archive, `2` compressed, `3` encrypted. |

The `Health` field keeps NZBGet's meaning, so existing clients keep working:
it counts every article not tried yet as good, which is why a held copy of a
fleet, dead ones included, reports 1000 until it downloads. A client that
shows how alive a copy is should use `DupeProjected`, then `DupeParked`, then
`DupeAlive` when present, as the web interface does (`DownloadsUI.shownHealth`).

## Changes to existing methods

RustyGet closes several security problems of the classic API. Clients that rely on the old behavior may need changes:

- **JSON-P off by default.** Any web page can make a browser send a JSON-P request, with the login the browser keeps, and read the answer. RustyGet answers JSON-P only with option `JsonP=yes`, and then with the restricted user's access, so user names, passwords, and API keys come back as `***`. Otherwise every JSON-P request answers error 403. The classic API answered JSON-P with full access.
- **POST for methods that change something.** Plain GET and JSON-P run only the methods that read: `config`, `configtemplates`, `dump`, `history`, `listfiles`, `listgroups`, `loadconfig`, `loadlog`, `log`, `logupdate`, `postqueue`, `servervolumes`, `status`, `sysinfo`, `systemhealth`, `urlqueue`, and `version`. Every other method answers error 4 there. `readurl`, `testserverspeed`, `testdiskspeed`, and `testnetworkspeed` joined the methods that need POST. In the classic API, JSON-P ran every method, so a `<script>` tag on any web page could edit the queue, change settings, or shut the program down with the browser's saved login.
- **Access checks in multicalls.** The XML-RPC `system.multicall` method runs each call with the caller's access level. In the classic API, a restricted account could read passwords through it.
- **Range checks.** Integers outside the 32-bit range are rejected instead of wrapping around. `editqueue` checks scores, priorities, parameters, and duplicate modes.
- **Safe configuration saves.** `saveconfig` refuses an empty or malformed option list instead of truncating the file.
- **Decoding.** `append` decodes post-processing parameters, so passwords with quotes work. Numeric escapes decode to UTF-8, and characters outside the Basic Multilingual Plane come out as valid JSON.
- **Correct output.** Request IDs are echoed in full, `servervolumes` returns valid JSON, and `resetservervolume` persists the reset.
