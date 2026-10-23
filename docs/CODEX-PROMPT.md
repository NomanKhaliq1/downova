You are the designer and front-end developer for a Windows download manager. The project is in `C:\Users\PMLS\Desktop\noman\download` (a git repository). The engine, local API and browser extension are finished and tested. Your job is the **look and feel**: the manager window, the browser-extension popup, and the brand assets (logo, icons, installer art). Do not change behaviour.

Start by reading, in this order:
1. `docs/CODEX-DESIGN-BRIEF.md` (your full brief: rules, every screen, asset list, how to run, acceptance checklist)
2. `server/API.md` (the only API you may use; do not invent endpoints)
3. `ui/index.html` (a working but unstyled reference of every API call; keep its behaviour)
4. `extension/popup.html` and `extension/popup.js` (keep the message protocol with `background.js`)
5. `brand/brand.toml` (name, colour and company come from here; the name is a placeholder, never hard-code it)

Work like this:
- Create a git branch `design` and commit in small steps. Do not push anything and do not touch `core/`, `server/`, `host/`, `extension/background.js` or `extension/lib.js`.
- Plain HTML, CSS and JavaScript only. No framework, no build step, no npm packages, no CDN, no web fonts, no external requests. `ui/` must stay under 300 KB and the extension popup under 60 KB. Light and dark theme, keyboard accessible, AA contrast, responsive from 360 to 1600 px, ready for right-to-left languages.
- Add `?mock=1` to `ui/index.html` so every screen and state (empty, loading, error, 200 tasks, live progress, clipboard offer, first-run welcome) can be previewed with fake data and no server.
- To test with real data, run the app with temporary folders exactly as described in the brief (set `DM_DATA_DIR`, `DM_DOWNLOAD_DIR`, `DM_UI_DIR`, start `target\release\dmd.exe`, open the URL with the token from `api.json`). Never use the real `%APPDATA%` or Downloads folders.
- Put assets where the brief's table says: `brand/assets/` (logo, tray and app icons as multi-size ICO, installer wizard images as BMP), `extension/icons/` (PNG 16/32/48/128, keep the file names), and write `docs/DESIGN.md` (palette tokens for both themes, type scale, spacing, components).
- The mark must still read clearly at 16×16. Design it as a simple, distinctive identity, not a generic download arrow, and give a white version for dark backgrounds.

Before you finish, check every item of the acceptance checklist at the end of the brief yourself, take a screenshot of each screen in light and dark at 360, 900 and 1600 px (save them in `docs/screenshots/`), and confirm in the browser network log that nothing leaves `127.0.0.1`.

When done, reply with: the branch name and last commit, the list of files you added or changed, anything from the brief you did not do and why, and any behaviour you think the API or extension should support to make the design better (as suggestions only; do not implement them).
