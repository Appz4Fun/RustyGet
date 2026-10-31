## About

The core functionality of NZBGet can be extended using extensions. 
NZBGet provides documented entry points for extensions. 
On certain events the extensions are executed, they receive information about event, 
do certain work and can communicate with NZBGet to give it instructions for further processing.

## Installation

### Using the Extension Manager

On the `SETTINGS` page you can easily find `EXTENSION MANAGER` 
and use it to install the extensions you need. Make sure that the internet connection is working 
and the path to the `7z` in `UNPACK->SevenZipCmd`(required for unpacking the extensions) is correct.
The list of available extensions can be found in the 
[nzbget-extensions](https://github.com/nzbgetcom/nzbget-extensions) repository.

### Manually, for your custom extensions

Option `ScriptDir` defines the location of extensions. 
To make an extension available in NZBGet put the extension into this directory. 
Then go to settings tab in web-interface (if you were already on settings 
tab switch to downloads tab and then back to settings 
tab to reread the list of available extensions from the disk).

Menu at the left of page should list all extensions found in `ScriptDir`. 
Select an extension to review or change its options (if it has any).

## Writing extension

In general, an extension consists of 2 files: `manifest.json` - 
with all the meta-data describing the extension 
and the `executable` file, like `main.py`.

`manifest.json` example:
```json
{
  "main": "main.py",
  "name": "EMail",
  "homepage": "https://github.com/nzbgetcom/Extension-EMail",
  "kind": "POST-PROCESSING",
  "displayName": "My EMail Extension",
  "version": "2.0",
  "nzbgetMinVersion": "24",
  "author": "John Doe",
  "license": "GNU",
  "about": "Sends E-Mail notification.",
  "queueEvents": "",
  "taskTime": "",
  "requirements": [
    "This script requires Python3.8 to be installed on your system."
  ],
  "description": ["This script sends E-Mail notification when the job is done."],
  "options": [
    {
      "name": "Server",
      "displayName": "Server",
      "value": "smtp.gmail.com",
      "description": ["SMTP server host."],
      "select": []
    },
    {
      "name": "Port",
      "displayName": "Port",
      "value": 25,
      "description": ["SMTP server port (1-65535)"],
      "select": [1, 65535]
    },
    {
      "section": "Categories",
      "name": "SendMail",
      "displayName": "SendMail",
      "value": "Always",
      "description": ["When to send the message."],
      "select": ["Always", "OnFailure"]
    }
  ],
  "commands": [
    {
      "name": "ConnectionTest",
      "action": "Send Test E-Mail",
      "displayName": "ConnectionTest",
      "description": ["To check connection parameters click the button."]
    },
    {
      "section": "Feeds",
      "name": "ConnectionTest",
      "action": "Send Test E-Mail",
      "displayName": "ConnectionTest",
      "description": ["To check connection parameters click the button."]
    }
  ],
  "sections": [
   {
      "name": "Categories",
      "prefix": "Category",
      "multi": true
    },
    {
      "name": "Feeds",
      "prefix": "Feed",
      "multi": false
    }
  ]
}
```

### `"main"`

The name of the executable file. 
For example: `main.py`, `main.exe`, `MyExt.sh`.

### `"name"`

The extension name, identifier and prefix needed to 
save the extension configuration options to `nzbget.conf`.

### `"homepage"`

Just a link to the repository where the extension lives.

### `"kind"`

Depending on the purpose of the extension, can be:
```
"kind": "POST-PROCESSING"
"kind": "SCAN"
"kind": "QUEUE"
"kind": "SCHEDULER"
"kind": "FEED"
```

If the extension can be used for multiple purposes the `kind` can be mixed, for example:
```
"kind": "SCAN/QUEUE"
```

More information about extension kinds can be found at:
 - [POST-PROCESSING](#post-processing-extensions)
 - [SCAN](#scan-extensions)
 - [QUEUE](#queue-extensions)
 - [SCHEDULER](#scheduler-extensions)
 - [FEED](#feed-extensions)

### `"displayName"`

The name that will be displayed in the web interface.

### `"version"`

Extension version.

### `"nzbgetMinVersion" (optional)`

NZBGet minimum required version.

### `"author"`

Author's name.

### `"license"`

Extension license.

### `"about"`

Brief description of the extension.

### `"description"`

For more detailed description of the extension.

### `"queueEvents"`

To describe the events that could be used by the extension, e.g.:

```json
"queueEvents": "NZB_ADDED, NZB_DOWNLOADED"
```

More information can be found at [QUEUE](#queue-extensions).

### "`requirements"`

To describe the list of requirements, e.g.:

```json
"requirements": [
  "This script requires Python3.8+ to be installed on your system."
],
```

### `"options"`

Let’s say we are writing an extension to send E-Mail notification. 
The user needs to configure the extension with options such as SMTP-Server host, 
port, login and password. To avoid hard-coding of these data in the extension 
NZBGet allows the extension to define the required options. 
The options are then made available on the settings page for user to configure.

In the provided `manifest.json` example we defined three options:

```json
"options": [
  {
    "name": "Server",
    "displayName": "Server",
    "value": "smtp.gmail.com",
    "description": ["SMTP server host."],
    "select": []
  },
  {
    "section": "Categories",
    "name": "Port",
    "displayName": "Port",
    "value": 25,
    "description": ["SMTP server port (1-65535)"],
    "select": [1, 65535]
  },
  {
    "name": "SendMail",
    "displayName": "SendMail",
    "value": "Always",
    "description": ["When to send the message."],
    "select": ["Always", "OnFailure"]
  }
],
```
>`"section"` property is optional. Default value is `"options"`. 
> Required `"nzbgetMinVersion"`: `"24"`.

When the user saves settings in web-interface the extension configuration 
options are saved to NZBGet configuration file using the extension name as prefix. For example:
```
EMail:Server=smtp.gmail.com
EMail:Port=25
```

Extension configuration options are passed using env-vars with prefix `NZBPO_`. 
There are two env-vars for each option:
 - one env-var with the name exactly as defined by the option `name`;
 - another env-var with the name written in UPPER CASE and with 
 special characters replaced with underscores.

For example, for option `Server.Name` two env-vars are passed: 
`NZBPO_Server.Name` and `NZBPO_SERVER_NAME`.

> In a case the user has installed the extension but have not saved the configuration, the options are not saved to 
configuration file yet. The extension will not get the options passed. 
This is a situation your extension must handle. You can either use a default settings or terminate the extension with 
a proper message asking the user to check and save configuration in web-interface. 

Example (python):
```python
required_options = ('NZBPO_FROM', 'NZBPO_TO', 'NZBPO_SERVER', 'NZBPO_PORT', 'NZBPO_ENCRYPTION',
'NZBPO_USERNAME', 'NZBPO_PASSWORD', 'NZBPO_FILELIST', 'NZBPO_BROKENLOG', 'NZBPO_POSTPROCESSLOG')
for optname in required_options:
if (not optname in os.environ):
    print('[ERROR] Option %s is missing in configuration file. Please check script settings' % optname[6:])
    sys.exit(POSTPROCESS_ERROR)
```

### `"commands"`

Sometimes it may be helpful to be able to execute extensions from the settings page. 
For example the EMail extension could use a button "Send test email". For other extensions something like 
"Validate settings" or "Cleanup database" may be useful too.

Starting from v19 it is possible to put buttons on the extension settings page. The buttons are defined as 
part of extension configuration, almost similar to extension configuration options:
```json
"commands": [
  {
    "section": "Categories",
    "name": "ConnectionTest",
    "action": "Send Test E-Mail",
    "displayName": "ConnectionTest",
    "description": ["To check connection parameters click the button."]
  }
],
```

This example creates a button with text "Send Test E-Mail" and description 
"To check connection parameters click the button.".

When user presses the button NZBGet executes the extension in a special context passing 
button name via env. var NZBCP_COMMAND. 
The extension can check if it runs in command mode by examining this variable (python example):
```python
# Exit codes used by NZBGet
COMMAND_SUCCESS = 93
COMMAND_ERROR = 94

# Check if the script is executed from settings page with a custom command
command = os.environ.get('NZBCP_COMMAND')
test_mode = command == 'ConnectionTest'
if command != None and not test_mode:
    print('[ERROR] Invalid command ' + command)
    sys.exit(COMMAND_ERROR)

if test_mode:
    print('[INFO] Test connection...')
    sys.exit(COMMAND_SUCCESS)

<script continues in normal mode>
```

During execution of the script NZBGet presents a special dialog showing extension output. 
The extension must exit with one of predefined exit codes indicating success 
(exit code `93`) or failure (exit code `94`).

User may close the progress dialog but the extension continues running in the background. 
All messages printed by the extension are saved to NZBGet log and are seen in web-interface on Messages tab.

### `"sections" (optional)`. 
>Required `"nzbgetMinVersion"`: `"24"`.

`Sections` are used to logically organize options and commands in `web-interface`.
>`Sections` with the reserved name `"options"` will be ignored.

```json
"sections": [
	{
		"name": "Categories",
		"prefix": "Category",
		"multi": true
	},
	{
		"name": "Feeds",
		"prefix": "Feed",
		"multi": false
	}
]

```

`"multi"` means that `options` or `commands` can be added dynamically in `web-intefrace.`

`"prefix"` is relevant for `multi` sections to avoid name conflicts of `options` saved in `nzbget.conf`.

Example of a `Name` option saved in `nzbget.conf` that refers to the `"Categories"` section with the prefix `"Category"`:

```conf
SpeedControl:Category1.Name=
SpeedControl:Category2.Name=
```

### `"taskTime"`

For [SCHEDULER](#scheduler-extensions) extensions.

Example:
```json
"taskTime": "*;*:00;*:30"
```

In that example the extension will be started at program boot (*) and then every 30 minutes.

## Internationalization (i18n)

>**Note:** Extension i18n requires NZBGet **v26.3** or newer.

Extensions can provide translations for their names, descriptions, options, and commands in the language selected by the user in the WebUI.

### How it works

There are **three files** involved in extension translation, each with a different role:

| File | Purpose |
|---|---|
| `manifest.json` | **Runtime English fallback.** `displayName`, `about`, option `displayName`/`description` values are shown in the WebUI when no translation exists. |
| `locales.source.json` | **Input for the build tool.** Contains English source strings that translators translate into other languages. The English text here duplicates `manifest.json`. |
| `_locales/{lang}/messages.json` | **Translated strings.** Generated by nzbabel from `locales.source.json`. Loaded at runtime by the WebUI. |

### `locales.source.json`

Create `locales.source.json` in the extension root directory with English source strings. This file is the **input for the nzbabel build tool** — it gives translators the English text and describes what each string is used for. The English text in this file must match what is in `manifest.json`.

```json
{
    "about": {
        "message": "About EMail",
        "description": "Section heading for extension description in settings. DO NOT translate 'EMail'."
    },
    "description": {
        "message": "This script sends E-Mail notification when the job is done.",
        "description": "Full description of the extension shown in settings"
    },
    "requirements": {
        "message": "This script requires Python3.8+ to be installed on your system.",
        "description": "System requirements displayed to the user"
    },
    "server_desc": {
        "message": "SMTP server host.",
        "description": "Help text for the Server option in EMail."
    },
    "port_desc": {
        "message": "SMTP server port (1-65535)",
        "description": "Help text for the Port option in EMail."
    },
    "connectiontest_desc": {
        "message": "To check connection parameters click the button.",
        "description": "Help text for the ConnectionTest command in EMail."
    }
}
```

### Key naming conventions

| Key pattern | Scope | Example |
|---|---|---|
| `about` | Section heading above the extension description in settings | `about` → "About EMail" |
| `description` | Full description shown in settings | `description` → "This script sends..." |
| `requirements` | System requirements displayed to the user | `requirements` → "Requires Python3.8+" |
| `{optionname}_desc` | Option help text | `server_desc` → "SMTP server host." |
| `{commandname}_desc` | Command help text | `connectiontest_desc` → "To check connection parameters..." |

>**Note:** `displayName` keys (for extension name, option labels, and command button text) are intentionally **not** generated by `generate_locales.py`. The WebUI's fallback mechanism automatically displays the English `displayName` value from `manifest.json` when a translation key is missing.

### Translation workflow

1. **Extension author** writes `locales.source.json` with English `message` values (matching the text in `manifest.json`) and informative `description` fields for translators.
2. **Build system** (nzbabel) reads `locales.source.json` and generates `_locales/{lang}/messages.json` files for each target language, where non-English `message` values are replaced with translations.
3. **WebUI** loads translations from `extensions/{extname}/_locales/{lang}/messages.json` for each installed extension and merges them with the prefix `ext_{extname}_` (e.g., `ext_email_about`).
4. **Fallback**: The `manifest.json` values are always the fallback, in two scenarios:
   - **Pre-v26.3 NZBGet**: The i18n system is not present. The extension works exactly as before — all text comes from `manifest.json` and `_locales/` files are ignored entirely.
   - **v26.3+ NZBGet**: If the `_locales/` directory does not exist or a specific key is missing, the WebUI shows the English text from `manifest.json`. The UI always shows readable text — never raw keys.

### Generation script

The NZBGet repository provides `scripts/generate_locales.py` to generate or update `locales.source.json` from `manifest.json`:

```bash
python3 path/to/nzbget/scripts/generate_locales.py path/to/Extension-Foo/manifest.json
```

In **merge mode** (default) the script reads an existing `locales.source.json`, updates `message` values from `manifest.json`, and preserves `description` hints for translators. Use `--fresh` to regenerate the file entirely.

## Tips

### Testing NZBGet version
NZBGet version is passed in env-var NZBOP_VERSION. Example values:

for stable version: nzbget-11.0;
for testing version: nzbget-11.0-testing-r620
Example (python):
```python
NZBGetVersion=os.environ['NZBOP_VERSION']
if NZBGetVersion[0:5] < '11.1':
    print('[ERROR] This script requires NZBGet 11.1 or newer. Please update NZBGet')
    sys.exit(POSTPROCESS_ERROR)
```

## Communication with NZBGet via RPC-API

With RPC-API more things can be done than using command line. For documentation on available 
RPC-methods see [API](../api/API.md).

Example: obtaining post-processing log of current nzb-file (this is a short version of script Logger.py supplied with NZBGet):

```python
import os
import sys
import datetime
from xmlrpc.client import ServerProxy

## Exit codes used by NZBGet
POSTPROCESS_SUCCESS = 93
POSTPROCESS_ERROR = 94

# To get the post-processing log we connect to NZBGet via XML-RPC
# and call method "postqueue", which returns the list of post-processing job.
# The first item in the list is current job. This item has a field 'Log',
# containing an array of log-entries.
# For more info visit https://nzbget.com/documentation/api/

# First we need to know connection info: host, port, username and password of NZBGet server.
# NZBGet passes all configuration options to post-processing script as
# environment variables.
host = os.environ["NZBOP_CONTROLIP"]
port = os.environ["NZBOP_CONTROLPORT"]
username = os.environ["NZBOP_CONTROLUSERNAME"]
password = os.environ["NZBOP_CONTROLPASSWORD"]

if host == "0.0.0.0":
    host = "127.0.0.1"

# Build an URL for XML-RPC requests
rpcUrl = f"http://{username}:{password}@{host}:{port}/xmlrpc"

# Create remote server object
server = ServerProxy(rpcUrl)

# # Call remote method 'postqueue'. The only parameter tells how many log-entries to return as maximum.
postqueue = server.postqueue(10000)

# # Get field 'Log' from the first post-processing job
log = postqueue[0]["Log"]

# post proccessing log file
pplog_file = f"{os.environ['NZBPP_DIRECTORY']}/_postprocesslog.txt"

# # Now iterate through entries and save them to the output file
if len(log) > 0:
    with open(pplog_file, "w") as f:
        for entry in log:
            timestamp = datetime.datetime.fromtimestamp(int(entry["Time"]))
            output_file = f"{entry['Kind']}\t{timestamp}\t{entry['Text']}\n"
            f.write(output_file)
    
        f.close()

sys.exit(POSTPROCESS_SUCCESS)
```

## Post-processing extensions

### About

After the download of nzb-file is completed NZBGet can call post-processing extensions (pp-extensions). 
The extensions can perform further processing of downloaded files such es delete unwanted files (*.url, etc.), 
send an e-mail notification, transfer the files to other application and do any other things. 
Please note that the par-check/repair and unpack are performed by NZBGet internally and are not part of 
post-processing extensions. You can activate par-check/repair and unpack without using of any post-processing extensions.

Please note that these settings define "defaults" for nzb-file. When you deactivate/activate the extension in the Extension Manager
this has no effect on already enqueued downloads. However it’s possible to alter the assigned pp-extensions 
for each nzb-file individually. To do so click on the nzb-file in the list of downloads, then click on Postprocess.

If you use more than one extension for one nzb-file it can be important to set the correct order of execution 
in the Extension Manager.

### Writing post-processing extensions

> Please read [Extensions](#about) for general information about extensions first.

### Nzb-file information

#### The information about nzb-file which is currently processed:

- **NZBPP_DIRECTORY** - Path to destination dir for downloaded files.
- **NZBPP_FINALDIR** - Final directory, if set by one of previous scripts (see below).
- **NZBPP_NZBNAME** - User-friendly name of processed nzb-file as it is displayed by the program. 
 The file path and extension are removed. If download was renamed, this parameter reflects the new name.
- **NZBPP_NZBFILENAME** - Name of processed nzb-file. If the file was added from incoming nzb-directory, 
 this is a full file name, including path and extension. If the file was added from web-interface, 
 it’s only the file name with extension. If the file was added via RPC-API (method append), 
 this can be any string but the use of actual file name is recommended for developers.
- **NZBPP_CATEGORY** - Category assigned to nzb-file (can be empty string).
- **NZBPP_TOTALSTATUS** - `v13.0` Total status of nzb-file:
  - **SUCCESS** - everything `OK`;
  - **WARNING** - download is damaged but probably can be repaired; user intervention is required;
  - **FAILURE** - download has failed or a serious error occurred during post-processing (unpack, par);
  - **DELETED** - download was deleted; post-processing scripts are usually not called in this case; 
 however it’s possible to force calling scripts with command "post-process again".
- **NZBPP_STATUS** - Complete status info for nzb-file: it consists of total status and status detail separated with slash. 
 There are many combinations. Just few examples:
  - **FAILURE/HEALTH**;
  - **FAILURE/PAR**;
  - **FAILURE/UNPACK**;
  - **WARNING/REPAIRABLE**;
  - **WARNING/SPACE**;
  - **WARNING/PASSWORD**;
  - **SUCCESS/ALL**;
  - **SUCCESS/UNPACK**.
For the complete list see description of [API-Method "history"](../api/API.md#api-method-history).
  - **NOTE** one difference to the status returned by method history is that `NZBPP_STATUS` assumes all scripts are ended successfully. Even if one of the scripts executed before the current one has failed the status will not be set to `WARNING/SCRIPT` as method history will do. For example for a successful download the status would be `SUCCESS/ALL` instead. Because most scripts don’t depend on other scripts they shouldn’t assume the download has failed if any of the previous scripts (such as a notification script) has failed. The scripts interested in that info still can use parameter `NZBPP_SCRIPTSTATUS`.
- **NZBPP_SCRIPTSTATUS** - `v13.0` Summary status of the scripts executed before the current one:
  - **NONE** - no other scripts were executed yet or all of them have ended with exit code `NONE`;
  - **SUCCESS** - all other scripts have ended with exit code "SUCCESS" ;
  - **FAILURE** - at least one of the script has failed.
- **NZBPP_PARSTATUS** - `v13.0` Result of par-check:
  - **0** = not checked: par-check is disabled or nzb-file does not contain any par-files;
  - **1** = checked and failed to repair;
  - **2** = checked and successfully repaired;
  - **3** = checked and can be repaired but repair is disabled;
  - **4** = par-check needed but skipped (option ParCheck=manual).
  - `Deprecated, use NZBPP_STATUS and NZBPP_TOTALSTATUS instead.`
- **NZBPP_UNPACKSTATUS** - `v13.0` Result of unpack:
  - **0** = unpack is disabled or was skipped due to nzb-file properties or due to errors during par-check;
  - **1** = unpack failed;
  - **2** = unpack successful;
  - **3** = write error (usually not enough disk space);
  - **4** = wrong password (only for rar5 archives).
  - `Deprecated, use NZBPP_STATUS and NZBPP_TOTALSTATUS instead.`
  - **NOTE** if DirectUnpack (option `DirectUnpack`) already extracted the files successfully before par-check finished,
 `NZBPP_UNPACKSTATUS=2` (unpack successful) can be reported together with `NZBPP_PARSTATUS=1` (checked and failed to repair):
 NZBGet finalizes the already-extracted files instead of skipping unpack in this case. `NZBPP_STATUS`/`NZBPP_TOTALSTATUS`
 still reflect the par failure (e.g. `FAILURE/PAR`).

#### Example: test if download failed
```sh
if [ "$NZBPP_TOTALSTATUS" != "SUCCESS" ]; then
    echo "[ERROR] This nzb-file was not processed correctly, terminating the script"
    exit 95
fi
```

### Exit codes
For post-processing extensions NZBGet checks the exit code of the script and sets the status in history item accordingly. 
Extensions can use following exit codes:
- **93** - Post-process successful (`status = SUCCESS`).
- **94** - Post-process failed (`status = FAILURE`).
- **95** - Post-process skipped (`status = NONE`). Use this code when you script terminates immediately without doing any job and when this is not a failure termination.
- **92** - Request NZBGet to do par-check/repair for current nzb-file. This code can be used by pp-scripts doing unpack on their own.
Extensions MUST return one of the listed status codes. 
If any other code (including 0) is returned the history item is marked with status `FAILURE`.

Example: exit code
```sh
POSTPROCESS_SUCCESS = 93
POSTPROCESS_ERROR = 94
echo "Hello from test script"
exit $POSTPROCESS_SUCCESS
```

### Control commands

pp-extensions which move downloaded files can inform NZBGet about that by printing special 
messages into standard output (which is processed by NZBGet). 
This allows the extensions called thereafter to process the files in the new location. Use command `DIRECTORY`:
```sh
echo "[NZB] DIRECTORY=/path/to/new/location"
```
The extensions executed after your script assume the files in the directory belong to the current download. 
If your extension moves files into a non empty directory which already contains other files, your should not use that command. 
Otherwise the other extensions may process all files in the directory, even the files existed before. 
In that case use command `FINALDIR` instead (which has less consequences):
```sh
echo "[NZB] FINALDIR=/path/to/new/location"
```

## Scan extensions

### About

Scan extensions are called when a new file is found in the incoming nzb directory (option NzbDir). 
If a file is being added via web-interface or via RPC-API from a third-party app the file is saved into nzb directory and then processed. NZBGet loads only files with nzb-extension but it calls the scan extensions for every file found in the nzb directory. 
This allows for example for scan extensions which unpack zip-files containing nzb-files.

### Writing scan extensions

> Please read [Extensions](#about) for general information about extensions first.

### File information
 - NZBNP_DIRECTORY - Path to directory, where file is located. It is a directory specified by the option NzbDir or a subdirectory.
 - NZBNP_FILENAME - Name of file to be processed.
 - NZBNP_NZBNAME - Nzb-name (without path but with extension). The extension can rename the file if needed.
 - NZBNP_URL - URL this nzb-file was downloaded from.
 - NZBNP_CATEGORY - Category of nzb-file. The extension can change this setting (see later).
 - NZBNP_PRIORITY - Priority of nzb-file. The extension can change this setting (see later).
 - NZBNP_TOP - Flag indicating that the file will be added to the top of queue: 0 or 1. The extension can change this setting (see later).
 - NZBNP_PAUSED - Flag indicating that the file will be added as paused: 0 or 1. The extension can change this setting (see later).
 - NZBNP_DUPEKEY - Duplicate key (see [RSS](../HOW_TO_USE.md#rss-and-duplicate-check)). The extension can change this setting (see later).
 - NZBNP_DUPESCORE - Duplicate score (see [RSS](../HOW_TO_USE.md#rss-and-duplicate-check)). The extension can change this setting (see later).
 - NZBNP_DUPEMODE - Duplicate mode, one of SCORE, ALL, FORCE (see [RSS](../HOW_TO_USE.md#rss-and-duplicate-check)). The extension can change this setting (see later).


### Control commands

Scan extensions can change nzb-name, category, priority, post-processing parameters and top-/paused-flags 
of the nzb-file by printing special messages into standard output (which is processed by NZBGet).

To change nzb-name use following syntax:
```sh
echo "[NZB] NZBNAME=my download"
```
To change category:
```sh
echo "[NZB] CATEGORY=my category"
```
To change priority:
```sh
echo "[NZB] PRIORITY=signed_integer_value"
```
For example: to set priority higher than normal:
```sh
echo "[NZB] PRIORITY=50"
```
Another example: use a negative value for "lower than normal" priority:
```sh
echo "[NZB] PRIORITY=-100"
```

Although priority can be any integer value, the web-interface 
operates with six predefined priorities: `*-100` - very low priority; `*-50` - low priority; `*0` - normal priority (default); 
`*50` - high priority; `*100` - very high priority; `*900` - force priority.

Downloads with priorities equal to or greater than `900` are downloaded and post-processed 
even if the program is in paused state (force mode).

To change duplicate key:
```sh
echo "[NZB] DUPEKEY=tv show s01e02"
```
To change duplicate score:
```sh
echo "[NZB] DUPESCORE=integer_value"
```
To change duplicate mode:
```sh
echo "[NZB] DUPEMODE=(SCORE|ALL|FORCE)"
```
To assign post-processing parameters:
```sh
echo "[NZB] NZBPR_myvar=my value"
```
The prefix `NZBPR_` will be removed. In this example a post-processing parameter with name `myvar` 
and value `my value` will be associated with nzb-file.

To change top-flag (nzb-file will be added to the top of queue):
To assign post-processing parameters:
```sh
echo "[NZB] TOP=1"
```
To change paused-flag (nzb-file will be added in paused state):
```sh
echo "[NZB] PAUSED=1"
```
Scan extensions can delete processed file, rename it or move somewhere. After the calling of the extension the file will be 
either added to 
queue (if it was an nzb-file) or renamed by adding the extension `.processed`.

> Files with extensions ".processed", ".queued" and ".error" are skipped during the directory scanning.

> Files with extension ".nzb_processed" are not passed to scan-extension before adding to queue. This feature allows scan-extension
to prevent the scanning of nzb-files extracted from archives, if they were already processed by the extension.

> Files added via RPC calls in particular from web-interface are saved into incoming nzb-directory and then processed by the extension.

## Queue extensions

### About 
Queue extensions are called after the download queue was changed. 
In the current version the queue extensions are called on defined events, described later. 
In the future they can be called on other events too.

### Writing queue extensions

> Please read [Extensions](#about) for general information about extensions first.

### Queue event information

 - **NZBNA_DIRECTORY** - Destination directory for downloaded files.
 - **NZBNA_FILENAME** - Filename of the nzb-file. If the file was added from nzb-directory this is the fullname with path. 
 If the file was added via web-interface it contains only filename without path.
 - **NZBNA_NZBNAME** - Nzb-name as displayed in web-interface.
 - **NZBNA_URL** - URL if the nzb-file was fetched from an URL.
 - **NZBNA_CATEGORY** - Category of nzb-file.
 - **NZBNA_PRIORITY** - Priority of nzb-file.
 - **NZBNA_NZBID** - ID of queue entry, can be used in RPC-calls.
 - **NZBNA_EVENT** - Describes why the extension was called. See below.
 - **NZBNA_URLSTATUS** - Details for event type URL_COMPLETED. One of `FAILURE`, `SCAN_SKIPPED`, `SCAN_FAILURE`.
 - **NZBNA_DELETESTATUS** - Details for event type NZB_DELETED. One of `MANUAL`, `DUPE`, `BAD`, `GOOD`, `COPY`, `SCAN`.

### Queue event type

The event type is passed with env. var NZBNA_EVENT and can have following values:
 - **NZB_ADDED** - after adding of nzb-file to queue;
 - **FILE_DOWNLOADED** - after a file included in nzb is downloaded;
 - **NZB_DOWNLOADED** - after all files in nzb are downloaded (before post-processing);
 - **NZB_DELETED** - after nzb-file is deleted from queue, by duplicate check or manually by user;
 - **URL_COMPLETED** - after an nzb-file queued with URL is fetched but could no be added for download.
In the future the list of supported events may be extended. To avoid conflicts with future NZBGet versions the extension 
must exit if the parameter has a value unknown to the extension:
```python
if os.environ.get('NZBNA_EVENT') not in ['NZB_ADDED', 'NZB_DOWNLOADED']:
    sys.exit(0)
```

In addition to checking the parameter NZBNA_EVENT the extension can tell NZBGet what events it is interested in in `manifest.json`:
```json
"queueEvents": "NZB_ADDED, NZB_DOWNLOADED",
```
That has an advantage that the extension isn’t called on events it can’t process anyway.

### Control commands

Queue extensions can change properties of nzb-file by printing special messages into standard output (which is processed by NZBGet).
To assign post-processing parameters:
```sh
echo "[NZB] NZBPR_myvar=my value"
```

The prefix `NZBPR_` will be removed. In this example a post-processing parameter 
with name `myvar` and value `my value` will be associated with nzb-file.

To cancel downloading and mark nzb as bad:
```sh
echo "[NZB] MARK=BAD"
```
When processing event NZB_DOWNLOADED, which is fired after all files are downloaded and 
just before unpack starts the extension can change final directory for nzb:
```sh
echo "[NZB] DIRECTORY=/mnt/movies"
```

### Synchronous/asynchronous and event queue

Only one queue extension is executed at a given time. If other events have occurred during executing of the 
extension the events are stored in a special queue and are processed later, when the extension terminates.

There are two kinds of events: synchronous and asynchronous. When a synchronous event must be processed NZBGet executes 
queue extension and waits for its termination before processing the nzb-file further. 
Only one event is of this kind - it’s `NZB_DOWNLOADED`.

All other events are asynchronous. When a extension is executed for an asynchronous event 
it’s possible that at the time a certain part of extension is being processed the state of 
nzb-file was changed or the file was even deleted. 
Extensions must take this possibility into account.

Although events `FILE_DOWNLOADED` are supposed to be executed after each downloaded file (part of nzb), 
no more than one event of this kind for a given nzb is preserved in the processing queue. 
This means that if during execution of queue extension multiple files were completed only one event `FILE_DOWNLOADED` 
will be processed. Therefore extension should not assume to be called for every downloaded file. 
Furthermore there is a program option EventInterval to reduce 
the number of extension executions (and system load) for event `FILE_DOWNLOADED`.

## Scheduler extensions

### About

Scheduler extensions are called by scheduler tasks (setup by the user).

### Writing scheduler extensions

> Please read [Extensions](#about) for general information about extensions first.

### Schedule

Users can activate scheduler extensions in two ways:

By creating a scheduler task with `TaskX.Command=Script` and choosing the extension. The schedule time is configured 
via scheduler task in option `TaskX.Time`.
For many scheduler extensions a reasonable default schedule can be provided by extension author, via extension definition. 
Scheduler extensions with default schedule can be activated/deactiated the Extension Manager; 
no creation of a scheduler task is required. If user doesn’t like the default scheduling he can create a scheduler task 
(instead of or in addition to activating the script via option "Extensions").

Example (explained below):
```json
{
  "main": "main.py",
  "name": "MyExtension",
  "homepage": "https://github.com/nzbgetcom/Extension-MyExtension",
  "kind": "QUEUE/SCHEDULER/POST-PROCESSING",
  "displayName": "My Extension",
  "version": "2.0",
  "author": "John Doe",
  "license": "GNU",
  "about": "Test script.",
  "queueEvents": "NZB_ADDED",
  "requirements": [],
  "description": ["This is a long description for test script."],
  "options": [
    {
      "name": "Option1",
      "displayName": "Option 1",
      "value": "yes",
      "description": ["Example script option (yes, no)."],
      "select": ["yes", "no"]
    }
  ],
  "commands": [],
  "taskTime": "*;*:00;*:30"
}
```

## Feed extensions

### About 

Feed extensions are called after an RSS feed is read and before it is processed by NZBGet.

### Writing feed extensions

> Please read [Extensions](#about) for general information about extensions first

### Feed information
>TODO

## LEGACY

> Starting NZBGet v23 all extensions should be using Extension Manager with new extensions format - it allows manage/update/install extensions easily, new approach is simple and flexible. 

> NZBGet v23 (Extension Manager) is compatible with new extensions and legacy extensions.

If you run NZBGet v22 or older - following information may be helpful:
 - [Extension scripts](https://nzbget.net/extension-scripts)
 - [Extension scripts forum](https://forum.nzbget.net/viewforum.php?f=8&sid=dc9d094d3efcafe6c217cbceae9ea721)
