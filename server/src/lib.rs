//! Local REST + SSE API and static UI hosting for the download manager.
//! Loopback only; every /api call except /api/ping needs the install token.
pub mod clipboard;
pub mod system;

use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as Sse1, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use dm_core::{AddOpts, Error, Filter, Manager, Options, Queue, Settings, Status, TaskOpts};
use futures_util::stream::{select, Stream, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_stream::wrappers::{BroadcastStream, IntervalStream};

pub struct AppState {
    pub mgr: Manager,
    pub token: String,
    pub product_id: String,
    /// Folder with the web UI files (index.html ...). Served without auth; it holds no secrets.
    pub ui_dir: PathBuf,
    /// Links noticed on the clipboard, forwarded to the UI as `clipboard` events.
    pub clip: broadcast::Sender<String>,
    /// Accepted `Host` header values; anything else is refused (DNS-rebinding protection).
    pub hosts: Vec<String>,
    /// Notified by `POST /api/quit` (installer/updater closing a running copy, or an Exit button in the UI).
    pub quit: Arc<tokio::sync::Notify>,
}

pub fn new_token() -> String {
    let mut b = [0u8; 24];
    getrandom::fill(&mut b).expect("system random source");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn router(s: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/tasks", get(list_tasks).post(add_tasks))
        .route("/tasks/pause-all", post(pause_all))
        .route("/tasks/resume-all", post(resume_all))
        .route("/tasks/reorder", post(reorder))
        .route("/tasks/{id}", get(get_task).delete(delete_task))
        .route("/tasks/{id}/pause", post(pause))
        .route("/tasks/{id}/resume", post(resume))
        .route("/tasks/{id}/queue", post(set_queue))
        .route("/tasks/{id}/open", post(open))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/queues", get(list_queues))
        .route("/queues/{name}", axum::routing::put(put_queue).delete(delete_queue))
        .route("/export", get(export))
        .route("/capture", post(capture))
        .route("/tools", get(get_tools))
        .route("/tools/ytdlp/install", post(install_ytdlp))
        .route("/tools/ytdlp/update", post(update_ytdlp))
        .route("/quit", post(quit))
        .route("/events", get(events))
        .layer(middleware::from_fn_with_state(s.clone(), auth));
    Router::new()
        .route("/api/ping", get(ping))
        .nest("/api", api)
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(s.clone(), host_check))
        .with_state(s)
}

// ---- guards ----

async fn host_check(State(s): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let ok = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).is_some_and(|h| s.hosts.iter().any(|a| a.eq_ignore_ascii_case(h)));
    if ok {
        next.run(req).await
    } else {
        (StatusCode::FORBIDDEN, "bad host").into_response()
    }
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

/// `Authorization: Bearer <token>`, or `?token=` for EventSource which cannot set headers.
async fn auth(State(s): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let from_header = req.headers().get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer ")).map(str::to_string);
    let from_query = req.uri().query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token="))).map(str::to_string);
    if from_header.or(from_query).is_some_and(|t| same(&t, &s.token)) {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({"error": "missing or wrong token"}))).into_response()
    }
}

// ---- errors ----

struct ApiError(Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (code, extra) = match &self.0 {
            Error::Invalid(_) => (StatusCode::BAD_REQUEST, Value::Null),
            Error::NotFound => (StatusCode::NOT_FOUND, Value::Null),
            Error::Duplicate(id) => (StatusCode::CONFLICT, json!(id)),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, Value::Null),
        };
        (code, Json(json!({"error": self.0.to_string(), "id": extra}))).into_response()
    }
}

type Res<T = Json<Value>> = Result<T, ApiError>;
type St = State<Arc<AppState>>;

fn ok() -> Res {
    Ok(Json(json!({"ok": true})))
}

// ---- handlers ----

async fn ping(State(s): St) -> Json<Value> {
    Json(json!({"app": s.product_id, "version": env!("CARGO_PKG_VERSION")}))
}

#[derive(Deserialize)]
struct ListQ {
    status: Option<Status>,
    category: Option<String>,
    queue: Option<String>,
    q: Option<String>,
}

async fn list_tasks(State(s): St, Query(q): Query<ListQ>) -> Json<Value> {
    let f = Filter { status: q.status, category: q.category, queue: q.queue, search: q.q };
    Json(json!(s.mgr.list(&f)))
}

async fn get_task(State(s): St, Path(id): Path<u64>) -> Res {
    Ok(Json(json!(s.mgr.get(id).ok_or(Error::NotFound)?)))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AddReq {
    /// One link or pattern per line. A single link is fine.
    urls: String,
    queue: Option<String>,
    dir: Option<PathBuf>,
    paused: bool,
    force: bool,
    referer: Option<String>,
    cookie: Option<String>,
    user_agent: Option<String>,
    headers: HashMap<String, String>,
    sha256: Option<String>,
    connections: Option<usize>,
    speed_limit: Option<u64>,
    /// Name to save under (streams and links without a useful name).
    filename: Option<String>,
    /// Streams: best picture up to this height.
    max_height: Option<u32>,
    /// The link is a page (YouTube...) to hand to the helper tool yt-dlp.
    ytdlp: bool,
    /// With yt-dlp: only the sound.
    audio_only: bool,
}

impl AddReq {
    fn opts(&self) -> AddOpts {
        AddOpts {
            queue: self.queue.clone(),
            dir: self.dir.clone(),
            paused: self.paused,
            force: self.force,
            task: TaskOpts {
                connections: self.connections.map(|c| c.clamp(1, 32)),
                speed_limit: self.speed_limit,
                referer: self.referer.clone(),
                cookie: self.cookie.clone(),
                headers: self.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                user_agent: self.user_agent.clone(),
                sha256: self.sha256.clone(),
                filename: self.filename.clone(),
                max_height: self.max_height,
                ytdlp: self.ytdlp,
                audio_only: self.audio_only,
            },
        }
    }
}

async fn add_tasks(State(s): St, Json(r): Json<AddReq>) -> Res {
    if r.urls.trim().is_empty() {
        return Err(Error::Invalid("urls is empty".into()).into());
    }
    Ok(Json(json!(s.mgr.add_batch(&r.urls, &r.opts()))))
}

async fn pause(State(s): St, Path(id): Path<u64>) -> Res {
    s.mgr.pause(id)?;
    ok()
}

async fn resume(State(s): St, Path(id): Path<u64>) -> Res {
    s.mgr.resume(id)?;
    ok()
}

async fn pause_all(State(s): St) -> Res {
    s.mgr.pause_all();
    ok()
}

async fn resume_all(State(s): St) -> Res {
    s.mgr.resume_all();
    ok()
}

#[derive(Deserialize)]
struct DelQ {
    #[serde(default)]
    files: bool,
}

async fn delete_task(State(s): St, Path(id): Path<u64>, Query(q): Query<DelQ>) -> Res {
    s.mgr.remove(id, q.files)?;
    ok()
}

#[derive(Deserialize)]
struct Ids {
    ids: Vec<u64>,
}

async fn reorder(State(s): St, Json(b): Json<Ids>) -> Res {
    s.mgr.reorder(&b.ids);
    ok()
}

#[derive(Deserialize)]
struct QueueName {
    queue: String,
}

async fn set_queue(State(s): St, Path(id): Path<u64>, Json(b): Json<QueueName>) -> Res {
    s.mgr.set_task_queue(id, &b.queue)?;
    ok()
}

#[derive(Deserialize)]
struct OpenReq {
    /// "file" or "folder"
    what: String,
}

async fn open(State(s): St, Path(id): Path<u64>, Json(b): Json<OpenReq>) -> Res {
    let t = s.mgr.get(id).ok_or(Error::NotFound)?.task;
    let path = match (&t.status, &t.path) {
        (Status::Done, Some(p)) if p.exists() => p.clone(),
        _ => return Err(Error::Invalid("the file is not on disk".into()).into()),
    };
    match b.what.as_str() {
        "file" => system::open_file(&path),
        "folder" => system::reveal(&path),
        _ => return Err(Error::Invalid("what must be file or folder".into()).into()),
    }
    ok()
}

async fn get_settings(State(s): St) -> Json<Settings> {
    Json(s.mgr.settings())
}

async fn put_settings(State(s): St, Json(mut n): Json<Settings>) -> Res<Json<Settings>> {
    n.connections = n.connections.clamp(1, 32);
    n.max_simultaneous = n.max_simultaneous.clamp(1, 16);
    let old = s.mgr.settings();
    s.mgr.set_settings(n.clone());
    if old.start_with_windows != n.start_with_windows {
        system::set_autostart(&s.product_id, n.start_with_windows);
    }
    Ok(Json(n))
}

async fn list_queues(State(s): St) -> Json<Value> {
    Json(json!(s.mgr.queues()))
}

async fn put_queue(State(s): St, Path(name): Path<String>, Json(mut q): Json<Queue>) -> Res {
    if name.trim().is_empty() {
        return Err(Error::Invalid("queue name is empty".into()).into());
    }
    q.name = name;
    q.max_simultaneous = q.max_simultaneous.clamp(1, 16);
    s.mgr.save_queue(q);
    ok()
}

async fn delete_queue(State(s): St, Path(name): Path<String>) -> Res {
    s.mgr.remove_queue(&name)?;
    ok()
}

#[derive(Deserialize)]
struct ExportQ {
    status: Option<Status>,
    queue: Option<String>,
}

async fn export(State(s): St, Query(q): Query<ExportQ>) -> impl IntoResponse {
    let text = s.mgr.export_links(&Filter { status: q.status, queue: q.queue, ..Filter::default() });
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text)
}

#[derive(Deserialize)]
struct CaptureReq {
    url: String,
    referer: Option<String>,
    cookie: Option<String>,
    user_agent: Option<String>,
}

/// Called by the browser extension for every download it sees.
async fn capture(State(s): St, Json(r): Json<CaptureReq>) -> Res {
    if s.mgr.settings().is_excluded(&r.url) {
        return Ok(Json(json!({"captured": false, "reason": "excluded"})));
    }
    let opts = AddOpts {
        task: TaskOpts { referer: r.referer, cookie: r.cookie, user_agent: r.user_agent, ..TaskOpts::default() },
        ..AddOpts::default()
    };
    match s.mgr.add(&r.url, &opts) {
        Ok(id) => Ok(Json(json!({"captured": true, "id": id}))),
        Err(Error::Duplicate(id)) => Ok(Json(json!({"captured": true, "id": id, "duplicate": true}))),
        Err(e) => Err(e.into()),
    }
}

// ---- helper tools (yt-dlp, ffmpeg) ----

fn tool_opts(s: &AppState) -> Options {
    let set = s.mgr.settings();
    Options { ytdlp_path: set.ytdlp_path, ffmpeg_path: set.ffmpeg_path, tools_dir: Some(s.mgr.tools_dir()), ..Options::default() }
}

#[derive(Deserialize)]
struct ToolsQ {
    /// Also run the tool to learn its version (takes a moment, so only on request).
    #[serde(default)]
    details: bool,
}

async fn get_tools(State(s): St, Query(q): Query<ToolsQ>) -> Json<Value> {
    let o = tool_opts(&s);
    let (y, f) = (dm_core::ytdlp::find(&o), dm_core::stream::find_ffmpeg(&o));
    let version = match (&y, q.details) {
        (Some(p), true) => dm_core::tools::ytdlp_version(p).await,
        _ => None,
    };
    let managed = y.as_ref().is_some_and(|p| p.starts_with(s.mgr.tools_dir()));
    Json(json!({"ytdlp": {"ready": y.is_some(), "path": y, "version": version, "managed": managed}, "ffmpeg": {"ready": f.is_some(), "path": f}}))
}

/// Downloads yt-dlp from its GitHub releases (checksum verified). Only called when the user presses the button.
async fn install_ytdlp(State(s): St) -> Res {
    let p = dm_core::tools::install_ytdlp(&s.mgr.tools_dir()).await?;
    let v = dm_core::tools::ytdlp_version(&p).await;
    Ok(Json(json!({"ok": true, "path": p, "version": v})))
}

async fn update_ytdlp(State(s): St) -> Res {
    let p = dm_core::ytdlp::find(&tool_opts(&s)).ok_or(Error::NotFound)?;
    let v = dm_core::tools::update_ytdlp(&p).await?;
    Ok(Json(json!({"ok": true, "version": v})))
}

async fn quit(State(s): St) -> Res {
    s.quit.notify_one();
    ok()
}

// ---- live events ----

fn sse(name: &str, data: Value) -> Sse1 {
    Sse1::default().event(name).data(data.to_string())
}

async fn events(State(s): St) -> Sse<impl Stream<Item = Result<Sse1, Infallible>>> {
    use dm_core::Event as E;
    let tasks = BroadcastStream::new(s.mgr.subscribe()).filter_map(|e| async move {
        let ev = match e.ok()? {
            E::Added(id) => sse("task", json!({"kind": "added", "id": id})),
            E::Changed(id) => sse("task", json!({"kind": "changed", "id": id})),
            E::Removed(id) => sse("task", json!({"kind": "removed", "id": id})),
            E::Finished(id) => sse("task", json!({"kind": "finished", "id": id})),
            E::QueueDrained(q, a) => sse("drained", json!({"queue": q, "action": a})),
        };
        Some(Ok(ev))
    });
    let clip = BroadcastStream::new(s.clip.subscribe()).filter_map(|u| async move { Some(Ok(sse("clipboard", json!({"url": u.ok()?})))) });
    let mgr = s.mgr.clone();
    let progress = IntervalStream::new(tokio::time::interval(Duration::from_millis(500))).map(move |_| {
        let running: Vec<Value> = mgr
            .list(&Filter { status: Some(Status::Downloading), ..Filter::default() })
            .into_iter()
            .map(|v| json!({"id": v.task.id, "name": v.task.name, "downloaded": v.task.downloaded, "total": v.task.total, "speed": v.speed, "eta_secs": v.eta_secs}))
            .collect();
        Ok(sse("progress", json!(running)))
    });
    Sse::new(select(select(tasks, clip), progress)).keep_alive(KeepAlive::default())
}

// ---- static UI ----

async fn static_file(State(s): St, req: Request) -> Response {
    let rel = req.uri().path().trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    if rel.split('/').any(|p| p.is_empty() || p == ".." || p.contains(['\\', ':'])) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(bytes) = tokio::fs::read(s.ui_dir.join(rel)).await else { return StatusCode::NOT_FOUND.into_response() };
    let ct = match rel.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    let h: HeaderMap = [(header::CONTENT_TYPE, ct.parse().unwrap()), (header::CACHE_CONTROL, "no-store".parse().unwrap())].into_iter().collect();
    (h, bytes).into_response()
}
