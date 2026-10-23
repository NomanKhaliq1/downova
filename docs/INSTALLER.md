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
- **Browser extension.** Chrome and Edge only enable extensions from their stores on normal Windows PCs. Publish `dist\extension` to the Chrome Web Store (and Edge Add-ons), then put the page address into `store_url` in `brand\brand.toml`: the installer's last page then opens it. Until then the installer offers to open the extension folder for "Load unpacked". The store assigns its own extension ID: add it to `extension_ids` in `brand\brand.toml` and rebuild.
- **Updates.** The app does not check for updates yet; users install a newer setup over the old one.
