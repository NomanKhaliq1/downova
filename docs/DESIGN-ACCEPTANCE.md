# Design acceptance Â· 24 October 2026

This document records checks against `docs/CODEX-DESIGN-BRIEF.md`. The engine, API, native host and extension background/library are unchanged. Final evidence: 204 screenshots, 70,744 UI bytes, 23,811 popup bytes including shared modules and loaded icons, 3,575 loopback-only browser requests, and zero console errors.

- [x] Reference UI operations retained: token from the hash is stored in sessionStorage; Bearer-authenticated fetch; token-authenticated EventSource; add, pause, resume/retry, remove without files, finished-file/folder controls, pause all/resume all, settings/clipboard monitoring, task/progress/clipboard events.
- [x] New presentation covers every manager screen, all seven settings tabs, all three add tabs, and the popup. Only documented API endpoints and existing extension messages are used.
- [x] Mock mode supports empty, loading, errors, 200 tasks, changing progress, clipboard offer, welcome and completion toast. Removal and shutdown confirmations are available through the normal controls. Search provides the no-results state. Mock mode also loads directly from disk with no server.
- [x] UI below 300 KB and all popup resources below 60 KB; exact byte totals are recorded in the browser log. No framework, npm runtime package, CDN, web font, analytics or external resource.
- [x] Light/dark/system theme; stored manual override; reduced-motion handling; labels, textual statuses, progress names, visible focus, Escape, multi-select, native forms, dialog tab wrapping, keyboard details/context/reorder actions. The browser test exercises keyboard entry, focus containment and dismissal.
- [x] Body has no horizontal overflow in the screenshot matrix at 360, 900 and 1600 px. Task lists scroll within the window; narrow cards retain progress, speed, ETA and status. RTL previews are included at all three widths in both themes.
- [x] No JavaScript exceptions during the browser pass. Browser console errors and outbound page requests are asserted by the verification script. The network evidence records page requests to 127.0.0.1 only.
- [x] Name/version from ping in the live manager; popup name from existing status protocol; generated identity values and assets from `brand/brand.toml`. Shared UI strings are centralized. No new product name is invented.
- [x] Logo mark, wide logo, white variants, multi-size app/tray/active/paused ICOs, four extension PNGs, and four correctly-sized 24-bit BMPs. The browser check validates image header dimensions and icon entries.
- [x] Real engine launched with the brief's three environment variables and temporary folders. Add paused, resume, real task/progress events, pause/resume all, saved settings, queue creation/deletion, export, removal retaining a file and removal deleting a file passed. The engine was shut down afterwards.
- [x] Existing eleven extension-library tests pass. Popup protocol tests verify the exact media-add payload (including referer and filename), batch-add payload, status/media requests, capture switch and open-manager message using a test Chrome API shim.

## Explicit limits

The design uses aggregate bytes instead of invented segment progress. Category override, page scraping in the manager, extension installation/status, and effective request headers are absent from the API; the interface explains those limits. See `DESIGN.md` for suggested future capabilities.

The real-engine check intercepts file/folder opening after verifying the documented payload. It does not launch Explorer or a file association. Native-messaging installation was not reconfigured: the existing protocol is exercised with a test shim, while actual app/API behaviour is exercised against dmd. No real network download, Windows shutdown/sleep/hibernate/exit-on-queue-drain, user-supplied command, or browser installation is triggered by the checks.

Automated keyboard checks, measured AA text/control contrast pairs, and visual review are included. A human Windows screen-reader session, touch hardware and multiple monitor scaling configurations were not available in this pass. New brand colours need their own contrast review. These are validation limits, not claims of tests performed.
