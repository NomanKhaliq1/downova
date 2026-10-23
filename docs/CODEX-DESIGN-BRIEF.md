# Design brief for Codex: UI, extension popup and brand assets

You are designing the look of a lightweight Windows download manager. The engine, local API and browser extension already work. **Do not change behaviour or the API; design the screens and the brand assets.** The installer will be built after your design is delivered, using your assets.

Product name, colours and company come from `brand/brand.toml` (`name`, `primary_color`, `company`). The name is currently the placeholder `BRAND_NAME`. Never hard-code a name or a colour: read them from the brand file (the UI gets the name from `GET /api/ping` → `app`, and the colour from a CSS variable you define from one place).

## Hard rules

- **Lightweight is the product's main feature.** Plain HTML, CSS and JavaScript (ES modules are fine). No framework, no build step, no npm packages, no icon fonts. Whole `ui/` folder under **300 KB**, extension popup under **60 KB**.
- **No network at all except the local API.** No CDN, no web fonts, no analytics, no external images. Use the system font stack (Segoe UI on Windows). Inline SVG for icons.
- Works fully offline, light and dark theme (`prefers-color-scheme`, plus a manual switch stored in `localStorage`), keyboard accessible (visible focus, logical tab order, Escape closes dialogs), screen-reader labels, contrast AA, respects `prefers-reduced-motion`.
- Responsive from 360 px to 1600 px wide (it opens in a small app-style window, often 900×600).
- No `eval`, no inline event-handler attributes (the page may later get a strict Content-Security-Policy).
- Text is English now, but keep every visible string in one object so a language (for example Urdu, right-to-left) can be added later. Layout must survive RTL.
- Do not touch: `core/`, `server/`, `host/`, `extension/background.js`, `extension/lib.js`. In `extension/popup.js` keep the message protocol with `background.js` (`status`, `media`, `add`, `set_capture`, `open_ui`).

## Where things go

| What | Where |
|---|---|
| Manager window (all screens below) | `ui/` (`index.html`, `app.css`, `app.js`, optional extra modules) |
| Extension popup | `extension/popup.html`, `extension/popup.css`, `extension/popup.js` |
| Logo (square mark + wide wordmark, SVG) | `brand/assets/logo.svg`, `brand/assets/logo-wide.svg` |
| Tray icon, multi-size ICO (16, 24, 32, 48, 256) | `brand/assets/tray.ico` (plus `tray-paused.ico` if you want a paused state) |
| Application icon, multi-size ICO | `brand/assets/app.ico` |
| Extension icons PNG 16/32/48/128 | `extension/icons/` (a script, `scripts/make-icons.mjs`, makes placeholders today; replace the files, keep the names) |
| Installer wizard art | `brand/assets/wizard-large.bmp` (164×314 and a 2× version 328×628), `brand/assets/wizard-small.bmp` (55×55 and 110×110) |
| Short design notes (colours, spacing, type scale, components) | `docs/DESIGN.md` |

## How to run it with real data (without touching anyone's real files)

The app is already built. In PowerShell, from the project folder:

```powershell
$env:DM_DATA_DIR     = "$env:TEMP\dm-design\data"
$env:DM_DOWNLOAD_DIR = "$env:TEMP\dm-design\downloads"
$env:DM_UI_DIR       = (Resolve-Path ui).Path          # serves your files, edit and refresh
.\target\release\dmd.exe
$a = Get-Content "$env:DM_DATA_DIR\api.json" | ConvertFrom-Json
"http://127.0.0.1:$($a.port)/#token=$($a.token)&view=main"   # open this in Edge or Chrome
```

Stop it from the tray menu (Quit), or `.\target\release\dmhost.exe quit` with the same `DM_DATA_DIR` set. Because designing needs states that are hard to produce live (errors, empty lists, 200 tasks), also add a **mock mode**: opening `ui/index.html?mock=1` must show believable fake data and fake live progress without any server. This is also how screenshots should be made.

The complete API (endpoints, objects, events) is in **`server/API.md`**. Use only what is documented there; do not invent endpoints. A working but unstyled reference implementation of every call is in the current `ui/index.html` (token from `location.hash`, `fetch` with `Authorization: Bearer`, `EventSource` with `?token=`). Keep that behaviour.

## Screens to design (manager window)

1. **Main window.** Left: category tree (All, Downloading, Queued, Paused, Finished, Failed; then categories Compressed, Documents, Music, Video, Images, Programs, Other; then Queues). Centre: task table (file-type icon, name, size, progress bar with percent, speed, ETA, status, date added) with sorting, multi-select, context menu, drag to reorder (calls `POST /api/tasks/reorder`). Toolbar: Add, Pause, Resume, Remove, Pause all, Resume all, Open file, Open folder, Search box (`?q=`), Settings. Status bar: total speed, active count, free-text `note` of the selected task.
2. **Add download dialog.** Tabs: *Single link*, *Many links* (textarea; show the live preview of how patterns like `file[001-020].jpg`, `[1-10:2]`, `[a-e]` expand, and a count), *From file* (import a text file of links). Fields: save-to folder, category (auto), queue, connections slider (1–32), speed limit, referer, cookie, user agent, checksum, "start now / add paused", and **quality cap** for streams (Best, 1080, 720, 480, 360) and **file name**. Show duplicates and invalid lines from the response.
3. **Download details.** Opens on double-click. Big progress, speed graph (last 60 s, drawn with inline SVG or canvas, no library), segment map, URL, saved path, headers used, error text, `note`, buttons Pause/Resume/Open/Remove.
4. **Queues and scheduler.** List of queues with max simultaneous downloads; editor for schedule (days as seven toggles = bit mask, start/stop time pickers, overnight windows), and "when finished": Nothing, Exit app, Sleep, Hibernate, Shutdown (warn that Windows gives 60 s to cancel), Run command.
5. **Settings** (tabs): General (download root, start with Windows, clipboard offer), Connections (default connections, max simultaneous, global speed limit, proxy), Folders and categories (edit extensions and folders, site rules), Sites and file types the browser extension must leave alone (exclusions), Streams (ffmpeg path, explain what it is needed for), Advanced (user agent, on-complete command with `{path}`), About (name, version from `/api/ping`, links).
6. **History.** Finished and failed downloads with search and filters, bulk remove, "remove and delete files", export links (`GET /api/export`).
7. **Grabber results.** A list of found links/images (same component as the extension popup's page-link grabber): checkboxes, filter box (words or `/regex/`), select all/none, count, "download selected".
8. **Extension popup** (380 px wide): connection status line (Connected / app not running / integration not installed / update needed), the "Catch browser downloads" switch, **found on this page** list (kind badge video/audio/hls/dash, name, size, Download button), page link/image grabber with filter, "Open manager" button. Keep the element ids that `popup.js` uses or update `popup.js` accordingly.

Also design these small but important pieces: empty states (no downloads yet, no results), loading skeletons, error banner when the app cannot be reached or the token is missing ("Open this window from the tray icon"), toast for finished downloads, the **clipboard offer** banner (`clipboard` event: "Download this link? Yes / No / Never"), a first-run welcome that shows three steps (install the browser extension, set the download folder, try a link), confirmation dialogs (remove, remove with files, shutdown action), and a compact "mini mode" list for narrow windows.

## Brand assets

- A simple, distinctive mark that still reads at 16×16 (a down-arrow into a tray is the current placeholder; replace it with the real identity). Provide it in the brand colour on transparent, plus a white version for dark backgrounds.
- Tray icon states: idle, active (downloading), paused (optional).
- Installer wizard images in the same style (Inno Setup `WizardImageFile` / `WizardSmallImageFile`, 24-bit BMP).
- A short style guide in `docs/DESIGN.md`: palette tokens (light and dark), type scale, spacing, radius, elevation, component list.

## Acceptance checklist (I will verify these after delivery)

- [ ] Every API call in the old `ui/index.html` still works (add, pause, resume, remove, open, settings, queues, events, clipboard offer).
- [ ] `?mock=1` shows every screen and state listed above.
- [ ] Total `ui/` size < 300 KB, extension popup < 60 KB; no request leaves `127.0.0.1` (checked in the browser network log).
- [ ] Light and dark themes both complete; keyboard-only use works; no console errors.
- [ ] Looks right at 360, 900 and 1600 px width, and mirrored (RTL).
- [ ] Brand name and colour come only from the brand file / `ping`.
- [ ] Icons and images exist at the paths in the table above, at the stated sizes.
