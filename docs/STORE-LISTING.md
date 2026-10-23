# Chrome Web Store / Edge Add-ons listing (draft)

Upload `dist\store\downova-extension-<version>.zip` (made by `scripts\package-store.ps1`; the store assigns the extension ID).
Chrome Web Store: one-time developer registration fee, review usually takes a few days. Use **Unlisted** visibility if the extension
should only be reachable from the installer's link. After approval:

1. Copy the store's extension ID into `extension_ids` in `brand\brand.toml` (keep the old one for developer builds) and the store page address into `store_url`.
2. Rebuild the installer (`scripts\build-installer.ps1`): its last page then opens the store page, where Chrome itself asks "Add Downova?".
3. Optional, administrators only: Chrome can also be told to offer a store extension to every user through the registry (`HKLM\Software\Google\Chrome\Extensions\<id>` with an `update_url` of the Web Store). On Windows this only works for Web Store extensions, so it needs steps 1 and 2 first.

## Name, summary, category
- Name: Downova
- Summary (132 characters max): Send downloads and videos from your browser to the Downova app: fast, resumable, and private on your PC.
- Category: Productivity

## Description
Downova catches your downloads and hands them to the Downova app, which downloads them faster (several connections), resumes them if your connection drops and files them into folders.
A small download button appears on videos you play, and the extension lists the video and audio files it finds on the page. It can also collect all links or images of a page for you to pick from.
Everything stays on your computer: the extension only talks to the Downova app installed on the same PC.
Needs the Downova app (Windows).

## Single purpose
Hand files and media from the browser to the locally installed Downova download manager.

## Permission justifications (the review asks for each)
| Permission | Why |
|---|---|
| `downloads` | Notice a download the browser started, cancel the browser's copy once the Downova app has taken it over, and keep the browser's list tidy. |
| `nativeMessaging` | Talk to the Downova app on the same computer. Nothing is sent over the internet. |
| `cookies` | Send the page's login cookies to the app for that one download, so files behind a login can be fetched. Used only for the link being downloaded. |
| `webRequest` (read-only, no blocking) | See the type and size of media files a page loads, to offer them in the download button and the popup. |
| `contextMenus` | "Download link / video / get all links and images" in the right-click menu. |
| `scripting` | Collect the links or images of the current page when the user presses "Links" or "Images" in the popup. |
| `storage` | Remember two switches (catch downloads, show the button on videos) and the media found in the current tab. |
| Host access `<all_urls>` | Downloads and videos can be on any website; the button and the media list must work everywhere. |
| Content script on all pages | Shows the download button over videos. It reads nothing from the page except where the video elements are. |

## Privacy policy (publish at a stable address and enter it in the store form)
- The extension does not collect, store or transmit personal data to the developer or any third party.
- Links, cookies and headers of a download are sent only to the Downova application on the user's own computer (through Chrome's native messaging), only for the download the user starts or the browser performs.
- Settings (two switches) and the list of media found in the open tab are kept in the browser's local extension storage.
- Removing the extension or the app removes this data.

## Single-purpose note for yt-dlp
The extension itself never downloads from YouTube-style sites. When the user presses the button on such a page, the page link is passed to the Downova app, which uses the separate open-source tool yt-dlp only if the user has set it up in the app's settings. Mention this honestly in the listing if you offer that feature, and only save videos you are allowed to download.
