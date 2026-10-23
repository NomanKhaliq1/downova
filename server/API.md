# Local API (the contract the UI and the browser extension are built against)

The background app (`dmd`) serves everything on `http://127.0.0.1:<port>`. The port changes on every start.
Both the port and the access token are written to `%APPDATA%\<product_id>\api.json`:

```json
{ "port": 55863, "token": "7ee7…", "pid": 1234 }
```

The tray menu opens the UI as `http://127.0.0.1:<port>/#token=<token>&view=main` (`view=add` focuses the add box).
The page must read the token from `location.hash` and keep it in `sessionStorage`.

## Rules

- Only `127.0.0.1:<port>` and `localhost:<port>` are accepted in the `Host` header (anything else gets 403).
- Every `/api/*` call except `GET /api/ping` needs `Authorization: Bearer <token>`. `EventSource` cannot set headers, so `/api/events?token=<token>` is also accepted.
- Request and response bodies are JSON unless noted. Errors are `{"error": "message", "id": null}` with status 400 (bad input), 404 (unknown task/queue), 409 (duplicate, `id` = existing task) or 500.
- Static files from the `ui/` folder are served without a token (`/` = `index.html`). No external fonts, scripts or CDN calls.
- No CORS headers: the UI must be served by this same server.

## Task object

```json
{
  "id": 7, "url": "https://…/file.zip", "name": "file.zip", "path": "C:\\…\\file.zip",
  "dir": "C:\\…\\Compressed", "category": "Compressed", "queue": "Main",
  "status": "Queued | Downloading | Paused | Done | Failed",
  "total": 1048576, "downloaded": 524288, "speed": 2097152, "eta_secs": 3,
  "error": null, "added": 1792780170, "finished": null, "priority": 0,
  "opts": { "connections": null, "speed_limit": null, "referer": null, "cookie": null,
            "headers": [], "user_agent": null, "sha256": null }
}
```

`name` is empty until the download has started. `speed` is bytes/second, `eta_secs` may be null. Times are Unix seconds.

## Endpoints

| Method and path | Body / query | Result |
|---|---|---|
| `GET /api/ping` | none (no token) | `{"app","version"}` |
| `GET /api/tasks` | `?status=Done&category=Video&queue=Main&q=text` (all optional; `q` matches name or URL) | array of task objects. History = `status=Done` |
| `POST /api/tasks` | `{urls, queue?, dir?, paused?, force?, referer?, cookie?, user_agent?, headers?{}, sha256?, connections?, speed_limit?}` | `{added:[ids], duplicates:[ids], invalid:[text]}`. `urls` is text with one link or pattern per line; `f[001-050].jpg`, `[1-10:2]`, `[a-e]` expand; lines starting with `#` are ignored |
| `GET /api/tasks/{id}` | | task object |
| `DELETE /api/tasks/{id}?files=true` | `files` also deletes the file or partial file | `{ok}` |
| `POST /api/tasks/{id}/pause` | `{}` | `{ok}` |
| `POST /api/tasks/{id}/resume` | `{}` (also retries a failed task) | `{ok}` |
| `POST /api/tasks/{id}/queue` | `{queue}` | `{ok}` |
| `POST /api/tasks/{id}/open` | `{what:"file"|"folder"}` (finished tasks only) | `{ok}` |
| `POST /api/tasks/pause-all`, `/resume-all` | `{}` | `{ok}` |
| `POST /api/tasks/reorder` | `{ids:[…]}` first = runs first | `{ok}` |
| `GET /api/queues` | | `[{name, max_simultaneous, enabled, schedule, on_finish}]` |
| `PUT /api/queues/{name}` | queue object (name comes from the path); creates or replaces | `{ok}` |
| `DELETE /api/queues/{name}` | tasks move to `Main`; `Main` cannot be removed (400) | `{ok}` |
| `GET /api/settings` | | settings object (below) |
| `PUT /api/settings` | full settings object | saved settings (values are clamped) |
| `GET /api/export` | `?status=&queue=` | `text/plain`, one URL per line |
| `POST /api/capture` | `{url, referer?, cookie?, user_agent?}` (browser extension) | `{captured:false, reason:"excluded"}` or `{captured:true, id, duplicate?}` |
| `POST /api/quit` | `{}` | `{ok}`; saves progress and exits the app |
| `GET /api/events` | Server-Sent Events | see below |

### Queue object

```json
{ "name": "Night", "max_simultaneous": 2, "enabled": true,
  "schedule": { "days": 127, "start": 1380, "stop": 360 },
  "on_finish": "None | Exit | Sleep | Hibernate | Shutdown | {\"Command\": \"cmd text\"}" }
```

`schedule` may be null (always open). `days` is a bit mask, bit 0 = Monday … bit 6 = Sunday (127 = every day). `start` and `stop` are minutes since local midnight, either may be null; `start` > `stop` means an overnight window.

### Settings object

```json
{ "download_root": "C:\\Users\\me\\Downloads",
  "categories": [{ "name": "Video", "extensions": ["mp4","mkv"], "folder": "Video" }],
  "other_folder": "Other",
  "site_rules": [{ "host": "example.com", "folder": "Work" }],
  "exclusions": { "extensions": ["torrent"], "sites": ["bank.com"] },
  "connections": 8, "max_simultaneous": 3, "speed_limit": null,
  "user_agent": null, "proxy": null, "on_complete_command": null,
  "clipboard_monitor": false, "start_with_windows": false }
```

`speed_limit` is bytes/second for the whole app (null = unlimited). `folder` values are relative to `download_root` unless absolute. `on_complete_command` runs after each finished download, `{path}` is replaced by the quoted file path.

## Live events: `GET /api/events?token=…`

| `event:` | `data:` |
|---|---|
| `progress` | every 500 ms: `[{id, name, downloaded, total, speed, eta_secs}]` for running downloads (may be `[]`) |
| `task` | `{kind:"added"|"changed"|"removed"|"finished", id}`; re-fetch `GET /api/tasks/{id}` on added/changed/finished |
| `drained` | `{queue, action}` a queue finished everything |
| `clipboard` | `{url}` the user copied a link and `clipboard_monitor` is on; ask before adding |

## Not covered yet

Site logins (needs encrypted storage), tray notifications, the browser extension's own messages (Phase 4).
