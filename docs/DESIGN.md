# Download manager design

The identity is a pair of offset gates: two independent transfers becoming one orderly workspace. The open centre and five-unit strokes survive at 16 px. The manager uses a neutral work surface, restrained brand colour, file-type tiles and textual status badges. At narrow widths, the table becomes a compact list. The sidebar and content scroll independently in desktop windows.

## Identity and regeneration

`brand/brand.toml` is the source for the name, company, homepage and primary colour. Run `node scripts/make-icons.mjs` after changing it. This development utility uses only Node's standard library. It writes the SVGs, multi-size ICOs, 24-bit BMPs, extension PNGs, manager `brand.js` / `brand.css`, popup `popup-brand.css`, and the popup's copy of the translation object. There is no application build step. The live manager's displayed name and version always come from `/api/ping`; mock mode uses the generated brand configuration. The popup uses the existing background status response for its identity.

`logo.svg` is the transparent square mark. `logo-wide.svg` pairs it with a system-font wordmark. `logo-white.svg` and `logo-wide-white.svg` are for dark backgrounds. The app icon is the white mark on a blue rounded square. Tray variants are transparent idle, filled active, and paused. Every ICO contains 16, 24, 32, 48 and 256 px images. Extension names remain `16.png`, `32.png`, `48.png`, `128.png`. Installer artwork is supplied at 164×314 / 328×628 and 55×55 / 110×110 in uncompressed 24-bit BMP format; high-density files use the `@2x` suffix.

## Palette

| Token | Light | Dark |
| --- | --- | --- |
| `--brand` | Generated `primary_color` | Generated `primary_color` |
| `--bg` | `#f5f7fb` | `#10151e` |
| `--panel` | `#ffffff` | `#181f2c` |
| `--text` | `#182235` | `#ecf1fc` |
| `--muted` | `#58677d` | `#a3b1c7` |
| `--line` | `#dce2ec` | `#354156` |
| `--control` | `#7b879c` | `#697b96` |
| `--soft` | `#edf2fc` | `#26334b` |
| `--accent` | Brand colour | 45% brand mixed with white |
| `--good` | `#137349` | `#78d9ae` |
| `--bad` | `#b32539` | `#ff9cac` |

Primary buttons use white on the brand colour (5.17:1). Brand text on the light selection surface is 4.60:1; input boundaries are at least 3.62:1. The browser check measures text, secondary text, accent, success, error and control-boundary pairs in both themes. A new primary colour should be contrast-checked before regenerating assets. Status always has a text label as well as colour. Inactive controls are visibly disabled. Theme follows the operating system until the user chooses Light or Dark; the choice is stored in localStorage. Manager and extension have independent localStorage origins.

## Typography, spacing and surfaces

- System stack: Segoe UI, system-ui, sans-serif. No downloaded fonts.
- Type: 26 px page title (23 px narrow), 20 px section title, 14 px base, 12 px form/table text, 10–11 px compact metadata. Weight 600–700 marks hierarchy; tabular data stays compact.
- Spacing: 4 px base, with 8 / 12 / 16 / 24 / 32 px steps. Desktop gutters are 28 px, wide gutters 40 px, narrow gutters 14 px.
- Radius: 6 px fields, 7 px controls, 10 px panels, 14 px dialogs.
- Elevation: flat panels with a 1 px boundary; shadow only for dialogs, contextual actions and toasts. The dialog backdrop softens the underlying view.

## Components and interaction

The manager includes navigation groups, transfer summary tiles, sortable/selectable task rows, progress meters, file-type tiles, status badges, responsive task cards, request-option forms, tab groups, queue cards, seven-day schedule toggles, settings sections, link-review rows, a progress graph, byte map, clipboard offer, empty/loading/error states, confirmations and live-region toasts.

Select multiple tasks with checkboxes or Ctrl-click. Double-click or focus a row and press Enter for details. Shift+F10 opens contextual actions. Drag rows to set priority; Alt+Up / Alt+Down does the same using the keyboard. Reordering returns to manual order if the table was sorted. Escape dismisses dialogs and contextual actions. Dialog headers and actions stay visible while long forms scroll. Dialogs trap Tab / Shift+Tab and Add focuses the link field. Native labels and controls supply accessible names; focused controls have a 3 px outline. Progress updates change values without rebuilding the task list. Settings forms are not redrawn for background task events.

The popup shares the tokens and file-list language. It retains `status`, `media`, `add`, `set_capture` and `open_ui` messages, page link/image collection, filtering, selection, and the right-click-menu handoff. It adds presentation-only theme selection and a preview mode. The popup preview does not require a browser extension host.

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
