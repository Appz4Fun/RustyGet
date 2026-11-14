# Oxide: the RustyGet design system

Oxide is the visual language of RustyGet. It's taken from the project's GitHub Pages site ([appz4fun.github.io/RustyGet](https://appz4fun.github.io/RustyGet/), source in `site/assets/site.css`), and this file describes it so the web UI and any other surface can use the same system.

It borrows from metal and the oxides that form on it: iron oxide (rust) for action, copper patina for health, steel for reference, and cast iron for text.

| File | What it is |
|---|---|
| `DESIGN.md` | This guide: principles, roles, type, space, components, themes |
| `docs/design-system/oxide.json` | Token source for Oxide, including the mapping onto the web UI's token names |
| `webui/oxide.css` | Generated CSS. Load it after `tokens.css` to give the web UI the Oxide look |
| `scripts/gen_oxide_tokens.py` | Writes `webui/oxide.css`. `--check` also checks every contrast pair |
| `site/assets/site.css` | The Pages site's own copy of the tokens, the reference values |
| `site/content/design-system.md` | The public design-system page with live swatches |

```sh
python3 scripts/gen_oxide_tokens.py          # regenerate webui/oxide.css
python3 scripts/gen_oxide_tokens.py --check  # fail if stale, if a contrast pair fails, or if a web UI token has no mapping
```

## Principles

- **Functional first.** Every element helps someone find, read, or act. Decoration that doesn't do one of those jobs stays out.
- **Quiet surfaces, one accent.** Warm paper and cast-iron ink carry the page. Rust marks the single most important action or state in view.
- **Color carries meaning.** Each accent has one job and keeps it everywhere: on the site, in README badges, and in the web UI.
- **Readable in both themes.** Every text color passes WCAG AA (4.5:1) on every surface it appears on, in light and dark.
- **Feels like the toolchain.** Callouts read like compiler diagnostics, locations read like Rust module paths, and code and numbers are set in monospace.

## Color

Values are light / dark. Use the token, never the hex.

### Surfaces and text

| Token | Light | Dark | Role |
|---|---|---|---|
| `--paper` | `#f7f4ef` | `#12100e` | Page background |
| `--surface` | `#ffffff` | `#1b1714` | Raised cards, tables, panels, dialogs |
| `--sunken` | `#f0eae2` | `#1e1915` | Inset areas: code, table headers, hovered nav items |
| `--ink` | `#1c1916` | `#f1eae2` | Body text and icons |
| `--muted` | `#5c534b` | `#b3a79b` | Secondary text: help, sizes, ETAs, captions, column headers |
| `--faint` | `#9b9085` | `#6f655c` | Disabled text and decorative glyphs only (below 4.5:1 on purpose) |
| `--line` | `#e4dcd2` | `#352e28` | Hairlines: borders, rules, row dividers |
| `--line-strong` | `#cfc4b8` | `#4a4038` | Emphasised rules, `kbd` edges, hovered card borders |
| `--control-edge` | `#8c8076` | `#7d7166` | Edges of controls that must be seen, such as an unchecked checkbox (3:1) |

`--faint` and `--control-edge` are additions for the web UI. The site doesn't need them, but an app with checkboxes and disabled states does. Both are derived from `--muted` and `--line-strong`.

### Accents

Each accent has a `-soft` partner for tinted grounds. Accent text sits on `--paper`, `--surface`, or its own `-soft` color.

| Token | Light | Dark | Meaning | Used for |
|---|---|---|---|---|
| `--rust` | `#a8400f` | `#f08c5a` | Action and emphasis | Links, the primary button, the current page or tab, focus rings, `new` tags, the primary copy of a fleet |
| `--patina` | `#1d6f60` | `#6cc9b4` | Healthy and verified | Success, downloading, twins proven by par2 checksums, completed steps, `help:` callouts |
| `--steel` | `#3d5a73` | `#9ab8d1` | Reference and neutral | `note:` callouts, alts, `changed` tags, info and live-repair labels |
| `--amber` | `#8a5a00` | `#e3b45a` | Needs attention | Warnings, paused, degraded health, settings that send network traffic |
| `--ember` | `#b02a1f` | `#f2867a` | Failure and danger | Failed downloads, errors, destructive buttons, settings that delete data |
| `--on-accent` | `#ffffff` | `#12100e` | Text on solid rust | Primary button label, selected tab, active page number |

Soft grounds: `--rust-soft` `#f7e6dc` / `#3b2318`, `--patina-soft` `#dcefe9` / `#16302a`, `--steel-soft` `#e1e9f0` / `#1d2a35`, `--amber-soft` `#f6ebd2` / `#352914`, `--ember-soft` `#f8e1de` / `#3a1c18`.

Note that in dark the rust fill is light, so `--on-accent` flips to near-black. Never hard-code white text on rust.

### Contrast

Against `--paper`, measured by `gen_oxide_tokens.py --check`:

| Token | Light | Dark |
|---|---|---|
| `--ink` | 16.0:1 | 15.9:1 |
| `--muted` | 6.9:1 | 8.1:1 |
| `--rust` | 5.6:1 | 7.8:1 |
| `--patina` | 5.5:1 | 9.6:1 |
| `--steel` | 6.6:1 | 9.2:1 |
| `--amber` | 5.4:1 | 9.9:1 |
| `--ember` | 6.0:1 | 7.7:1 |
| `--on-accent` on `--rust` | 6.2:1 | 7.8:1 |
| `--control-edge` on `--surface` | 3.8:1 | 3.8:1 |

The lowest text pairing is `--amber` on `--amber-soft` at 5.0:1. Add a pairing to `contrast` in `oxide.json` whenever a new one is used.

## Typography

- **Text:** the platform stack, `system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif`. It loads instantly and matches the operating system.
- **Mono:** `ui-monospace, "JetBrains Mono", "Cascadia Code", "SF Mono", Menlo, Consolas, monospace`. Code, file names, sizes, speeds, tags, nav labels, eyebrows and module paths use it.

| Style | Size | Line height | Weight | Tracking | Family |
|---|---|---|---|---|---|
| Page title | `clamp(2rem, 3.4vw, 2.75rem)` | 1.2 | 680 | -0.035em | sans |
| H2 | 1.55rem | 1.2 | 680 | -0.02em | sans |
| H3 | 1.15rem | 1.2 | 680 | -0.02em | sans |
| Lede | 1.15rem | 1.6 | 400 | | sans, `--muted` |
| Body | 16px | 1.65 | 400 | | sans |
| Small | 0.9rem | 1.55 | 400 | | sans |
| Code | 0.86rem | 1.6 | 400 | | mono |
| Eyebrow | 0.74rem | 1.4 | 600 | 0.06em, uppercase | mono, `--rust` |
| Label | 0.7rem | 1.4 | 600 | 0.08em, uppercase | mono, `--muted` |
| Tag | 0.68rem | 1.5 | 600 | 0.03em | mono |

Bold is 650. Paragraphs stop at about 76 characters.

## Space, shape, and elevation

- **Spacing** is a 4px scale: `--space-1` 4, `-2` 8, `-3` 12, `-4` 16, `-5` 20, `-6` 24, `-8` 32, `-10` 40.
- **Radius:** `--radius` 10px for panels, cards, tables and dialogs; `--radius-sm` 6px for buttons, fields and nav items; `--radius-xs` 4px for tags, inline code and `kbd`; `--radius-full` for pills.
- **Borders** are 1px `--line`. Structure comes from borders, not shadows.
- **Shadow** (`--shadow`) appears only on hover and on floating panels such as menus, search results and dialogs.
- **Layout:** 60px masthead, a sidebar, an 800px content column (1120px for wide pages), and a 1560px page maximum. Desktop gutter is 40px; phones keep a 16px gutter and fold the sidebar into a menu.
- **Motion** is short and functional. Honour `prefers-reduced-motion: reduce` by turning it off.

## Components

**Buttons.** Default: `--surface` fill, 1px `--line-strong` border, `--ink` label, 600 weight, `--radius-sm`, padding 9px 16px; the border turns `--ink` on hover. Primary: `--rust` fill, `--on-accent` label, `brightness(1.08)` on hover. Destructive: `--ember-soft` ground with `--ember` text. One primary button per view.

**Links.** `--rust`, underlined 1px with 3px offset, 2px on hover. External links get a trailing ↗.

**Focus.** 2px solid `--rust` outline, 3px offset, on every interactive element.

**Tags and status labels.** Mono, 0.68rem, `--radius-xs`, padding 1px 7px, accent text on its soft ground: `new`/`PRIMARY` rust, `SUCCESS`/`TWIN` patina, `changed`/`ALT`/info steel, `WARNING`/`PAUSED` amber, `FAILURE` ember. Neutral tags use `--sunken` with `--muted`.

**Diagnostics (callouts and alerts).** Shaped like compiler output: a mono level word, a colon, an optional title, then the body behind a 2px gutter in the level's soft color. Levels: `note` steel, `help` patina, `important` rust, `warning` amber, `caution` ember. In Markdown, write them with GitHub's alert syntax.

**Tables.** Wrapped in a `--surface` panel with a 1px `--line` border and `--radius`. Header cells are `--sunken` with `--muted` 0.78rem 650 text. Rows divide with `--line`; numeric columns align right and use mono.

**Cards.** `--surface`, 1px `--line`, `--radius`, padding 18px. A linked card gets `--line-strong` and `--shadow` on hover.

**Code blocks.** `--sunken`, 1px `--line`, `--radius`, with a header bar naming the language and a copy button. Inline code: `--sunken`, 1px `--line`, 4px radius, 0.86em.

**Navigation.** Sidebar groups sit under mono uppercase labels. Items are `--muted` and turn `--ink` on a `--sunken` hover; the current item is `--ink` at 620 weight on `--rust-soft`, with a rust bar on its left edge. In the web UI's tab bar, the selected tab is solid rust with `--on-accent` text.

**Module paths.** Each page shows its location as a Rust-style path, such as `rustyget::docs::design_system`, in 0.78rem mono `--muted`, with the current segment in `--rust`. It doubles as a breadcrumb.

**Readouts.** Live figures (speed, sizes, health) are mono. Good values are patina, attention amber, emphasis rust, and the rest muted.

## Themes

- Light is warm paper and ink; dark is cast iron with lighter, warmer accents.
- The Pages site follows `prefers-color-scheme` and lets a `data-theme="light"` or `"dark"` attribute on `<html>` override it.
- The web UI always sets `data-theme` before its styles load. `webui/oxide.css` puts dark on `:root, [data-theme="dark"]` and light on `[data-theme="light"]` after it, the same order as `tokens.css`.
- Each theme sets `color-scheme`, so native controls and scrollbars match.
- The browser `theme-color` should be `--paper`: `#f7f4ef` light, `#12100e` dark.

## Using Oxide in the web UI

The web UI's components (`ds.css`, `skin.css`) read semantic names from `webui/tokens.css`, such as `--bg-window`, `--label-primary` and `--accent`. `webui/oxide.css` keeps those names and points them at Oxide, so no component CSS has to change:

| Web UI token | Oxide |
|---|---|
| `--bg-window` | `--paper` |
| `--bg-content`, `--bg-elevated` | `--surface` |
| `--bg-glass` | `--surface` at 80% |
| `--fill-*` | `--ink` at 3 to 14% |
| `--label-primary` / `-secondary` / `-tertiary` | `--ink` / `--muted` / `--faint` |
| `--separator`, `--rule`, `--control-border` | `--line`, `--line-strong`, `--control-edge` |
| `--accent`, `--accent-fill`, `--focus-ring`, `--brand` | `--rust` |
| `--accent-soft` | `--rust-soft` |
| `--green*` | `--patina*` |
| `--yellow*` | `--amber*` |
| `--red*` | `--ember*` |
| `--cyan*` | `--steel*` |
| `--radius-lg`, `--radius-xl` | 10px |
| `--font-sans`, `--font-mono` | the Oxide stacks |
| `--shadow-glow` | none (Oxide doesn't glow) |

To switch the web UI over, add `oxide.css` after `skin.css` in both the debug and the combined stylesheet lists in `webui/index.html`, and change the two `theme-color` values in its theme script to `--paper`. Until then the file is unused, and the UI keeps its current look.

When a token is added to `docs/design-system/tokens.json`, add its mapping to `webui` in `oxide.json`; `--check` fails until it has one.

## Voice

- Follow the [Google developer documentation style guide](https://developers.google.com/style). Vale checks the site and README on each build.
- Use second person, present tense, and sentence case for headings and buttons.
- Use the project's words consistently: **fleet** for all postings of one title, **primary** for the copy being downloaded, **twin** for a byte-identical copy proven by par2 checksums, and **alt** for the same release in another encode.
- Never claim a feature the code doesn't have. Mark planned work as planned.
