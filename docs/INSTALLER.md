# Building and testing the installer

```powershell
powershell -File scripts\build-installer.ps1        # cargo release build + extension + installer -> dist\downova-setup-<version>.exe
powershell -File scripts\build-installer.ps1 -SkipBuild   # reuse target\release
powershell -File scripts\test-installer.ps1         # install, upgrade over a running app, uninstall; checks nothing is left
```

Needs Inno Setup 6 (https://jrsoftware.org/isinfo.php). Everything about the product (name, id, company, GUID, store URL) comes from `brand\brand.toml`;
the version comes from `host\Cargo.toml` (the extension and the browser helper must carry the same version).

## What the installer does

- Per-user install into `%LOCALAPPDATA%\Programs\Downova`: no administrator rights, no UAC prompt.
- Copies `dmd.exe` (the app), `dmhost.exe` (browser helper), `ui\` and `extension\`.
- Registers the browser helper for Chrome, Edge and Brave (current user only) by running `dmhost install`.
- Optional: desktop shortcut, start with Windows (a quiet tray start, `--background`).
- Upgrade: asks a running app to save and exit (`dmhost quit`), then replaces the files. Same GUID, so Windows treats it as the same product.
- Uninstall: closes the app, removes the browser helper registration, shortcuts and files. The download list and settings (`%APPDATA%\downova`) are kept unless the user says yes to the question (a silent uninstall always keeps them). Downloaded files are never touched.

## Things that need a decision or an account

- **Never change `app_guid` or `product_id` after the first release** (upgrades, uninstall, browser helper name and the data folder depend on them).
- **Code signing.** The installer and exe files are not signed, so Windows SmartScreen will warn ("unknown publisher") until they are. Buy an Authenticode certificate, configure it in Inno Setup (Tools > Configure Sign Tools) and enable the `SignTool` lines in `installer\setup.iss`.
- **Browser extension.** On normal Windows PCs Chrome and Edge only enable extensions that come from their stores; Chrome's registry route (`HKLM\...\Extensions\<id>`) accepts only a Web Store `update_url`, a local .crx path is refused since Chrome 33, and the `--load-extension` flag is ignored by current Chrome. So "Chrome asks to add it" needs a store listing: run `scripts\package-store.ps1`, upload the zip (see `docs\STORE-LISTING.md`), then put the store page address into `store_url` and the store's extension ID into `extension_ids` in `brand\brand.toml`, and rebuild. Until then the installer's last page offers two steps: open Chrome's Extensions page, and show the extension folder for "Load unpacked".
- **Browser helper self-repair.** The app re-registers the browser helper at every start if its registry entry is missing (something may remove it), and the installer checks it at the end and tries once more.
- **Updates.** The app does not check for updates yet; users install a newer setup over the old one.

## YouTube-style sites (yt-dlp)

The floating button on a YouTube, Vimeo, X, TikTok... page hands the page to the helper tool yt-dlp. The tool is not bundled: Settings > Streams > "Set up yt-dlp" downloads the latest release from github.com/yt-dlp (checksum verified) into `%APPDATA%\downova\tools`. Without ffmpeg only ready-made single files are available (often 360p or 720p); set an ffmpeg path in the same tab for the best quality. YouTube changes often: use "Update yt-dlp" when downloads stop working.
