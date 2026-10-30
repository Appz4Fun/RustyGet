## Status of this document

The document describes methods available in NZBGet `version 13.0 and later`. The document is updated after a new stable version is out. Current testing version may have methods or fields not described here; these methods or fields may change before getting stable.

## Protocols

NZBGet supports `XML-RPC`, `JSON-RPC` and `JSON-P-RPC`. RPC-protocols allow to control the program from other applications. Many programming languages have libraries, which make the usage of `XML-RPC`, `JSON-RPC` and `JSON-P-RPC` very simple. The implementations of all three protocols in NZBGet are equal: all methods are identical, except that `JSON-P-RPC` is off unless option `JsonP` turns it on, and then runs only the methods that read (see below). You may choose the protocol, which is better supported by your programming language.

## Authentication

NZBGet has three pairs of username/password:

- options `ControlUsername` and `ControlPassword` - Full access, usually used when connecting to web-interface;
- options `RestrictedUsername` and `RestrictedPassword` - `v15.0` Restricted user can control the program with few restrictions. He has access to web-interface and can see most program settings. He can not change program settings and can not view security related options or options provided by extension scripts. In terms of RPC-API the user:
  - cannot use method `saveconfig`;
  - methods `config` and `loadconfig` return string `***` for options those content is protected from the user.
- options `AddUsername` and `AddPassword` - `v15.0` This user has only two permissions:
  - add new downloads using RPC-method `append`;
  - check program version using RPC-method `version`.


The RPC server uses HTTP basic authentication. The URL would be something like:

- for **XML-RPC**: `http://username:password@localhost:6789/xmlrpc`
- for **JSON-RPC**: `http://username:password@localhost:6789/jsonrpc`
- for **JSON-P-RPC**: `http://username:password@localhost:6789/jsonprpc` (only with option `JsonP` active)
 
If HTTP basic authentication is somewhat problematic the username/password can also be passed in the URL as the first part of the path:

- `http://localhost:6789/username:password/xmlrpc`

`Security warning`: HTTP authentication is not secure. Although the password is encoded using Base64 it is not encrypted. For secure communication use HTTPS (needs to be explicitly enabled in NZBGet settings by user).

## Features and limitations

- Multicalls are supported for `XML-RPC` and not supported for `JSON-RPC`.
- Introspection is not supported.
- Only positional parameters are supported in `JSON-RPC`. Named parameters are not supported. Therefore parameter names are ignored but the order of parameters is important. All parameters are mandatory.
- Each call to `JSON-P-RPC` has one additional parameter - the name of callback-function. This parameter must be named `callback` and must be passed first (before any other parameters).
- `JSON-P-RPC` is answered only when option `JsonP` is active; otherwise every call answers error 403. Any web page can make a browser send a `JSON-P` request with the login it keeps and read the answer, so `JSON-P` always runs with the access of the restricted user: protected options such as user names, passwords and API keys come back as `***`.
- `JSON-P-RPC` runs only the methods that read: `config`, `configtemplates`, `dump`, `history`, `listfiles`, `listgroups`, `loadconfig`, `loadextensions`, `loadlog`, `log`, `logscript`, `logupdate`, `postqueue`, `servervolumes`, `status`, `sysinfo`, `systemhealth`, `urlqueue` and `version`. Any web page can make a browser send a `JSON-P` request, so every other method answers error 4 there; call it with `JSON-RPC` or `XML-RPC` using HTTP method POST. The same methods are the only ones a plain GET request may call.
- 64 bit integers are returned in two separate fields `Hi` and `Lo` (for example `FileSizeHi` and `FileSizeLo`). These fields are unsigned 32 bit integers. Although dynamic languages such as PHP or Python have no problems with these fields the `XML-RPC` specification does not allow unsigned integers. This may cause troubles in statically typed languages such as Java or C++ if `XML-RPC`-parser expects only signed integers in 32 bit range. As a solution use `JSON-RPC` (which does allow unsigned integers) instead of `XML-RPC`.

## Program control

- [version](#api-method-version)
- [shutdown](#api-method-shutdown)
- [reload](#api-method-reload)

## Queue and history

- [listgroups](#api-method-listgroups)
- [listfiles](#api-method-listfiles)
- [history](#api-method-history)
- [append](#api-method-append)
- [editqueue](#api-method-editqueue)
- [scan](#api-method-scan)

## Status, logging and statistics

- [status](#api-method-status)
- [sysinfo](#api-method-sysinfo)
- [systemhealth](#api-method-systemhealth)
- [log](#api-method-log)
- [writelog](#api-method-writelog)
- [loadlog](#api-method-loadlog)
- [logscript](#api-method-logscript)
- [logupdate](#api-method-logupdate)
- [servervolumes](#api-method-servervolumes)
- [resetservervolume](#api-method-resetservervolume)

## Pause and speed limit

- [rate](#api-method-rate)
- [pausedownload](#api-method-pausedownload)
- [resumedownload](#api-method-resumedownload)
- [pausepost](#api-method-pausepost)
- [resumepost](#api-method-resumepost)
- [pausescan](#api-method-pausescan)
- [resumescan](#api-method-resumescan)
- [scheduleresume](#api-method-scheduleresume)

## Configuration

- [config](#api-method-config)
- [loadconfig](#api-method-loadconfig)
- [saveconfig](#api-method-saveconfig)
- [configtemplates](#api-method-configtemplates)

## Extensions

- [loadextensions](#api-method-loadextensions)
- [downloadextension](#api-method-downloadextension)
- [updateextension](#api-method-updateextension)
- [deleteextension](#api-method-deleteextension)

## Tests

- [testextension](#api-method-testextension)
- [testserver](#api-method-testserver)
- [testserverspeed](#api-method-testserverspeed)
- [testdiskspeed](#api-method-testdiskspeed)
- [testnetworkspeed](#api-method-testnetworkspeed)

## Method reference

### API-method `version`

#### Signature
``` c++
string version();
```

#### Description
Request the version-string of the program.

#### Return value
Version `string`

### API-method `shutdown`

#### Signature
``` c++
bool shutdown();
```

#### Description
Stop all activities and reinitialize the program. This method must be called after changing of program options for them to have effect.

#### Return value
Always `true`

### API-method `reload`

#### Signature
``` c++
bool reload();
```

#### Description
Shutdown the program.

#### Return value
Always `true`

### API-method `listgroups`

#### Signature
``` c++
struct[] listgroups(int NumberOfLogEntries);
```

#### Description
Request for list of downloads (nzb-files). This method returns summary information for each group (nzb-file).

#### Arguments
- **NumberOfLogEntries** `(int)` - ~~`v15.0`~~ Number of post-processing log-entries (field `Log`), which should be returned for the top (currently processing) item in post-processing state. Deprecated, must be 0.

#### Return value
This method returns array of structures with following fields:

- **NZBID** `(int)` - ID of NZB-file.
- **FirstID** `(int)` - ~~`v13.0`~~ Deprecated, use `NZBID` instead.
- **LastID** `(int)` - ~~`v13.0`~~ Deprecated, use `NZBID` instead.
- **NZBFilename** `(string)` - Name of nzb-file, this file was added to queue from. The filename could include fullpath (if client sent it by adding the file to queue).
- **NZBName** `(string)` - The name of nzb-file without path and extension. Ready for user-friendly output.
- **NZBNicename** `(string)` - ~~`v15.0`~~ Deprecated, use `NZBName` instead.
- **Kind** `(string)` - Kind of queue entry: NZB or URL.
- **URL** `(string)` - URL where the NZB-file was fetched `(Kind=NZB)` or should be fetched `(Kind=URL)`.
- **DestDir** `(string)` - Destination directory for output file.
- **FinalDir** `(string)` - Final destination if set by one of post-processing scripts. Can be set only for items in post-processing state.
- **Category** `(string)` - Category for group or empty string if none category is assigned.
- **FileSizeLo** `(int)` - Initial size of all files in group in bytes, Low 32-bits of 64-bit value.
- **FileSizeHi** `(int)` - Initial size of all files in group in bytes, High 32-bits of 64-bit value.
- **FileSizeMB** `(int)` - Initial size of all files in group in MiB.
- **RemainingSizeLo** `(int)` - Remaining size of all (remaining) files in group in bytes, Low 32-bits of 64-bit value.
- **RemainingSizeHi** `(int)` - Remaining size of all (remaining) files in group in bytes, High 32-bits of 64-bit value.
- **RemainingSizeMB** `(int)` - Remaining size of all (remaining) files in group in MiB.
- **PausedSizeLo** `(int)` - Size of all paused files in group in bytes, Low 32-bits of 64-bit value.
- **PausedSizeHi** `(int)` - Size of all paused files in group in bytes, High 32-bits of 64-bit value.
- **PausedSizeMB** `(int)` - Size of all paused files in group in MiB.
- **FileCount** `(int)` - Initial number of files in group.
- **RemainingFileCount** `(int)` - Remaining (current) number of files in group.
- **RemainingParCount** `(int)` - Remaining (current) number of par-files in group.
- **MinPostTime** `(int)` - Date/time when the oldest file in the group was posted to newsgroup (Time is in C/Unix format).
- **MaxPostTime** `(int)` - Date/time when the newest file in the group was posted to newsgroup (Time is in C/Unix format).
- **MinPriority** `(int)` - ~~`v13.0`~~ Deprecated, use `MaxPriority` instead.
- **MaxPriority** `(int)` - Priority of the group. “Max” in the field name has historical reasons.
- **ActiveDownloads** `(int)` - Number of active downloads in the group. With this filed can be determined what group(s) is (are) being currently downloaded. In most cases only one group is downloaded at a time however more that one group can be downloaded simultaneously when the first group is almost completely downloaded.
- **Status** `(string)`- Status of the group:
  - **QUEUED** - queued for download;
  - **PAUSED** - paused;
  - **DOWNLOADING** - item is being downloaded;
  - **FETCHING** - nzb-file is being fetched from URL `(Kind=URL)`;
  - **PP_QUEUED** - queued for post-processing (completely downloaded);
  - **LOADING_PARS** - stage of par-check;
  - **VERIFYING_SOURCES** - stage of par-check;
  - **REPAIRING** - stage of par-check;
  - **VERIFYING_REPAIRED** - stage of par-check;
  - **RENAMING** - processed by par-renamer;
  - **UNPACKING** - being unpacked;
  - **MOVING** - moving files from intermediate directory into destination directory;
  - **POST_UNPACK_RENAMING** - renaming excessively obfuscated downloaded files after unpacking;
  - **POST_DOWNLOAD_RENAMING** - renaming excessively obfuscated downloaded non-archive files;
  - **EXECUTING_SCRIPT** - executing post-processing script;
  - **PP_FINISHED** - post-processing is finished, the item is about to be moved to history.
- **TotalArticles** `(int)` - Total number of articles in all files of the group.
- **SuccessArticles** `(int)` - Number of successfully downloaded articles.
- **FailedArticles** `(int)` - Number of failed article downloads.
- **Health** `(int)` - Current health of the group, in permille. 1000 means 100.0%. The health can go down below this valued during download if more article fails. It can never increase (unless merging of groups). Higher values are better.
- **CriticalHealth** `(int)` - Calculated critical health of the group, in permille. 1000 means 100.0%. The critical health is calculated based on the number and size of par-files. Lower values are better.
- **DownloadedSizeLo** `(int)` - `v14.0` Amount of downloaded data for group in bytes, Low 32-bits of 64-bit value.
- **DownloadedSizeHi** `(int)` - `v14.0` Amount of downloaded data for group in bytes, High 32-bits of 64-bit value.
- **DownloadedSizeMB** `(int)` - `v14.0` Amount of downloaded data for group in MiB.
- **DownloadTimeSec** `(int)` - `v14.0` Download time in seconds.
- **MessageCount** `(int)` - `v15.0` Number of messages stored in the item log. Messages can be retrieved with method [loadlog](#api-method-loadlog).
- **DupeKey** `(string)` - Duplicate key. [RSS](../usage/RSS.md).
- **DupeScore** `(int)` - Duplicate score. [RSS](../usage/RSS.md).
- **DupeMode** `(string)` - Duplicate mode. One of SCORE, ALL, FORCE. [RSS](../usage/RSS.md).
- **Parameters** `(struct[])` - Post-processing parameters for group. An array of structures with following fields:
  - **Name** `(string)`- Name of post-processing parameter.
  - **Value** `(string)` - Value of post-processing parameter.
- **Deleted** `(bool)` - ~~`v12.0`~~ Deprecated, use DeleteStatus instead.
- **ServerStats** `(struct[])` - Per news-server download statistics. For description see method [history](#api-method-history).
- **ParStatus**, **UnpackStatus**, **ExParStatus**, **MoveStatus**, **ScriptStatus**, **DeleteStatus**, **UrlStatus**, **MarkStatus**, **ScriptStatuses**, **PostTotalTimeSec**, **ParTimeSec**, **RepairTimeSec**, **UnpackTimeSec** - These fields have meaning only for a group which is being currently post-processed. For description see method [history](#api-method-history).
- **PostInfoText** `(string)` - Text with short description of current action in post processor. For example: `Verifying file myfile.rar`. Only for a group which is being currently post-processed.
- **PostStageProgress** `(int)` - Completing of current stage, in permille. 1000 means 100.0%. Only for a group which is being currently post-processed.
- **PostTotalTimeSec** `(int)` - Number of seconds this post-job is being processed (after it first changed the state from PP-QUEUED). Only for a group which is being currently post-processed.
- **PostStageTimeSec** `(int)` - Number of seconds the current stage is being processed. Only for a group which is being currently post-processed.
- **Log** `(struct[])` - ~~`v15.0`~~ Array of structs with log-messages. For description of struct see method [log](#api-method-log). Only for a group which is being currently post-processed. The number of returned entries is limited by parameter `NumberOfLogEntries`. Deprecated, use method [loadlog](#api-method-loadlog) instead.

### API-method `listfiles`

#### Signature
``` c++
struct[] listfiles(int IDFrom, int IDTo, int NZBID);
```

#### Description
Request for file's list of a group (nzb-file).

#### Arguments
- **IDFrom** `(int)` - ~~`v12.0`~~ Deprecated, must be 0.
- **IDTo** `(int)` - ~~`v12.0`~~ Deprecated, must be 0.
- **NZBID** `(int)` - NZBID of the group to be returned.

To get the list of all files from all groups set all parameters to 0.

#### Return value
This method returns an array of structures with following fields:

- **ID** `(int)` - ID of file.
- **NZBID** `(int)` - ID of NZB-file.
- **NZBFilename** `(string)` - Name of nzb-file. The filename could include fullpath (if client sent it by adding the file to queue).
- **NZBName** `(string)` - The name of nzb-file without path and extension. Ready for user-friendly output.
- **NZBNicename** `(string)` - ~~`v12.0`~~ Deprecated, use `NZBName` instead.
- **Subject** `(string)` - Subject of article (read from nzb-file).
- **Filename** `(string)` - Filename parsed from subject. It could be incorrect since the subject not always correct formated. After the first article for file is read, the correct filename is read from article body.
- **FilenameConfirmed** `(bool)` - `true` if filename was already read from article’s body. `false` if the name was parsed from subject. For confirmed filenames the destination file on disk will be exactly as specified in field `filename`. For unconfirmed filenames the name could change later.
- **DestDir** `(string)` - Destination directory for output file.
- **FileSizeLo** `(int)` - Filesize in bytes, Low 32-bits of 64-bit value.
- **FileSizeHi** `(int)` - Filesize in bytes, High 32-bits of 64-bit value.
- **RemainingSizeLo** `(int)` - Remaining size in bytes, Low 32-bits of 64-bit value.
- **RemainingSizeHi** `(int)` - Remaining size in bytes, High 32-bits of 64-bit value.
- **Paused** `(bool)` - `true` if file is paused.
- **PostTime** `(int)` - Date/time when the file was posted to newsgroup (Time is in C/Unix format).
- **Category** `(string)` - Category for group or empty string if none category is assigned.
- **Priority** `(int)` - ~~`v13.0`~~ Deprecated, use MaxPriority of the group (method [listgroups](#api-method-listgroups)) instead.
- **ActiveDownloads** `(int)` - Number of active downloads for the file. With this filed can be determined what file(s) is (are) being currently downloaded.
- **Progress** `(int) `- `v15.0` Download progress, a number in the range 0..1000. Divide it to 10 to get percent-value.

### API-method `history`

#### Signature
``` c++
struct[] history(bool Hidden);
```

#### Description
Request for list of items in history-list.

#### Arguments
- **Hidden** `(bool)` - Also return hidden history records. Use this only if you need to see the old (hidden) history records (Kind=DUP). Normal (unhidden) records are always returned.

#### Return value
This method returns an array of structures with following fields:

- **NZBID** `(int)` - ID of NZB-file.
- **ID (int)** - ~~`v13.0`~~ Deprecated, use `NZBID` instead.
- **Kind** `(string)` - Kind of history item. One of the predefined text constants:
  - **NZB** for nzb-files;
  - **URL** for failed URL downloads. **NOTE**: successful URL-downloads result in adding files to download queue, a success-history-item is not created in this case;
  - **DUP** for hidden history items.
- **NZBFilename** `(string)` - Name of nzb-file, this file was added to queue from. The filename could include fullpath (if client sent it by adding the file to queue).
- **Name** `(string)` - The name of nzb-file or info-name of URL, without path and extension. Ready for user-friendly output.
- **NZBName** `(string)` - nzb-file name.
- **NZBNicename** `(string)` - ~~`v12.0`~~ Deprecated, use `NZBName` instead.
- **URL** `(string)` - URL.
- **RetryData** `(bool)` - Has completed files.
- **HistoryTime** `(int)` - Date/time when the file was added to history (Time is in C/Unix format).
- **DestDir** `(string)` - Destination directory for output files.
- **FinalDir** `(string)` - Final destination if set by one of post-processing scripts.
- **Category** `(string)` - Category for group or empty string if none category is assigned.
- **FileSizeLo** `(int)` - Initial size of all files in group in bytes, Low 32-bits of 64-bit value.
- **FileSizeHi** `(int)` - Initial size of all files in group in bytes, High 32-bits of 64-bit value.
- **FileSizeMB** `(int)` - Initial size of all files in group in MiB.
- **FileCount** `(int)` - Initial number of files in group.
- **RemainingFileCount** `(int)` - Number of parked files in group. If this number is greater than `0`, the history item can be returned to download queue using command `HistoryReturn` of method [editqueue](#api-method-editqueue).
- **MinPostTime** `(int)` - Date/time when the oldest file in the item was posted to newsgroup (Time is in C/Unix format).
- **MaxPostTime** `(int)` - Date/time when the newest file in the item was posted to newsgroup (Time is in C/Unix format).
- **TotalArticles** `(int)` - Total number of articles in all files of the group.
- **SuccessArticles** `(int)` - Number of successfully downloaded articles.
- **FailedArticles** `(int)` - Number of failed article downloads.
- **Health `(int)` - Final health of the group, in permille. 1000 means 100.0%. Higher values are better.
- **CriticalHealth** `(int)` - Calculated critical health of the group, in permille. 1000 means 100.0%. The critical health is calculated based on the number and size of par-files. Lower values are better.
- **Deleted** `(bool)` - ~~`v12.0`~~ Deprecated, use DeleteStatus instead.
- **DownloadedSizeLo** `(int)` - `v14.0` Amount of downloaded data for group in bytes, Low 32-bits of 64-bit value.
- **DownloadedSizeHi `(int)` - v14.0 Amount of downloaded data for group in bytes, High 32-bits of 64-bit value.
- **DownloadedSizeMB** `(int)` - `v14.0` Amount of downloaded data for group in MiB.
- **DownloadTimeSec** `(int)` - `v14.0` Download time in seconds.
- **PostTotalTimeSec** `(int)` - `v14.0` Total post-processing time in seconds.
- **ParTimeSec** `(int)` - `v14.0` Par-check time in seconds (incl. verification and repair).
- **RepairTimeSec** `(int)` - `v14.0` Par-repair time in seconds.
- **UnpackTimeSec** `(int)` - `v14.0` Unpack time in seconds.
- **MessageCount** `(int)` - `v15.0` Number of messages stored in the item log. Messages can be retrieved with method loadlog.
- **DupeKey** `(string)` - Duplicate key. See [RSS](../usage/RSS.md).
- **DupeScore** `(int)` - Duplicate score. See [RSS](../usage/RSS.md).
- **DupeMode** `(string)` - Duplicate mode. One of **SCORE**, **ALL**, **FORCE**. See [RSS](../usage/RSS.md).
- **Status** `(string)` - Total status of the download. One of the predefined text constants such as `SUCCESS/ALL` or `FAILURE/UNPACK`, etc. For the complete list see below.
- **ParStatus** `(string)` - Result of par-check/repair:
  - **NONE** - par-check wasn’t performed;
  - **FAILURE** - par-check has failed;
  - **REPAIR_POSSIBLE** - download is damaged, additional par-files were downloaded but the download was not repaired. Either the option ParRepair is disabled or the par-repair was cancelled by option ParTimeLimit;
  - **SUCCESS** - par-check was successful;
  - **MANUAL** - download is damaged but was not checked/repaired because option `ParCheck` is set to `Manual`.
- **ExParStatus** `(string)` - `v16.0` Indicates if the download was repaired using duplicate par-scan mode (option `ParScan=dupe`):
  - **RECIPIENT** - repaired using blocks from other duplicates;
  - **DONOR** - has donated blocks to repair another duplicate;
- **UnpackStatus** `(string)` - Result of unpack:
  - **NONE** - unpack wasn’t performed, either no archive files were found or the unpack is disabled for that download or globally;
  - **FAILURE** - unpack has failed;
  - **SPACE** - unpack has failed due to not enough disk space;
  - **PASSWORD** - unpack has failed because the password was not provided or was wrong. Only for rar5-archives;
  - **SUCCESS** - unpack was successful.
- **UrlStatus** `(string)` - Result of URL-download:
  - **NONE** - that nzb-file were not fetched from an URL;
  - **SUCCESS** - that nzb-file was fetched from an URL;
  - **FAILURE** - the fetching of the URL has failed.
  - **SCAN_SKIPPED** - The URL was fetched successfully but downloaded file was not nzb-file and was skipped by the scanner;
  - **SCAN_FAILURE** - The URL was fetched successfully but an error occurred during scanning of the downloaded file. The downloaded file isn’t a proper nzb-file. This status usually means the web-server has returned an error page (HTML page) instead of the nzb-file.
- **ScriptStatus `(string)` - Accumulated result of all post-processing scripts. One of the predefined text constants: `NONE`, `FAILURE`, `SUCCESS`. Also see field `ScriptStatuses`.
- **ScriptStatuses** `(struct[])` - Status info of each post-processing script. See below.
- **MoveStatus** `(string)` - Result of moving files from intermediate directory into final directory:
  - **NONE** - the moving wasn’t made because either the option `InterDir` is not in use or the par-check or unpack have failed;
  - **SUCCESS** - files were moved successfully;
  - **FAILURE** - the moving has failed.
- **DeleteStatus** `(string)` - Indicates if the download was deleted:
  - **NONE** - not deleted;
  - **MANUAL** - the download was manually deleted by user;
  - **HEALTH** - the download was deleted by health check;
  - **DUPE** - the download was deleted by duplicate check;
  - **BAD** - `v14.0` the download was marked as `BAD` by a queue-script during download;
  - **SCAN** - `v16.0` the download was deleted because the nzb-file could not be parsed (malformed nzb-file);
  - **COPY** - `v16.0` the download was deleted by duplicate check because an nzb-file with exactly same content exists in download queue or in history.
- **MarkStatus** `(string)` - Indicates if the download was marked by user:
  - **NONE** - not marked;
  - **GOOD** - the download was marked as good by user using command `Mark as good` in history dialog;
  - **BAD** - the download was marked as bad by user using command `Mark as bad` in history dialog;
- **ExtraParBlocks** `(int)` - `v16.0` amount of extra par-blocks received from other duplicates or donated to other duplicates, when duplicate par-scan mode was used (option `ParScan`=`dupe`):
  - **> 0** - has received extra blocks;
  - **< 0** - has donated extra blocks;
- **Parameters** `(struct[])` - Post-processing parameters for group. For description of struct see method [listgroups](#api-method-listgroups).
- **ServerStats** `(struct[])` - Per-server article completion statistics.
- **Log** `(struct[])` - ~~`v13.0`~~ Deprecated, was never really used.

### Field Status
Field `Status` can be set to one of the following values:

**For history items with `Kind=NZB`**

- **SUCCESS/ALL** - Downloaded and par-checked or unpacked successfully. All post-processing scripts were successful. The download is completely OK.
- **SUCCESS/UNPACK** - Similar to `SUCCESS/ALL` but no post-processing scripts were executed. Downloaded and unpacked successfully. Par-check was successful or was not necessary.
- **SUCCESS/PAR** - Similar to `SUCCESS/ALL` but no post-processing scripts were executed. Downloaded and par-checked successfully. No unpack was made (there are no archive files or unpack was disabled for that download or globally). At least one of the post-processing scripts has failed.
- **SUCCESS/HEALTH** - Download was successful, download health is 100.0%. No par-check was made (there are no par-files). No unpack was made (there are no archive files or unpack was disabled for that download or globally).
- **SUCCESS/GOOD** - The download was marked as good by user using command Mark as good in history dialog.
- **SUCCESS/MARK** - `v15.0` The download was marked as success by user using command `Mark as success` in history dialog.
- **WARNING/SCRIPT** - Downloaded successfully. Par-check and unpack were either successful or were not performed. At least one post-processing script has failed.
- **WARNING/SPACE** - Unpack has failed due to not enough space on the drive.
- **WARNING/PASSWORD** - Unpack has failed because the password was not provided or was wrong.
- **WARNING/DAMAGED** - Par-check is required but is disabled in settings (option `ParCheck=Manual`).
- **WARNING/REPAIRABLE** - Par-check has detected a damage and has downloaded additional par-files but the repair is disabled in settings (option `ParRepair=no`).
- **WARNING/HEALTH** - Download health is below 100.0%. No par-check was made (there are no par-files). No unpack was made (there are no archive files or unpack was disabled for that download or globally).
- **DELETED/MANUAL** - The download was manually deleted by user.
- **DELETED/DUPE** - The download was deleted by duplicate check.
- **DELETED/COPY** - `v16.0` The download was deleted by duplicate check because this nzb-file already exists in download queue or in history.
- **DELETED/GOOD** - `v16.0` The download was deleted by duplicate check because there is a duplicate history item with status `GOOD` or a duplicate hidden history item with status`SUCCESS` which do not have any visible duplicates.
- **FAILURE/PAR** - The par-check has failed.
- **FAILURE/UNPACK** - The unpack has failed and there are no par-files.
- **FAILURE/MOVE** - An error has occurred when moving files from intermediate directory into the final destination directory.
- **FAILURE/SCAN** - `v16.0` nzb-file could not be parsed (malformed nzb-file).
- **FAILURE/BAD** - The download was marked as bad by user using command `Mark as bad` in history dialog.
- **FAILURE/HEALTH** - Download health is below critical health. No par-check was made (there are no par-files or the download was aborted by health check). No unpack was made (there are no archive files or unpack was disabled for that download or globally or the download was aborted by health check).

**For history items with `Kind=URL`**

- **DELETED/MANUAL** - The download was manually deleted by user.
- **DELETED/DUPE** - The download was deleted by duplicate check.
- **WARNING/SKIPPED** - The URL was fetched successfully but downloaded file was not nzb-file and was skipped by the scanner.
- **FAILURE/FETCH** - Fetching of the URL has failed.
- **FAILURE/SCAN** - The URL was fetched successfully but an error occurred during scanning of the downloaded file. The downloaded file isn’t a proper nzb-file. This status usually means the web-server has returned an error page (HTML page) instead of the nzb-file.

**For history items with `Kind=DUP`**

- **SUCCESS/HIDDEN** - The hidden history item has status `SUCCESS`.
- **SUCCESS/GOOD** - The download was marked as good by user using command Mark as good in history dialog.
- **FAILURE/HIDDEN** - The hidden history item has status `FAILURE`.
- **DELETED/MANUAL** - The download was manually deleted by user.
- **DELETED/DUPE** - The download was deleted by duplicate check.
- **FAILURE/BAD** - The download was marked as bad by user using command `Mark as bad` in history dialog.

#### Field ScriptStatuses
Contains an array of structs with following fields:

- **Name** `(string)` - Script name.
- **Status** `(string)` - Result of post-processing script exection. One of the predefined text constants: `NONE`, `FAILURE`, `SUCCESS`.

#### Field ServerStats
Contains an array of structs with following fields:

- **ServerID** `(int)` - Server number as defined in section `news servers` of the configuration file.
- **SuccessArticles** `(int)` - Number of successfully downloaded articles.
- **FailedArticles** `(int)` - Number of failed articles.

### API-method `append`

#### Signature
``` c++
int append(
  string Filename, 
  string Content, 
  string Category,
  int Priority, 
  bool AddToTop, 
  bool AddPaused, 
  string DupeKey,
  int DupeScore, 
  string DupeMode, 
  bool AutoCategory,
  struct[] PPParameters
);
```

_Add an NZB, archive file, or URL to the download queue_

#### Arguments
- **Filename** `(string)` - name of the file with extension which determines how it's handled (e.g., my-file.nzb, project.zip, data.rar). This parameter can be an empty string if **Content** contains a URL; in that case, the filename is read from HTTP headers. If `Filename` is provided it must have correct extension (usually `.nzb`, `.zip`, `.rar`) according to file content. Files without a recognized extension are not added to the queue directly but are passed to scan-scripts for further processing.
- **Category** `(string)` - category for nzb-file. Can be empty string.
- **Priority** `(int)` - priority for nzb-file. 0 for `normal priority`, positive values for high priority and negative values for low priority. Downloads with priorities equal to or greater than 900 are downloaded and post-processed even if the program is in paused state (force mode). Default priorities are: -100 (very low), -50 (low), 0 (normal), 50 (high), 100 (very high), 900 (force).
- **AddToTop** `(bool)` - `true` if the file should be added to the top of the download queue or `false` if to the end.
- **AddPaused** `(bool)` - `true` if the file should be added in paused state.
DupeKey (string) - duplicate key for nzb-file. See [RSS](../usage/RSS.md).
- **DupeScore** `(int)` - duplicate score for nzb-file. See [RSS](../usage/RSS.md).
- **DupeMode** `(string)` - duplicate mode for nzb-file. See [RSS](../usage/RSS.md).
- **AutoCategory** `(bool)` - If true, the category will be automatically detected from the NZB file (if available).
- **PPParameters** `(array)` - `v16.0` post-processing parameters. The array consists of structures with following fields:
  - **Name** `(string)` - name of post-processing parameter.
  - **Value** `(string)` - value of post-processing parameter.

#### Return value
Positive number representing `NZBID` of the queue item. `0` and negative numbers represent error codes. Current version uses only error code `0`, newer versions may use other error codes for detailed information about error.

#### Example

```python
from xmlrpc.client import ServerProxy
from base64 import standard_b64encode

server = ServerProxy("http://nzbget:tegbzn6789@localhost:6789/xmlrpc")
filename = "/tmp/test.nzb"
with open(filename, "rb") as f:
    nzb_content = f.read()
base64_nzb_content = standard_b64encode(nzb_content).decode()
server.append(
    filename,
    base64_nzb_content,
    "software",
    0,
    False,
    False,
    "",
    0,
    "SCORE",
    [
      ("*unpack:", "yes"), 
      ("EMail.py:", "yes")
    ],
)
```

### API-method `editqueue`

#### Signature

#### Since 
`v18.0`
``` c++
bool editqueue(string Command, string Param, int[] IDs);
```

#### Till 
`v18.0`
```c++
bool editqueue(string Command, int Offset, string Param, int[] IDs);
```

#### Description
Edit items in download queue or in history.

#### Arguments
- **Command** `(string)` - one of the following commands:
  - **FileMoveOffset** - Move files relative to the current position in queue. `v18.0` Param contains offset. ~~`v18.0`~~ Offset is passed in `Offset`.
  - **FileMoveTop** - Move files to top of queue.
  - **FileMoveBottom** - Move files to bottom of queue.
  - **FilePause** - Pause files.
  - **FileResume** - Resume (unpause) files.
  - **FileDelete** - Delete files.
  - **FilePauseAllPars** - Pause only pars (does not affect other files).
  - **FilePauseExtraPars** - Pause only pars, except main par-file (does not affect other files).
  - **FileSetPriority** - ~~`v13.0`~~ Deprecated, use GroupSetPriority instead.
  - **FileReorder** - Reorder files in the group. The list of IDs may include files only from one group.
  - **FileSplit** - Split nzb-file. The list of IDs contains the files to move into new download item.
  - **GroupMoveOffset** - Move groups relative to the current position in queue. `v18.0` Param contains offset. ~~`v18.0`~~ Offset is passed in Offset.
  - **GroupMoveTop** - Move groups to top of queue.
  - **GroupMoveBottom** - Move groups to bottom of queue.
  - **GroupPause** - Pause groups.
  - **GroupResume** - Resume (unpause) groups.
  - **GroupDelete** - Delete groups and put to history.
  - **GroupDupeDelete** - Delete groups, put to history and mark as duplicate.
  - **GroupFinalDelete** - Delete groups without adding to history.
  - **GroupPauseAllPars** - Pause only pars (does not affect other files).
  - **GroupPauseExtraPars** - Pause only pars, except main par-file (does not affect other files).
  - **GroupSetPriority** - Set priority for all files in group. Param contains priority value.
  - **GroupSetCategory** - Set category for group. Param contains category name.
  - **GroupApplyCategory** - Set or change category for groups and reassign pp-params according to category settings. `Param` contains category name.
  - **GroupMerge** - Merge groups.
  - **GroupSetParameter** - Set post-processing parameter for group. `Param` contains string in form of `Paramname=Paramvalue`.
  - **GroupSetName** - Rename group. Param contains new name.
  - **GroupSetDupeKey** - Set duplicate key. Param contains duplicate key. See [RSS](../usage/RSS.md).
  - **GroupSetDupeScore** - Set duplicate score. Param contains duplicate score. See [RSS](../usage/RSS.md).
  - **GroupSetDupeMode** - Set duplicate mode. Param contains one of `SCORE`, `ALL`, `FORCE`. See [RSS](../usage/RSS.md).
  - **GroupSort** - `v15.0` Sort selected or all groups. Parameter `Param` must be one of: `name`, `priority`, `category`, `size`, `left`; add character `+` or `-` to sort to explicitly define ascending or descending order (for example `name-`); if none of these characters is used the auto-mode is active: the items are sorted in ascending order first, if nothing changed - they are sorted again in descending order. `Parameter IDs` contains the list of groups to sort; pass empty array to sort all groups.
  - **PostMoveOffset** - ~~`v13.0`~~ Deprecated, use `GroupMoveOffset` instead.
  - **PostMoveTop** - ~~`v13.0`~~ Deprecated, use `GroupMoveTop` instead.
  - **PostMoveBottom** - ~~`v13.0`~~ Deprecated, use `GroupMoveBottom instead.
  - **PostDelete** - Delete post-jobs.
  - **HistoryDelete** - Hide history items (mark as hidden).
  - **HistoryFinalDelete** - Delete history items.
  - **HistoryReturn** - Return history items back to download queue.
  - **HistoryProcess** - Post-process history items again.
  - **HistoryRedownload** - Move history items back to download queue for redownload.
  - **HistorySetName** - `v15.0` Rename history item. `Param` contains new name.
  - **HistorySetCategory** - `v15.0` Set category for history item. Param contains category name.
  - **HistorySetParameter** - Set post-processing parameter for history items. `Param` contains string in form of `Paramname=Paramvalue`.
  - **HistorySetDupeKey** - Set duplicate key. `Param` contains duplicate key. See [RSS](../usage/RSS.md).
  - **HistorySetDupeScore** - Set duplicate score. `Param` contains duplicate score. See [RSS](../usage/RSS.md).
  - **HistorySetDupeMode** - Set duplicate mode. `Param` contains one of `SCORE`, `ALL`, `FORCE`. See [RSS](../usage/RSS.md).
  - **HistorySetDupeBackup** - Set `use as duplicate backup-flag` for history items. `Param` contains `0` or `1`. See [RSS](../usage/RSS.md).
  - **HistoryMarkBad** - Mark history item as bad (and download other duplicate). See [RSS](../usage/RSS.md).
  - **HistoryMarkGood** - Mark history item as good. See [RSS](../usage/RSS.md).
  - **HistoryMarkSuccess** - `v15.0` Mark history item as success. See [RSS](../usage/RSS.md).
- **Offset (int)** - ~~`v18.0`~~ offset for commands `FileMoveOffset` and `GroupMoveOffset`. For all other commands must be `0`. `v18.0` Offset is passed in `Param` and parameter `Offset` should not be passed at all.
- **Param** `(string)` - additional parameter if mentioned in the command description, otherwise an empty string.
- **IDs** `(struct[])` - array of IDs (as integers).
  - File-commands (FileXXX) need ID of file.
  - All other commands need `NZBID`.

#### Return value
`true` on success or `false` on failure.

### API-method `scan`

#### Signature
``` c++
bool scan(bool SyncMode);
```

#### Description
Request rescanning of incoming directory for nzb-files (option `NzbDir`).

#### Arguments
- **SyncMode** `(bool)` - `Optional`. waits for completing of scan
  before reporting the status.

#### Return value
Always `true`

### API-method `status`

#### Signature
``` c++
struct status();
```

#### Description
Request for current status (summary) information.

#### Return value
This method returns structure with following fields:

- **RemainingSizeLo** `(int)` - Remaining size of all entries in download queue, in bytes. This field contains the low 32-bits of 64-bit value
- **RemainingSizeHi** `(int)` - Remaining size of all entries in download queue, in bytes. This field contains the high 32-bits of 64-bit value
- **RemainingSizeMB** `(int)` - Remaining size of all entries in download queue, in MiB.
- **ForcedSizeLo** `(int)` - Remaining size of entries with FORCE priority, in bytes. This field contains the low 32-bits of 64-bit value
- **ForcedSizeHi** `(int)` - Remaining size of entries with FORCE priority, in bytes. This field contains the high 32-bits of 64-bit value
- **ForcedSizeMB** `(int)` - Remaining size of entries with FORCE priority, in MiB.
- **DownloadedSizeLo** `(int)` - Amount of data downloaded since server start, in bytes. This field contains the low 32-bits of 64-bit value
- **DownloadedSizeHi** `(int)` - Amount of data downloaded since server start, in bytes. This field contains the high 32-bits of 64-bit value
- **DownloadedSizeMB** `(int)` - Amount of data downloaded since server start, in MiB.
- **MonthSizeLo** `(int)` - Amount of data downloaded this month, in bytes. This field contains the low 32-bits of 64-bit value
- **MonthSizeHi** `(int)` - Amount of data downloaded this month, in bytes. This field contains the high 32-bits of 64-bit value
- **MonthSizeMB** `(int)` - Amount of data downloaded this month, in MiB.
- **DaySizeLo** `(int)` - Amount of data downloaded today, in bytes. This field contains the low 32-bits of 64-bit value.
- **DaySizeHi** `(int)` - Amount of data downloaded today, in bytes. This field contains the high 32-bits of 64-bit value.
- **DaySizeMB** `(int)` - Amount of data downloaded today, in MiB.
- **QuotaReached** `(bool)` - Indicates whether the quota has been reached.
- **ArticleCacheLo** `(int)` - `v14.0` Current usage of article cache, in bytes. This field contains the low 32-bits of 64-bit value.
- **ArticleCacheHi** `(int)` - `v14.0` Current usage of article cache, in bytes. This field contains the high 32-bits of 64-bit value.
- **ArticleCacheMB** `(int)` - `v14.0` Current usage of article cache, in MiB.
- **DownloadRate** `(int)` - ~~`24.2`~~ Deprecated. Current download speed, in Bytes per Second.
- **DownloadRateLo** `(int)` - `v24.2` Current download speed, in Bytes per Second. This field contains the low 32-bits of 64-bit value.
- **DownloadRateHi** `(int)` - `v24.2` Current download speed, in Bytes per Second. This field contains the high 32-bits of 64-bit value.
- **AverageDownloadRate** `(int)` - ~~`v24.2`~~ Deprecated. Average download speed since server start, in Bytes per Second.
- **AverageDownloadRateLo** `(int)` - `v24.2` Average download speed since server start, in Bytes per Second. This field contains the low 32-bits of 64-bit value.
- **AverageDownloadRateHi** `(int)` - `v24.2` Average download speed since server start, in Bytes per Second. This field contains the high 32-bits of 64-bit value.
- **DownloadLimit** `(int)` - Current download limit, in Bytes per Second. The limit can be changed via method “rate”. Be aware of different scales used by the method rate (Kilobytes) and this field (Bytes).
- **ThreadCount** `(int)` - Number of threads running. It includes all threads, created by the program, not only download-threads.
- **PostJobCount** `(int)` - Number of Par-Jobs or Post-processing script jobs in the post-processing queue (including current file).
- **ParJobCount** `(int)` - ~~`v12.0`~~ Deprecated, use PostJobCount instead.
- **UrlCount** `(int)` - Number of URLs in the URL-queue (including current file).
- **UpTimeSec** `(int)` - Server uptime in seconds.
- **DownloadTimeSec** `(int)` - Server download time in seconds.
- **ServerStandBy** `(bool)` - `false` - there are currently downloads running, `true` - no downloads in progress (server paused or all jobs completed).
- **DownloadPaused** `(bool)` - `true` if download queue is paused via first pause register (soft-pause).
- **Download2Paused** `(bool)` - ~~`v13.0`~~ Deprecated, use DownloadPaused instead.
- **ServerPaused** `(bool)` - ~~`v12.0`~~ Deprecated, use DownloadPaused instead.
- **PostPaused** `(bool)` - `true` if post-processor queue is currently in paused-state.
- **ScanPaused** `(bool)` - `true` if the scanning of incoming nzb-directory is currently in paused-state.
- **ServerTime** `(int)` - Current time on computer running NZBGet. Time is in C/Unix format (number of seconds since 00:00:00 UTC, January 1, 1970).
- **ResumeTime** `(int)` - Time to resume if set with method `scheduleresume`. Time is in C/Unix format.
- **FeedActive** `(bool)` - `true` if any RSS feed is being fetched right now.
- **FreeDiskSpaceLo** `(int)` - Free disk space on `DestDir`, in bytes. This field contains the low 32-bits of 64-bit value
- **FreeDiskSpaceHi** `(int)` - Free disk space on `DestDir`, in bytes. This field contains the high 32-bits of 64-bit value
- **FreeDiskSpaceMB** `(int)` - Free disk space on `DestDir`, in MiB.
- **TotalDiskSpaceLo** `(int)` - `v24.2` Total disk space on `DestDir`, in bytes. This field contains the low 32-bits of 64-bit value
- **TotalDiskSpaceHi** `(int)` - `v24.2` Total disk space on `DestDir`, in bytes. This field contains the high 32-bits of 64-bit value
- **TotalDiskSpaceMB** `(int)` - `v24.2` Total disk space on `DestDir`, in MiB.
- **FreeInterDiskSpaceLo** `(int)` - `v24.3` Free disk space on `InterDir`, in bytes. This field contains the low 32-bits of 64-bit value
- **FreeInterDiskSpaceHi** `(int)` - `v24.3` Free disk space on `InterDir`, in bytes. This field contains the high 32-bits of 64-bit value
- **FreeInterDiskSpaceMB** `(int)` - `v24.3` Free disk space on `InterDir`, in MiB.
- **TotalInterDiskSpaceLo** `(int)` - `v24.3` Total disk space on `InterDir`, in bytes. This field contains the low 32-bits of 64-bit value
- **TotalInterDiskSpaceHi** `(int)` - `v24.3` Total disk space on `InterDir`, in bytes. This field contains the high 32-bits of 64-bit value
- **TotalInterDiskSpaceMB** `(int)` - `v24.3` Total disk space on `InterDir`, in MiB.
- **QueueScriptCount** `(int)` - Indicates number of queue-scripts queued for execution including the currently running.
- **NewsServers** `(struct[])` - Status of news-servers, array of structures with following fields
  - **ID** `(int)` - Server number in the configuration file. For example `1` for server defined by options `Server1.Host`, `Server1.Port`, etc.
  - **Active** `(bool)` - `true` if server is in active state (enabled). `Active` doesn’t mean that the data is being downloaded from the server right now. This only means the server can be used for download (if there are any download jobs).

### API-method `sysinfo`

### Since 
`v24.2`

#### Signature
``` c++
struct sysinfo();
```

#### Description
returns information about the user's environment and hardware.


#### Return value
This method returns an array of structures with following fields:

- **OS** `(struct)`
  - **Name** `(string)` - OS name.
  - **Version** `(string)` - OS version.
- **CPU** `(struct)`
  - **Model** `(string)` - CPU model.
  - **Arch** `(string)` - CPU arch.
- **Network** `(struct)`
  - **PublicIP** `(string)` - User's public IP.
  - **PrivateIP** `(string)` - User's private IP.
- **Tools** `(struct[])`
  - **Name** `(string)` - Tool name, e.g. `unrar`.
  - **Version** `(string)` - Tool version, e.g. `7.00`.
  - **Path** `(string)` - Tool path, e.g. `C:\ProgramData\NZBGet\unrar`.
- **Libraries** `(struct[])` - Libraries
  - **Name** `(string)` - Library name, e.g. `OpenSSL`.
  - **Version** `(string)` - Library version, e.g. `3.3.1`.

### API-method `systemhealth`

### Since
`v26.0`

#### Status
**Experimental (Not recommended for use)**

#### Signature
``` c++
struct systemhealth();
```

#### Description
Performs a diagnostic validation of the running configuration. The result is a structured report grouping findings by section, option and subsection.

#### Return value
This method returns a structure with the following fields:

- **Status** `(String)` - Overall health severity: **Ok**, **Info**, **Warning**, **Error**.

- **Alerts** `(struct[])` - Flat list of problematic configuration options across all sections.
    - **Name** `(String)` - Option name (e.g. `ControlPassword`, `MainDir`).
    - **Status** `(struct)` - The status object with `Severity` and `Message`.

- **Sections** `(struct[])` - Detailed per-section reports. Each `SectionReport` contains:
    - **Name** `(string)` - Section name (e.g. `Paths`, `Unpack`).
    - **Alerts** `(struct[])` - General alerts for the section.
    - **Options** `(struct[])` - Per-option checks:
        - **Name** `(string)` - Option name.
        - **Status** `(struct)` - `Severity` and `Message`.
    - **Subsections** `(struct[])` - Nested reports (e.g. individual news servers, categories).

#### Sections
The health check covers multiple sections (representative list):

- `Paths` — Main directories and writable/readable checks.
- `NewsServers` — Per-server configuration checks.
- `Security` — Authentication, control port/usability, TLS cert/key.
- `Categories` — Category definitions and paths.
- `Feeds` — RSS/Feed configuration checks.
- `IncomingNzb` — Incoming NZB directory options.
- `DownloadQueue` — Cache, write buffer, direct-write checks.
- `Connection` — Connection and proxy related options.
- `Logging` — Log file and rotation settings.
- `SchedulerTasks` — Scheduler-related checks.
- `CheckAndRepair` — PAR/repair and CRC checks.
- `Unpack` — Unpacker/extension handling.
- `ExtensionScripts` — Script availability and permissions.

#### Example
```json
{
    "Status": "Warning",
    "Alerts": [ 
        { 
            "Name": "MainDir", 
            "Status": 
            { 
                "Severity": "Error", 
                "Message": "MainDir cannot be empty." 
            } 
        } 
    ],
    "Sections": 
    [ 
        { 
            "Name": "Paths", 
            "Alerts": [], 
            "Options": 
            [
                { 
                    "Name": "MainDir", 
                    "Status": 
                    { 
                        "Severity": "Error", 
                        "Message": "MainDir cannot be empty." 
                    } 
                } 
            ], 
            "Subsections": [] 
        },
        {
            "Name": "NewsServers",
            "Alerts": [],
            "Options": [],
            "Subsections": [
                {
                    "Name": "Server1",
                    "Options": 
                    [
                        {
                            "Name": "Host",
                            "Status":
                            {
                                "Severity": "Error",
                                "Message": "Hostname cannot be empty."
                            }
                        }
                    ]
                }
            ]
        }
    ]
}
```

### API-method `log`

#### Signature
``` c++
struct[] log(int IDFrom, int NumberOfEntries);
```

#### Description
This method returns entries from screen’s log-buffer. The size of this buffer is limited and can be set via option LogBufferSize. Which messages should be saved to screen-log and which should be saved to log-file can be set via options `DetailTarget`, `InfoTarget`, `WarningTarget`, `ErrorTarget` and `DebugTarget`.

#### Arguments
- **IDFrom** `(int)` - The first ID to be returned.
- **NumberOfEntries** `(int)` - Number of entries, which should be returned.

**NOTE**: only one parameter - either `IDFrom` or `NumberOfEntries` - can be specified. The other parameter must be `0`.

**TIP**: If your application stores log-entries between sub-sequential calls to method log(), the usage of parameter IDFrom is recommended, since it reduces the amount of transferred data.

#### Return value
This method returns array of structures with following fields:

- **ID** `(int)` - ID of log-entry.
- **Kind** `(string)` - Class of log-entry, one of the following strings: `INFO`, `WARNING`, `ERROR`, `DEBUG`.
- **Time** `(int)` - Time in C/Unix format (number of seconds since 00:00:00 UTC, January 1, 1970).
- **Text** `(string)` - Log-message.

**NOTE**: if there are no entries for requested criteria, an empty array is returned.

### API-method `writelog`

#### Signature
``` c++
bool writelog(string Kind, string Text);
```

#### Description
Append log-entry into server’s log-file and on-screen log-buffer.

#### Arguments
- **Kind** `(string)` - Kind of log-message. Must be one of the following strings: `INFO`, `WARNING`, `ERROR`, `DETAIL`, `DEBUG`. Debug-messages are available, only if the program was compiled in debug-mode.
- **Text** `(string)` - Text to be added into log.

#### Return value
`true` on success or `failure` result on error.

### API-method `loadlog`

### Since
`v15.0`

#### Signature
``` c++
struct[] loadlog(int NZBID, int IDFrom, int NumberOfEntries);
```

#### Description
Loads from disk and returns nzb-log for a specific nzb-file.

#### Arguments
- **NZBID** `(int)` - id of nzb-file.

**NOTE** For other arguments and return values see method [log](#api-method-log)

### API-method `logscript`

#### Signature
``` c++
struct[] logscript(int idfrom, int entries);
```

#### Description
Loads from disk and returns the log of a specific extension script.

**NOTE** For the arguments and return values see method [log](#api-method-log)

### API-method `logupdate`

#### Signature
``` c++
struct[] logupdate(int idfrom, int entries);
```

#### Description
Loads from disk and returns an update log.

**NOTE** For the arguments and return values see method [log](#api-method-log)

### API-method `servervolumes`

#### Signature
``` c++
struct[] servervolumes();
```

#### Description
Returns download volume statistics per news-server.

#### Return value
This method returns an array of structures with following fields:

- **ServerID** `(int)` - ID of news server.
- **DataTime** `(int)` - Date/time when the data was last updated (time is in C/Unix format).
- **TotalSizeLo** `(int)` - Total amount of downloaded data since program installation, low 32-bits of 64-bit value.
- **TotalSizeHi** `(int)` - Total amount of downloaded data since program installation, high 32-bits of 64-bit value.
- **TotalSizeMB** `(int)` - Total amount of downloaded data since program installation, in MiB.
- **CustomSizeLo** `(int)` - Amount of downloaded data since last reset of custom counter, low 32-bits of 64-bit value.
- **CustomSizeHi** `(int)` - Amount of downloaded data since last reset of custom counter, high 32-bits of 64-bit value.
- **CustomSizeMB** `(int)` - Amount of downloaded data since last reset of custom counter, in MiB.
- **CustomTime** `(int)` - Date/time of the last reset of custom counter (time is in C/Unix format).
- **CountersResetTime** `(int)` - Date/time of the last reset of all counters (time is in C/Unix format).
- **BytesPerSeconds** `(struct[])` - Per-second amount of data downloaded in last 60 seconds. See below.
- **BytesPerMinutes** `(struct[])` - Per-minute amount of data downloaded in last 60 minutes. See below.
- **BytesPerHours** `(struct[])` - Per-hour amount of data downloaded in last 24 hours. See below.
- **BytesPerDays** `(struct[])` - Per-day amount of data downloaded since program installation. See below.
- **ArticlesPerDays** `(struct[])` - Per-day amount of failed and success articles since program installation. See below.
- **SecSlot** `(int)` - The current second slot of field `BytesPerSeconds` the program writes into.
- **MinSlot** `(int)` - The current minute slot of field `BytesPerMinutes` the program writes into.
- **HourSlot** `(int)` - The current hour slot of field `BytesPerHours` the program writes into.
- **DaySlot** `(int)` - The current day slot of field `BytesPerDays` the program writes into.
- **FirstDay** `(int)` - Indicates which calendar day the very first slot of `BytesPerDays`, `ArticlesPerDay` correspond to. Details see below.

**NOTE**: The first record (serverid=0) are totals for all servers.

#### Field BytesPerSeconds, BytesPerMinutes, BytesPerHours, BytesPerDays
Contains an array of structs with following fields:

- **SizeLo** `(int)` - Amount of downloaded data, low 32-bits of 64-bit value.
- **SizeHi** `(int)` - Amount of downloaded data, high 32-bits of 64-bit value.
- **SizeMB** `(int)` - Amount of downloaded data, in MiB.

#### ArticlesPerDays
Contains an array of structs with following fields:

- **Failed** `(int)` - Amount of failed articles.
- **Success** `(int)` - Amount of success articles.

#### Seconds, minutes and hours slots
These slots are arrays of fixed sizes (60, 60 and 24) which contain data for the last 60 seconds, 60 minutes and 24 hours. For example if current time “16:00:21” when the current slots would be:

- `SecSlot = 21`;
- `MinSlot = 0`;
- `HourSlot = 16`.
Element 21 of `BytesPerSeconds` contains the amount of data downloaded in current second. Element 20 - in the previous second and element 22 - data downloaded 59 seconds ago.

Similarly for minutes `(BytesPerMinutes)` and hours `(BytesPerHours)` arrays.

#### Daily slots
Daily slots are counted from the installation date, more precisely - from the date the program was used the first time after the installation. Or the first time it was used after deleting of statistics data, which is stored in directory pointed by option `QueueDir`.

Therefore the first element of `BytesPerDays` array contains the amount of data downloaded at the first day of program usage. The second element - on the second day. The sub-sequential slots are created for each day, regardless of the fact if the program was running on this day or not.

Similarly for `(ArticlesPerDays)` array.

Field `DaySlot` shows into which day slot the data is currently being written.

To find out the day slot for a specific day:

- get the timestamp for any time (hour/minute/second) of this day, in C/Unix format. The C/Unix time format is defined as the number of seconds that have elapsed since 00:00:00, Thursday, 1 January 1970.
- divide this number by the number of seconds in one day `(24*60*60=86400)`, take the integral part.
- subtract the number contained in field `FirstDay`.

For example, if the program was just installed and today is `17 March 2017` the field `FirstDay` will have value `17242` (timestamp 1489788000 divided by 86400). To find out the slot for `17 March 2017` we take the timestamp for any second of this day (for example [1489788000](http://www.unixtimestamp.com/)) and divide it by 86400 and subtract value of `FirstDay` (17242). We get number `0`, indicating slot 0. For `18 March 2017` the formula gives slot number `1` (which BTW doesn’t exist on 17 March yet).

#### Monthly and yearly data
You need to calculate the slot numbers for the first and the last days of a certain month, year or any other period and then sum the data from all slots belonging to the period. As an example see the source code of web-interface in file `status.js`.

#### Summary
As you see this method returns the raw data from the statistics meter and you have to perform calculations to represent the data in a user friendly format such as: downloaded today, yesterday, this or previous week or month etc.

### API-method `resetservervolume`

#### Signature
``` c++
bool resetservervolume(int ServerId, string Counter);
```

#### Description
Resets download volume statistics for a specified news-server. This method allows for selective resetting of counters.

#### Arguments
- **ServerId** `(int)` - Server ID to reset.
- **Counter** `(string)` - Specifies which counter to reset. The behavior depends on the value of this argument:

  - **"CUSTOM"** (case-sensitive) - Resets only the custom counter associated with the server. Use this option to clear custom statistics while preserving overall download volume data.
  - **""** (empty string) - Resets all counters associated with the server, including the overall download volume and any custom counters.

#### Return value
`true` on success or `false` on failure.

### API-method `rate`

#### Signature
``` c++
bool rate(int Limit);
```

#### Description
Set download speed limit.

#### Arguments
- **Limit** `(int)` - new download speed limit in KBytes/second. Value `0` disables speed throttling.

#### Return value
Usually `true` but may return `false` if parameter Limit was out of range.

### API-method `pausedownload`

#### Signature
``` c++
bool pausedownload();
```

#### Description
Pause download queue.

#### Return value
Always `true`.

### API-method `resumedownload`

#### Signature
``` c++
bool resumedownload();
```

#### Description
Resume (previously paused) download queue.

#### Return value
Always `true`.

### API-method `pausepost`

#### Signature
``` c++
bool pausepost();
```

#### Description
Pause post-processing.

#### Return value
Always `true`.

### API-method `resumepost`

#### Signature
``` c++
bool resumepost();
```

#### Description
Resume (previously paused) post-processing.

#### Return value
Always `true`.

### API-method `pausescan`

#### Signature
``` c++
bool pausescan();
```

#### Description
Pause scanning of directory with incoming nzb-files (option `NzbDir`).

#### Return value
Always `true`.

### API-method `resumescan`

#### Signature
``` c++
bool resumescan();
```

#### Description
Resume (previously paused) scanning of directory with incoming nzb-files (option `NzbDir`).

#### Return value
Always `true`.

### API-method `scheduleresume`

#### Signature
``` c++
bool scheduleresume(int Seconds);
```

#### Description
Schedule resuming of all activities after expiring of wait interval.

#### Arguments
- **Seconds** `(int)` - Amount of seconds to wait before resuming.

#### Return value
`true` on success or `failure` result on error.

#### Remarks
Method `scheduleresume` activates a wait timer. When the timer fires then all pausable activities resume: download, post-processing and scan of incoming directory. This method doesn’t pause anything, activities must be paused before calling this method.

Calling any of methods `pausedownload`, `resumedownload`, `pausepost`, `resumepost`, `pausescan`, `resumescan` deactivates the wait timer.

### API-method `config`

#### Signature
``` c++
struct[] config();
```

#### Description
Returns current configuration loaded into program. Please note that the configuration file on disk may differ from the loaded configuration. This may occur if the configuration file on disk was changed after the program was launched or the program may get some options passed via command line.

#### Return value
This method returns array of structures with following fields:

- **Name** `(string)` - Option name.
- **Value** `(string)` - Option value.

#### Remarks
- For options with predefined possible values (yes/no, etc.) the values are returned always in lower case.
- If the option has variable references (e. g. `${MainDir}/dst`) the returned value has all variables substituted (e. g. `/home/user/downloads/dst`).

### API-method `loadconfig`

#### Signature
``` c++
struct[] loadconfig();
```

#### Description
Reads configuration file from the disk.

#### Return value
This method returns array of structures with following fields:

- **Name** `(string)` - Option name.
- **Value** `(string)` - Option value.

#### Remarks
The option value is returned exactly as it is stored in the configuration file. For example it may contain variable references (e. g. `${MainDir}/dst`).

### API-method `saveconfig`

#### Signature
``` c++
bool saveconfig(struct[] Options);
```

#### Description
Saves configuration file to the disk.

#### Arguments
- **Options** `(struct[])`
  - **Name** `(string)` - Option name.
  - **Value** `(string)` - Option value.

#### Return value
`true` on success or `false` on failure.

### API-method `configtemplates`

#### Signature
``` c++
struct[] configtemplates(bool LoadFromDisk)
```

#### Description
Returns NZBGet configuration file template and also extracts configuration sections from all post-processing files. This information is for example used by web-interface to build settings page or page `Postprocess` in download details dialog.

#### Arguments
- **LoadFromDisk** `(bool)` - `v15.0` `true` - load templates from disk, `false` - give templates loaded on program start.

### Since 
`v23.0`

#### Return value
This method returns array of structures with a field:

- **Template** `(string)` - Content of the configuration template (multiple lines).

### Till 
`v23.0`

#### Return value
This method returns array of structures with following fields:

- **Name** `(string)` - Post-processing script name. For example `videosort/VideoSort.py`. This field is empty in the first record, which holds the config template of the program itself.
- **DisplayName** `(string)` - Nice script name ready for displaying. For example `VideoSort`.
- **PostScript** `(bool)` - `true` for post-processing scripts.
- **ScanScript** `(bool)` - `true` for scan scripts.
- **QueueScript** `(bool)` - `true` for queue scripts.
- **SchedulerScript** `(bool)` - `true` for scheduler scripts.
- **Template** `(string)` - Content of the configuration template (multiple lines).

### API-method `loadextensions`

### Since 
`v23.0`

#### Signature
``` c++
struct[] loadextensions(bool LoadFromDisk);
```

#### Description
Reads extensions from the disk.

#### Arguments
- **LoadFromDisk** `(bool)` - `true` - load extensions from disk, `false` - give extensions loaded on program start.

#### Return value
This method returns array of structures.

- **Entry** `(string)`
- **Location** `(string)`
- **RootDir** `(string)`
- **Name** `(string)`
- **DisplayName** `(string)`
- **About** `(string)`
- **Author** `(string)`
- **Homepage** `(string)`
- **License** `(string)`
- **Version** `(string)`
- **NZBGetMinVersion** `(string)`
- **PostScript** `(bool)`
- **ScanScript** `(bool)`
- **QueueScript** `(bool)`
- **SchedulerScript** `(bool)`
- **FeedScript** `(bool)`
- **QueueEvents** `(string)`
- **TaskTime** `(string)`
- **Description** `(string[])`
- **Requirements** `(string[])`
- **Options** `(struct[])`
  - **Name** `(string)`
  - **DisplayName** `(string)`
  - **Description** `(string[])`
  - **Select** `(string[])`
- **Commands** `(struct[])`
  - **Name** `(string)`
  - **Action** `(string)`
  - **DisplayName** `(string)`
  - **Description** `(struct[])`
- **Sections** `(struct[])`
  - **Name** `(string)`
  - **Prefix** `(string)`
  - **Multi** `(bool)`

See [Extensions](../extensions/EXTENSIONS.md).

### API-method `downloadextension`

### Since 
`v23.0`

#### Signature
``` c++
bool downloadextension(string URL, string ExtName);
```

#### Description
Downloads and installs the extension.

#### Arguments
- **URL** - URL to download an extension.
- **ExtName** - Extension name.

#### Return value
`true` on success or `false` on failure.

### API-method `updateextension`

### Since 
`v23.0`

#### Signature
``` c++
bool updateextension(string URL, string ExtName);
```

#### Description
Updates an extension.

#### Arguments
- **URL** `(string)` - URL to download an extension.
- **ExtName** `(string)` - Extension name.

#### Return value
`true` on success or `failure` result on error.

### API-method `deleteextension`

### Since 
`v23.0`

#### Signature
``` c++
bool deleteextension(string ExtName);
```

#### Description
Deletes an extension.

#### Arguments
- **ExtName** - Extension name.

#### Return value
`true` on success or `false` on failure.

### API-method `testextension`

### Since 
`v23.0`

#### Signature
``` c++
bool testextension(string ExtEntryName);
```

#### Description
Tries to find an executor program, e.g. Pytohn for the extension.

#### Arguments
- `ExtEntryName` - Extension entry filename.

#### Return value
`true` on success or `false` on failure.

### API-method `testserver`

#### Signature
``` c++
string testserver(
  string host, 
  int port, 
  string username, 
  string password, 
  bool encryption, 
  string cipher, 
  int timeout,
  int certVerificationLevel
);
```

#### Description
Tries to connect to a server.

#### Arguments
- **host** `(string)` - Server host.
- **port** `(int)` - Port.
- **username** `(string)` - User name.
- **password** `(string)` - User password.
- **encryption** `(bool)` - The inscription should be used.
- **cipher** `(string)` - Cipher for use.
- **timeout** `(int)` - Connection timeout.
- **certVerificationLevel** `(int)` - Certificate verification level:
  This is an enumerated type where the following integer values are recognized:
  -   **0:** None - NO certificate signing check, NO certificate hostname check
  -   **1:** Minimal - certificate signing check, NO certificate hostname check
  -   **2:** Strict - certificate signing check, certificate hostname check

#### Return value
`string` result.

### API-method `testserverspeed`

### Since 
`v24.2`

#### Signature
``` c++
bool testserverspeed(string nzbFileUrl, int serverId);
```

#### Description
Adds a test NZB file with the highest priority to be downloaded upon receiving download speed results.

#### Arguments
- **nzbFileUrl** `(string)` - NZB file url to download.
- **serverId** `(int)` - Server ID.

#### Return value
`true` on success or `failure` result on error.

### API-method `testdiskspeed`

### Since 
`v24.3`

#### Signature
``` c++
struct testdiskspeed(
    string dirPath, 
    int writeBufferSize, 
    int maxFileSize,
    int timeout,
);
```

#### Description
The function runs a sequential write test followed by a sequential read test on a temporary file created in the given directory. The test uses regular buffered file I/O, so the result reflects the throughput the program itself gets from the file system, including the OS file cache. Each phase ends when the maximum file size is reached or the timeout period expires. The test file is always removed afterwards.

Downloads are paused while the test is running and are resumed afterwards (unless they were already paused before the test).

The test file size is limited so that at least 100 MiB of free space always remains on the disk; if there is not enough free space the method fails.

#### Arguments
- **dirPath** `(string)` - The path to the directory where the test file will be created (UTF-8).
- **writeBuffer** `(int)` - The size of the block used for writing and reading (in KiB). `0` selects the default of 1 MiB.
- **maxFileSize** `(int)` - The maximum size of the file to be created (in GiB).
- **timeout** `(int)` - Timeout of each test phase, write and read (in seconds).

#### Return value
- **SizeMB** `(int)` - Written data size (in MiB).
- **DurationMS** `(int)` - Write phase duration (in milliseconds).
- **ReadSizeMB** `(int)` - Read data size (in MiB). Since `v27.0`.
- **ReadDurationMS** `(int)` - Read phase duration (in milliseconds). Since `v27.0`.

### API-method `testnetworkspeed`

### Since 
`v24.6`

#### Signature
``` c++
struct testnetworkspeed();
```

#### Description
Starts the network speed test.

#### Return value
- **SpeedMbps** `(double)` - The measured network speed in Mbps.
