# IDM audit (this laptop, 2026-10-23)

Internet Download Manager 6.42.37 by Tonec Inc. Read-only inspection: nothing was changed or removed.

## 1. Where it lives

| What | Where |
|---|---|
| Program | `C:\Program Files (x86)\Internet Download Manager\` (186 files, 24 MB) |
| User data | `%APPDATA%\IDM\` (27 files, 1 MB): exclusion lists, folder history, scheduler queue, a dead Firefox add-on |
| Settings | `HKCU\SOFTWARE\DownloadManager` (about 70 values) and `HKLM\...\WOW6432Node\Internet Download Manager` |
| Autostart | `HKCU\...\Run\IDMan` = `IDMan.exe /onboot`, tray icon on |
| Process | `IDMan.exe` (running now). No Windows service, no scheduled task, no firewall rule |

## 2. What the pieces do

| Piece | Files | Job |
|---|---|---|
| Main app | `IDMan.exe` (5.9 MB) | Queue, segmented downloader, UI, scheduler, tray |
| Network drivers | `idmtdi32/64.sys` (TDI, 2020 build), `idmwfp32/64/AA.sys` (WFP, 2023) | Kernel-level traffic capture so downloads from any browser or app are caught. Enabled (`EnableDriver=1`) |
| Chrome/Edge extension | "IDM Integration Module", MV3, in 5 Chrome profiles | Catches downloads and media in the page, sends them to IDMan |
| Native messaging host | `IDMMsgHost.exe` + `IDMMsgHost.json`, name `com.tonec.idm`, registered for Chrome, Edge, Firefox | The pipe between the extension and IDMan.exe |
| Click catchers / IE helpers | `idmcchandler*.dll`, `IDMIECC*.dll`, `IEMonitor.exe`, `idmmzcc7*.dll`, `idmmzcc*.xpi` | Old hooks for Internet Explorer and legacy Firefox |
| Video grabber | `IDMVMPrs*.dll`, `IDMGrHlp.exe`, `idmvconv.dll`, `IDMGetAll*.dll`, `idmindex.dll` | Parses pages for video links, converts, "download all links" |
| Shell/IE menu | `IDMShellExt*.dll`, `downlWithIDM*.dll`, `idmbrbtn*.dll` | Right-click and "Download panel" |
| Helpers | `idmBroker.exe`, `IDMIntegrator64.exe`, `MediumILStart.exe`, `idmfsa.dll`, `idmnmcl.dll`, `IDMNetMon*.dll` | Settings broker, integration loader, privilege bridge, network monitor |
| Crypto | `libcrypto.dll`, `libssl.dll` = OpenSSL 1.1.1g | HTTPS. Version is from 2020 and end-of-life |
| Data | `defextmap.dat`, `urlexclist.dat`, `IDMFType.dat`, `defexclist.txt` | File-type map and site exclusion lists |

## 3. Browser extension (Chrome)

- ID `ngpampappnmepgilojfohadhhmbhlaek`. Manifest V3, service worker `background.js` (55 KB, minified).
- Permissions: `<all_urls>`, `cookies`, `tabs`, `webRequest`, `webNavigation`, `declarativeNetRequest`, `downloads`, `downloads.shelf`, `downloads.ui`, `management`, `proxy`, `nativeMessaging`, `scripting`, `storage`, `contextMenus`.
- A content script (`content.js`) runs on every page, in every frame, at `document_start`.
- Mechanism seen in the code: it listens to `webRequest` (headers, started, completed, error, redirect), cancels/resumes Chrome's own downloads with `downloads.cancel/resume`, reads cookies with `cookies.getAll`, then talks to `IDMMsgHost.exe` through native messaging.
- Only `internetdownloadmanager.com` and `tonec.com` can message it externally.

## 4. Findings (likely sources of the problems)

1. **Version mess across Chrome profiles.** Default, Profile 1, Profile 8 have 6.43.1. Profile 4 has 6.42.64. **Profile 10 has 6.42.18.3, an old Manifest V2 build using `webRequestBlocking`, which current Chrome no longer supports.** That profile's integration is very likely broken or disabled.
2. **Kernel drivers.** Two driver families (old TDI plus WFP) sit in the network path. They are the main cause of connection hiccups and blue screens, and they can clash with VPN or security software. Cisco Secure Client is still installed here, and it also hooks the network stack. I could not confirm a conflict, only that the setup makes one possible.
3. **Dead legacy code still installed.** Internet Explorer helpers, a legacy Firefox add-on folder (`%APPDATA%\IDM\idmmzcc5`), and a TDI driver from the Windows 7 era. None of it is used by modern Chrome, but it still loads or registers.
4. **Outdated OpenSSL 1.1.1g** (2020, end of life) inside a program that handles your cookies and downloads.
5. **Broad extension power.** It sees every page and every cookie in 5 profiles. That is normal for a download manager, but it is a big surface for something you cannot read the source of.
6. **Native host registered in HKLM, HKCU and Wow6432**, plus Edge and Firefox. Harmless, but it is why uninstalling by hand leaves traces.

## 5. Not checked

- Licence or registration state (deliberately not opened).
- Real download history and queue contents.
- Windows Event Log crashes or driver faults for IDM.
- A line-by-line read of `background.js` (it is minified).

## 6. Design for your own manager

IDM's real value is four things: capture, segmented download, resume, queue. Everything else is optional.

**Recommended small version (works in about 1 to 2 days):**

1. **Engine**: reuse an existing one. `aria2c` does multi-connection segments, resume, retries, speed limits and a JSON-RPC API. Add `yt-dlp` (and ffmpeg) for video and HLS/DASH.
2. **Chrome MV3 extension** (yours, minimal permissions):
   - `chrome.downloads.onCreated`: cancel Chrome's download and hand the URL to the engine.
   - `webRequest.onHeadersReceived`: spot media by `Content-Type` (`video/*`, `audio/*`) and `.m3u8` / `.mpd`.
   - Send cookies, `Referer` and `User-Agent` with each job, because many sites reject downloads without them.
   - Right-click menu "Download with X".
3. **Link to the engine**: either a small localhost server on `127.0.0.1` with a random token, or native messaging (extension ID pinned in `allowed_origins`). Native messaging is safer.
4. **UI**: start with AriaNg (web UI for aria2) or a tray app that shows the queue.
5. **No kernel driver.** You lose catching downloads from non-browser apps, and you gain stability.

**If you write the engine yourself** (Go, Rust or .NET): probe with `HEAD` / `Range: bytes=0-0` (check `Accept-Ranges`, `Content-Length`, `ETag`), split into 8 to 16 segments, preallocate the file, write each segment at its offset, save segment progress to a small JSON file for resume, use `If-Range` to detect a changed file, retry with backoff, and re-split the slowest segment when a fast one finishes.

**Pitfalls to design for early:** servers without Range support, expiring signed URLs, cookie/Referer checks, redirects, filename from `Content-Disposition`, very large files and disk space, antivirus locking `.part` files.
