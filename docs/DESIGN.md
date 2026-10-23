# Download manager design

The Downova identity is a split, faceted D: a stable vertical spine and an angular forward-moving loop. Six-unit strokes and the open counter remain clear at 16 px. The manager uses a neutral work surface, restrained brand colour, vector file-type icons and textual status badges. The transfer table is the main surface, with a compact navigation tree and toolbar. Narrow windows keep the same semantic table and scroll it horizontally. Navigation, table and secondary screens scroll independently.

## Identity and regeneration

`brand/brand.toml` is the source for the name, company, homepage and primary colour. Run `node scripts/make-icons.mjs` after changing it. This development utility uses only Node's standard library. It writes the SVGs, multi-size ICOs, 24-bit BMPs, extension PNGs, manager `brand.js` / `brand.css`, popup `popup-brand.css`, the popup's copy of the translation object and vector icons, and the extension's display-name configuration. There is no application build step. The display name comes from the generated brand configuration in both live and mock modes. `/api/ping` currently returns the internal product ID as `app`; its version is used unchanged. The popup uses the existing background status response for its identity.

`logo.svg` is the transparent square mark. `logo-wide.svg` pairs it with a system-font wordmark. `logo-white.svg` and `logo-wide-white.svg` are for dark backgrounds. The app icon is the white mark on a blue rounded square. Tray variants are transparent idle, filled active, and paused. Every ICO contains 16, 24, 32, 48 and 256 px images. Extension names remain `16.png`, `32.png`, `48.png`, `128.png`. Installer artwork is supplied at 164×314 / 328×628 and 55×55 / 110×110 in uncompressed 24-bit BMP format; high-density files use the `@2x` suffix.

## Palette

| Token | Light | Dark |
| --- | --- | --- |
| `--brand` | Generated `primary_color` | Generated `primary_color` |
| `--bg` | `#f5f6f8` | `#15191f` |
| `--panel` | `#ffffff` | `#1c222b` |
| `--text` | `#242b36` | `#edf1f7` |
| `--muted` | `#5d6878` | `#aab6c8` |
| `--line` | `#dfe4eb` | `#353f4d` |
| `--control` | `#8390a3` | `#75849a` |
| `--soft` | `#f3f5f8` | `#252d39` |
| `--accent` | Brand colour | 45% brand mixed with white |
| `--good` | `#137349` | `#78d9ae` |
| `--bad` | `#b32539` | `#ff9cac` |

Primary buttons use white on the brand colour (5.17:1). Manager controls use neutral greys. Brand colour is reserved for progress, selection, primary actions and keyboard focus; neutral file icons and plain status labels avoid decorative colour. The browser check measures text, secondary text, accent, success, error and control-boundary pairs in both themes. A new primary colour should be contrast-checked before regenerating assets. Status always has a text label as well as colour. Inactive controls are visibly disabled. Theme follows the operating system until the user chooses Light or Dark; the choice is stored in localStorage. Manager and extension have independent localStorage origins.

## Typography, spacing and surfaces

- System stack: Segoe UI, system-ui, sans-serif. No downloaded fonts.
- Type: 18 px page title, 16 px section title, 13 px base/file names and 12 px forms/table metrics, 10–11 px compact metadata. Data rows use regular weight; headings use 600. Desktop rows are 36 px high.
- Spacing: 4 px base, 4–8 px toolbar/table padding and 10–14 px form gutters. The table fills the remaining window height; there are no summary cards above it.
- Radius: 6 px fields/buttons, 8 px transfer surface, 10 px dialogs. Subtle horizontal dividers replace vertical table grid lines.
- Elevation: flat work surfaces and 1 px dividers. Only dialogs, menus and toasts have a small 3 px / 12 px shadow; the backdrop has no blur.

## Components and interaction

The manager includes navigation groups, compact toolbar actions with SVG icons and labels, sortable/selectable task rows, progress meters, neutral file-type icons, plain status labels, request-option forms, tab groups, queue cards, seven-day schedule toggles, settings sections, link-review rows, a progress graph, byte map, clipboard offer, empty/loading/error states, confirmations and live-region toasts.

Select multiple tasks with checkboxes or Ctrl-click. Double-click or focus a row and press Enter for details. Shift+F10 opens contextual actions. Drag rows to set priority; Alt+Up / Alt+Down does the same using the keyboard. Reordering returns to manual order if the table was sorted. Escape dismisses dialogs and contextual actions. Dialog headers and actions stay visible while long forms scroll. Dialogs trap Tab / Shift+Tab and Add focuses the link field. Native labels and controls supply accessible names; focused controls have a 2 px outline. Progress updates change values without rebuilding the task list. Settings forms are not redrawn for background task events.

The popup retains its existing compact browser layout and palette; the table above describes the manager palette. It retains `status`, `media`, `add`, `set_capture` and `open_ui` messages, page link/image collection, filtering, selection, and the right-click-menu handoff. It adds presentation-only theme selection and a preview mode. The popup preview does not require a browser extension host.

All UI copy is in `ui/strings.js` (`S`, including `S.popup`). Generated `extension/popup-strings.js` lets the packaged popup remain independent. Engine-supplied names, categories, errors, notes, queue names, paths and URLs remain data. To localize, translate the object and set the document's `lang` and `dir`. Spacing and alignment use logical properties; numeric metrics and path inputs are isolated left-to-right; graph time keeps its normal order.

## Offline previews

Open `ui/index.html?mock=1` directly, or serve it locally. No engine or API connection is created in mock mode. The preview strip selects live progress, empty, loading, connection error, 200 tasks, clipboard offer, welcome, and completion toast. Its screen selector opens Add, Details, Queues, Settings, History, Link grabber and Welcome. All mock actions work against in-memory data and reset on reload.

Examples:

```text
ui/index.html?mock=1&screen=details
ui/index.html?mock=1&screen=settings&tab=streams
ui/index.html?mock=1&state=many
ui/index.html?mock=1&dir=rtl
extension/popup.html?mock=1
```

The add dialog accepts the original pattern text. Its live preview is bounded to 500 expansions to avoid blocking the browser; the engine remains responsible for full validation and its own 10,000-link limit. HTTP sources shown in mock data are text, never fetched. Text-file import is read locally. About presents the homepage with a copy control so no outbound navigation is needed.

## API boundaries and suggestions

The documented API has no category override on add. The category field therefore explains automatic classification rather than submitting an invented property. The folder remains editable. It also has no segment-level data: Details explicitly labels its map as aggregate downloaded bytes. Headers shown are the request options reported by the API; cookies are masked. These are intentional, documented limits.

The manager reviews pasted links; page scraping stays in the extension because the API has no grabber endpoint. The welcome explains installation of the supplied extension; there is no extension-store installation endpoint to call.

Suggestions only: expose segment offsets and per-segment states; expose effective/redacted request headers; optionally accept a category override; let the extension hand a collected selection to a manager review screen; expose extension-installation status for the welcome. None of these API or protocol changes is implemented.

## Verification

Run `node scripts/design-check.mjs` for preview interaction, keyboard, assets, sizes, popup protocol, file-URL support, network logging and screenshot coverage. Run `node scripts/design-real-check.mjs` for the real engine using only `%TEMP%\dm-design\data`, `%TEMP%\dm-design\downloads`, and the repository `ui/` through `DM_DATA_DIR`, `DM_DOWNLOAD_DIR` and `DM_UI_DIR`. The script launches `target\release\dmd.exe`, reads that instance's `api.json`, opens the token-bearing URL and shuts it down through `/api/quit`. It never changes APPDATA or the user's Downloads. Development checks use the bundled Playwright; `DM_PLAYWRIGHT` can override its location. No dependency is shipped with the UI.

Evidence is in `docs/screenshots/`, `docs/design-browser-log.json`, `docs/design-real-log.json` and `docs/DESIGN-ACCEPTANCE.md`. Request logs omit the real token. Browser requests are inspected from Playwright's network events, equivalent to the page Network log. They include only 127.0.0.1 requests. The file-URL smoke check is separate and loads only local files.

## Downova rollout

All interface icons are local SVG paths (`ui/icons.js`, generated `extension/popup-icons.js`), including file categories, toolbar actions, sorting and dialog close. File extensions remain part of file names rather than standing in for icons. The display name is generated from the brand file. The existing technical product ID and native-host address are retained so installed integration and storage locations remain compatible. Engine and host embed the brand file at compile time; rebuild release binaries after a brand change.

The supplied tray/app ICOs are ready for native packaging. The current dmd tray implementation draws an arrow procedurally and does not load `tray_icon` from the brand file. That wiring is inside protected `server/` and remains pending; rebuilding applies the Downova tray tooltip/menu name but does not replace its procedural icon. No installer executable was built.

## Desktop density pass

The main window follows an Explorer/mail-client layout: a 176–188 px navigation tree, a compact connection/theme strip, a compact view title with search, an icon-and-label command toolbar, one data table, and a bottom status bar. Transfer speed stays in the existing status bar. Main/history tables keep real column headers and native horizontal scrolling at narrow widths instead of becoming cards. Secondary date/category columns are hidden below 1100 px; name, size, progress, speed, ETA and status remain in the table. No API, event, selection, sorting or download action changed. The browser regression asserts row height, table viewport height, the absence of summary cards, and semantic table layout at 360 px.

## Modern desktop surfaces and icon policy

The table remains the main element, without summary cards. Slightly larger file names, a compact single-row command bar at 900 px, inset work surface, softer control corners and neutral slate backgrounds give the manager a modern desktop appearance. Search sits beside the view title. There are no vertical spreadsheet grid lines or heavy card shadows.

Every application pictogram is an SVG path or SVG geometric shape. Dialog close and search clear are explicit accessible SVG buttons, never X/Unicode character substitutions. The search clear button sends the existing input event, preserving filtering and API queries. Browser-native search cancel glyphs are hidden. Select chevrons, disclosure arrows, checked/indeterminate boxes and the popup capture switch use embedded local SVG. Cancel remains a text label without a decorative cross. The regression checks assert empty text inside both close buttons, SVG path presence and search clearing, and reject icon glyph substitutions in manager/popup preview text.
