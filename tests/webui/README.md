The Add dialog's grouping of copies of one release (`webui/dupegroup.js`, see
`docs/DROP_AS_FLEET.md`) is checked in plain Node, without a browser or daemon:
`node tests/webui/dupe-group.js`. Its name cases are the ones the Rust unit
test `releasename::tests::copy_suffixes_strip_only_after_a_group_tag` checks.
The dialog, Downloads and History around it run in Playwright, offline (the
details fixture, no daemon): `node tests/webui/drop-fleet.js http://127.0.0.1:17999/`
checks the "N files → 1 fleet" preview, the box on by default, plain appends
with the box off or Force on, two releases as two fleets, the fleet panel's
primary and twin, alt and unverified backups, honest projected health, and
History's donors (never FAILURE/HEALTH), with no console errors.
F16 (fleets stay in Downloads) runs offline the same way:
`node tests/webui/fleet-downloads.js http://127.0.0.1:17999/` checks one row
per fleet collapsed by default, the open state remembered per viewer, every
copy in the daemon's failover order (DupeFleetOrder) with its label and state,
"probing…" right after adding, and no fleet copy in History, in both themes at
desktop and phone widths.

The WebUI density regression check uses Playwright and Chrome against an isolated,
loopback NZBGet instance serving the current WebUI. The fixture should start with
one configured news server. Never use a production instance: the check changes
the browser's unsaved configuration, and intercepts `saveconfig` before it reaches
the daemon.

```sh
node tests/webui/workspaces.js http://127.0.0.1:17689/
node tests/webui/workspace-interactions.js http://127.0.0.1:17689/
node tests/webui/full-width-layout.js http://127.0.0.1:17689/
node tests/webui/settings-density.js http://127.0.0.1:17689/
node tests/webui/settings-lists.js http://127.0.0.1:17689/
node tests/webui/settings-list-health.js http://127.0.0.1:17689/
node tests/webui/settings-phone-navigation.js http://127.0.0.1:17689/
node tests/webui/settings-titles.js http://127.0.0.1:17689/
node tests/webui/settings-locales.js http://127.0.0.1:17689/
node tests/webui/settings-savebar.js http://127.0.0.1:17689/   # offline, no daemon needed
node tests/webui/settings-cards.js http://127.0.0.1:17999/     # offline, no daemon needed
node tests/webui/settings-modes.js http://127.0.0.1:17999/     # offline, no daemon needed
node tests/webui/settings-removals.js http://127.0.0.1:17999/  # offline, no daemon needed
node tests/webui/settings-servers.js http://127.0.0.1:17999/   # offline, no daemon needed
node tests/webui/settings-host-lookup.js http://127.0.0.1:17999/  # offline, no daemon needed
node tests/webui/system-metrics.js http://127.0.0.1:17999/     # offline: Status tiles, Statistics charts, License page
node tests/webui/settings-health-startup.js http://127.0.0.1:17689/
node tests/webui/tls-cipher.js http://127.0.0.1:17689/
node tests/webui/tls-cipher-settings.js http://127.0.0.1:17689/
node tests/webui/bad-paths.js build/debug/rustyget 17791       # starts its own daemon
node tests/webui/first-run.js build/debug/rustyget            # starts its own daemons
```

`bad-paths.js` starts its own loopback daemon from a copy of `rustyget.conf` with
`MainDir=/nonexistent`. It checks that Downloads, History, Messages and Settings
work without console errors, that Messages holds one "can't be used ... Change
<option> in Settings > Paths" warning per bad path, that everything is paused,
and that typing a valid MainDir in Settings > Paths, saving and reloading clears
the warnings and unpauses. `NZBGET_UI_CAPTURE_DIR` saves JPEGs of that state.

`first-run.js` starts its own loopback daemons from a copy of the binary with
HOME pointed at an empty folder, plus a `--nserv` test news server. With no
configuration file it checks the created file, the address on stdout, and the
setup guide at desktop/light and phone/dark: password rules, live folder checks,
plain-language connection errors (DNS, closed port), a successful test, saving
(with a backup), the restart and the sign-in; the phone run also checks the
expert skip, the banner that stays, and that Resume explains instead of resuming.
With a terrible configuration file (wrong types, a bad choice, an unknown option,
a broken line, a duplicate, binary bytes, non-UTF-8, a 70 kB line) it checks one
file:line warning per bad line with what was used instead, the banner listing
them, Settings flagging values it can't save (Save stays off until they are
fixed), the daemon refusing bad values without touching the file, the backup
before a save, restoring it (with a confirmation), and the confirmations for
clearing messages and discarding changes. A missing, folder, empty and
unreadable configuration file each start too. `FIRST_RUN_ONLY` picks parts
(`noconfig,noconfig-phone,garbage,garbage-phone,oddfiles`); console errors fail it.

The TLS cipher dialog check fulfills candidate assets and intercepts every RPC.
It runs at 1280, 390 and 320 pixels in both themes, covering duplicate starts,
active-job reopening, progress, escaped provider text, compatibility details,
saved/reload results, cancellation, unsaved-server guards, unavailable backends,
keyboard close and the final table column. Optional sanitized JPEGs use
`NZBGET_UI_CAPTURE_DIR`. This dialog check uses a Config stub; actual editor
reconciliation and native behavior require the isolated daemon fixture described
in `docs/FUNCTIONAL_TESTING.md` and separate Settings checks.
The `tls-cipher-settings.js` check uses the real Settings model and intercepts
optimizer and save RPCs. It checks unsaved-server rejection, unrelated unsaved
field retention, confirmed cipher baselines, changed-browser rejection and the
actual subsequent save payload at desktop/phone widths in both themes.
Add `--offline` to run that same model/editor workflow with committed templates
and synthetic read replies, without a daemon. Automatic application startup is
omitted in this mode; actual Config, Options, controls, RPC transport and save
serialization remain active.

Checks cover both themes at desktop and phone widths, compact option height,
keyboard help disclosure, server Edit/Close, unsaved summary updates, adding,
reordering, deleting and renumbering servers, search navigation, and the raw save
payload after closing an editor.

The dialog layout regression needs no daemon. It uses the same loopback-only
target guard, fulfills committed assets locally, and omits automatic startup.
Actual modal templates, styles, Bootstrap and the merge renderer/caller remain
active; the merge RPC is intercepted. It checks full-width explanations and
merge lists, the final entry, matching speed/unit edges, and retained field
columns at 1280/768/390/320 pixels in both themes. Optional JPEGs use the same
existing `NZBGET_UI_CAPTURE_DIR` convention.

```sh
node tests/webui/dialog-layout.js http://127.0.0.1:17689/
```

The cards check runs the whole WebUI offline through `details-fixture.js`. It covers the
Overview tiles and their links, the Dupe fleets page holding every dupe option (and
Incoming NZBs keeping only its own), Advanced folds, search reaching folded options,
and the saved payload after editing a moved option, in both themes at desktop and
phone widths.

The modes check runs offline the same way. It covers Simple as the default, what
Advanced and Expert add (rows, tags, sidebar pages, Overview tiles), the remembered
mode and its arrow keys, search across modes, checkboxes, dropdowns, digit-only
number boxes with +/- and arrow keys, clamping, sticky slider presets and their help
table, options greyed out until another option is changed, new list entries, and
the saved payload, in both themes at desktop and phone widths. The other Settings
checks set Expert first, because they cover every option.

The removals check (offline) covers the removed options having no rows, the 1-5
server Priority menu and an old Level read as a priority, the single Check and
repair card and its Renaming menu (Advanced) against the three options (Expert),
the Hard linking lock, the "Allow other computers" checkbox, no update button or
update check, and a save that writes Priority and leaves Level and the removed
options out.

The servers check (offline, both themes at 1440 and 390) covers the right-hand
pane beside the list on a wide window and the page view on a phone, Add server
only from the list, the Cipher menu and Optimize button without a `Server1.` key,
a saved cipher outside the menu, and Save server: it writes only the servers (an
unsaved change elsewhere stays unsaved), runs the cipher test for that server when
Cipher is Automatic, applies the chosen cipher, and reloads afterwards.

The host lookup check (offline) types a host and checks the address list with
country, network and connect time, picking an address (Host becomes the IP and a
strict certificate check becomes Minimal), going back to the name, a name that
does not resolve, and the note shown when the DB-IP files are missing.

The locale check verifies the server health label exists in all 20 translated
catalogs and renders translated summaries and help names in German, Spanish and
Turkish.

The list check exercises Categories, RSS Feeds and Scheduler tasks, including an
empty scheduler, scoped editor controls, keyboard Edit/Close, adding, moving,
deleting, renumbering, search, escaping and retained save values. Feed URLs and
scheduler command parameters are excluded from summaries.

The health check simulates option warnings and errors in read-only RPC responses
and verifies their counts remain visible on all three collapsed list types.

The phone-navigation check bounds the section menu, reaches every section by
keyboard, keeps the focused link in view, and checks that categories appear in
the first phone screen. Desktop retains the full menu in both themes.

The title check returns repeatedly from Paths to all five static Settings pages
and verifies their localized headings in English, German, and Turkish.

The full-width check compares navigation and content bounds as the desktop window
grows, verifies Status and Statistics use the available width in both themes,
and retains the compact category view on a phone.

The compact-tiles check exercises both themes at 1920, 1280, 1024, 390, and 320 pixels.
It verifies three/two/one column placement, labels above controls, source-order
tabbing, full-width main tabs, keyboard help, retained unsaved values, the discard
guard, and escaped extension summaries.
It also checks the desktop Save button's primary background and readable label
in normal, hover, and focus states, neutral disabled styling, and the View control.
Set `NZBGET_UI_CAPTURE_DIR` to an existing directory to capture sanitized desktop
JPEGs from the fixture; never use production values for those pictures.

The health-startup check holds the actual read-only health RPC reply until
Settings loads, then releases it and verifies the report arrives without errors.

The workspace checks cover Downloads, History, Statistics and Messages in both
themes at 1920, 1280, 768, 390 and 320 pixels. They verify accessible headings,
bounded table scrolling and final-column access, nonoverlapping provider charts,
and Add dialog geometry and payloads. The interaction check exercises native
keyboard activation, paging, row menus, download/history edits and their RPC
payloads, filters, status/volume navigation and empty views at desktop and phone
and tablet widths, including resizing across the phone breakpoint. Delayed
intercepted download and history saves exercise the real pending indicator timer
and verify disabled buttons remain readable without spinner overlap. Offline sanitized replies retain the real renderers; tested mutations
are intercepted. Both checks reject non-loopback targets. Optional JPEG captures
use an existing `NZBGET_UI_CAPTURE_DIR`; use a fresh directory for each attempt.

The download details check needs no daemon. `details-fixture.js` serves the
committed WebUI with its real startup and answers every RPC with a dupe fleet
(a downloading primary, a twin and a borrowable alt in the queue, a failed copy
in history); writes are intercepted. It checks, in both themes at 1440, 1280,
768, 390 and 320 pixels, that details open as a side panel spanning the window
height, fit without scrolling on a 900-pixel desktop window, list the fleet in
donor priority, switch to another copy (queue or history) from the fleet card,
widen for the Files page and restore on Back, save the shown copy's name, and
resume list updates on close. Optional JPEGs use `NZBGET_UI_CAPTURE_DIR`.
Set `NZBGET_CHROME_CHANNEL=none` to use Playwright's bundled Chromium.

```sh
node tests/webui/download-details.js http://127.0.0.1:17999/
```
