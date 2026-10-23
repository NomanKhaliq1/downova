use crate::error::{Error, Result};
use crate::limiter::Limiter;
use crate::probe::{parse_content_range, probe, status_error, Probe};
use crate::state::{initial_segments, load_state, save_state, Seg, State};
use crate::util::numbered;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, COOKIE, IF_RANGE, RANGE, REFERER, USER_AGENT};
use reqwest::{Client, StatusCode};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinSet;

#[derive(Clone, Debug)]
pub struct Options {
    /// Parallel connections per file (1..=32).
    pub connections: usize,
    pub headers: Vec<(String, String)>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub cookie: Option<String>,
    pub proxy: Option<String>,
    /// Bytes per second, shared by every connection of this download.
    pub speed_limit: Option<u64>,
    /// Consecutive failures without progress before giving up.
    pub retries: u32,
    pub save_interval: Duration,
    /// Lower-case hex SHA-256 the finished file must match.
    pub sha256: Option<String>,
    /// A segment is only split in two if both halves stay at least this big.
    pub min_split: u64,
    /// Streams (HLS/DASH): highest picture height wanted; None = best available.
    pub max_height: Option<u32>,
    /// Where ffmpeg is (merges separate video/audio, converts .ts to .mp4). None = look beside the app and on PATH.
    pub ffmpeg_path: Option<PathBuf>,
    /// Name for the saved file when the link itself has no useful one (a page title, for example).
    pub file_name: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            connections: 8,
            headers: vec![],
            user_agent: None,
            referer: None,
            cookie: None,
            proxy: None,
            speed_limit: None,
            retries: 8,
            save_interval: Duration::from_secs(2),
            sha256: None,
            min_split: 1 << 20,
            max_height: None,
            ffmpeg_path: None,
            file_name: None,
        }
    }
}

#[derive(Default)]
pub struct Progress {
    pub downloaded: AtomicU64,
    pub total: AtomicU64,
    /// File name chosen for this download (empty until the probe is done).
    pub name: Mutex<String>,
    /// A remark for the user (for example: video and audio were saved separately).
    pub note: Mutex<String>,
}

/// Handles the caller keeps to watch, throttle or cancel a running download.
#[derive(Clone)]
pub struct Control {
    pub progress: Arc<Progress>,
    pub cancel: Arc<AtomicBool>,
    pub limiter: Arc<Limiter>,
    /// Optional limiter shared by many downloads (the app-wide speed cap).
    pub global: Option<Arc<Limiter>>,
}

impl Control {
    pub fn new(o: &Options) -> Self {
        Control {
            progress: Arc::default(),
            cancel: Arc::default(),
            limiter: Arc::new(Limiter::new(o.speed_limit)),
            global: None,
        }
    }

    pub fn with_global(mut self, g: Arc<Limiter>) -> Self {
        self.global = Some(g);
        self
    }

    pub(crate) async fn throttle(&self, n: u64) {
        self.limiter.take(n).await;
        if let Some(g) = &self.global {
            g.take(n).await;
        }
    }
}

const DEFAULT_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

fn bad_header<E: std::fmt::Display>(e: E) -> Error {
    Error::Server(format!("invalid header: {e}"))
}

fn build_client(o: &Options) -> Result<Client> {
    let mut h = HeaderMap::new();
    h.insert(USER_AGENT, HeaderValue::from_str(o.user_agent.as_deref().unwrap_or(DEFAULT_UA)).map_err(bad_header)?);
    if let Some(r) = &o.referer {
        h.insert(REFERER, HeaderValue::from_str(r).map_err(bad_header)?);
    }
    if let Some(c) = &o.cookie {
        h.insert(COOKIE, HeaderValue::from_str(c).map_err(bad_header)?);
    }
    for (k, v) in &o.headers {
        h.insert(HeaderName::from_bytes(k.as_bytes()).map_err(bad_header)?, HeaderValue::from_str(v).map_err(bad_header)?);
    }
    let mut b = Client::builder()
        .default_headers(h)
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30));
    if let Some(p) = &o.proxy {
        b = b.proxy(reqwest::Proxy::all(p)?);
    }
    Ok(b.build()?)
}

pub(crate) fn backoff(fails: u32) -> Duration {
    Duration::from_millis((500u64 << fails.min(6)).min(30_000))
}

#[cfg(windows)]
fn write_all_at(f: &File, mut buf: &[u8], mut off: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = f.seek_write(buf, off)?;
        buf = &buf[n..];
        off += n as u64;
    }
    Ok(())
}

#[cfg(unix)]
fn write_all_at(f: &File, buf: &[u8], off: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(f, buf, off)
}

/// Pick `<name>`, `<name> (1)`... Reuse a slot only when it holds this URL's unfinished download.
fn pick_name(dir: &Path, name: &str, url: &str) -> String {
    for n in 0.. {
        let cand = numbered(name, n);
        let st = dir.join(format!("{cand}.part.json"));
        match load_state(&st) {
            Some(s) if s.url == url => return cand,
            Some(_) => continue,
            None => {}
        }
        if !dir.join(&cand).exists() && !dir.join(format!("{cand}.part")).exists() {
            return cand;
        }
    }
    unreachable!()
}

fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Download `url` into `dir`; returns the final path. Resumes automatically when a matching `.part.json` exists.
pub async fn download(url: &str, dir: &Path, o: &Options, ctl: &Control) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let client = build_client(o)?;
    let p = probe(&client, url).await?;
    ctl.progress.total.store(p.total.unwrap_or(0), Relaxed);

    if crate::stream::is_manifest(&p) {
        return crate::stream::run(&client, url, dir, o, ctl).await;
    }

    let name = pick_name(dir, &p.filename, url);
    *ctl.progress.name.lock().unwrap() = name.clone();
    let final_path = dir.join(&name);
    let part = dir.join(format!("{name}.part"));
    let st_path = dir.join(format!("{name}.part.json"));

    if p.resumable && p.total.is_some_and(|t| t > 0) {
        segmented(&client, url, &p, &part, &st_path, o, ctl).await?;
    } else {
        single(&client, url, &part, o, ctl).await?;
    }

    if let Some(want) = &o.sha256 {
        let part2 = part.clone();
        let got = tokio::task::spawn_blocking(move || sha256_of(&part2)).await.map_err(|e| Error::Server(e.to_string()))??;
        if !got.eq_ignore_ascii_case(want) {
            let _ = fs::remove_file(&part);
            let _ = fs::remove_file(&st_path);
            return Err(Error::Checksum);
        }
    }
    fs::rename(&part, &final_path)?;
    let _ = fs::remove_file(&st_path);
    Ok(final_path)
}

// ---- no Range support: one stream, restart from zero on failure ----

async fn single(client: &Client, url: &str, part: &Path, o: &Options, ctl: &Control) -> Result<()> {
    let mut fails = 0;
    loop {
        match single_once(client, url, part, ctl).await {
            Err(e) if e.retryable() && fails < o.retries => {
                fails += 1;
                ctl.progress.downloaded.store(0, Relaxed);
                tokio::time::sleep(backoff(fails)).await;
            }
            r => return r,
        }
    }
}

async fn single_once(client: &Client, url: &str, part: &Path, ctl: &Control) -> Result<()> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(status_error(resp.status()));
    }
    if let Some(len) = resp.content_length() {
        ctl.progress.total.store(len, Relaxed);
    }
    let mut f = File::create(part)?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if ctl.cancel.load(Relaxed) {
            return Err(Error::Cancelled);
        }
        ctl.throttle(chunk.len() as u64).await;
        f.write_all(&chunk)?; // shortcut: blocking write on the async thread, fine for local disks
        ctl.progress.downloaded.fetch_add(chunk.len() as u64, Relaxed);
    }
    Ok(())
}

// ---- segmented download ----

struct Inner {
    segs: Vec<Seg>,
    /// A worker is currently fetching this segment.
    busy: Vec<bool>,
}

struct Ctx {
    client: Client,
    url: String,
    validator: Option<String>,
    file: File,
    inner: Mutex<Inner>,
    /// Set when any worker fails, so the others stop too.
    stop: AtomicBool,
    ctl: Control,
    retries: u32,
    min_split: u64,
}

impl Ctx {
    fn stopped(&self) -> bool {
        self.stop.load(Relaxed) || self.ctl.cancel.load(Relaxed)
    }

    /// Next unfinished idle segment, else half of the biggest remaining one (so fast connections help slow ones).
    fn claim(&self) -> Option<usize> {
        let mut g = self.inner.lock().unwrap();
        if let Some(i) = (0..g.segs.len()).find(|&i| !g.busy[i] && g.segs[i].remaining() > 0) {
            g.busy[i] = true;
            return Some(i);
        }
        let (i, rem) = (0..g.segs.len()).map(|i| (i, g.segs[i].remaining())).max_by_key(|&(_, r)| r)?;
        if rem < self.min_split * 2 {
            return None;
        }
        let mid = g.segs[i].pos + rem / 2;
        let end = g.segs[i].end;
        g.segs[i].end = mid - 1;
        g.segs.push(Seg { start: mid, end, pos: mid });
        g.busy.push(true);
        Some(g.segs.len() - 1)
    }

    fn release(&self, i: usize) {
        self.inner.lock().unwrap().busy[i] = false;
    }

    async fn worker(self: Arc<Self>) -> Result<()> {
        while let Some(i) = self.claim() {
            let r = self.run_seg(i).await;
            self.release(i);
            r?;
        }
        Ok(())
    }

    async fn run_seg(&self, i: usize) -> Result<()> {
        let mut fails = 0;
        loop {
            if self.stopped() {
                return Err(Error::Cancelled);
            }
            let (pos, end) = {
                let g = self.inner.lock().unwrap();
                (g.segs[i].pos, g.segs[i].end)
            };
            if pos > end {
                return Ok(());
            }
            let mut got = 0;
            match self.fetch(i, pos, end, &mut got).await {
                Ok(()) if got > 0 => fails = 0,
                Ok(()) => fails += 1, // empty body: count as a failure so we never spin
                Err(e) if e.retryable() => {
                    fails = if got > 0 { 1 } else { fails + 1 };
                    if fails > self.retries {
                        return Err(e);
                    }
                }
                Err(e) => return Err(e),
            }
            if fails > self.retries {
                return Err(Error::Transient("server keeps sending empty answers".into()));
            }
            if fails > 0 {
                tokio::time::sleep(backoff(fails)).await;
            }
        }
    }

    async fn fetch(&self, i: usize, pos: u64, end: u64, got: &mut u64) -> Result<()> {
        let mut req = self.client.get(&self.url).header(RANGE, format!("bytes={pos}-{end}"));
        if let Some(v) = &self.validator {
            req = req.header(IF_RANGE, v);
        }
        let resp = req.send().await?;
        match resp.status() {
            StatusCode::PARTIAL_CONTENT => {}
            StatusCode::OK => return Err(Error::Changed), // Range ignored or If-Range failed
            s => return Err(status_error(s)),
        }
        let start = resp.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok()).and_then(parse_content_range);
        if start.map(|(s, _)| s) != Some(pos) {
            return Err(Error::Server("server answered with the wrong byte range".into()));
        }
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if self.stopped() {
                return Err(Error::Cancelled);
            }
            let (pos, n) = {
                let g = self.inner.lock().unwrap();
                let s = &g.segs[i];
                (s.pos, (chunk.len() as u64).min(s.remaining()))
            };
            if n == 0 {
                break; // another worker took the rest of this segment
            }
            self.ctl.throttle(n).await;
            write_all_at(&self.file, &chunk[..n as usize], pos)?; // shortcut: blocking positional write, fine for local disks
            let mut g = self.inner.lock().unwrap();
            let s = &mut g.segs[i];
            // the end may have shrunk while we wrote; the overlap is identical data, just don't count it twice
            let adv = n.min(s.remaining());
            s.pos += adv;
            *got += adv;
            self.ctl.progress.downloaded.fetch_add(adv, Relaxed);
            if s.remaining() == 0 {
                break;
            }
        }
        Ok(())
    }

    fn snapshot(&self, meta: &State) -> State {
        State { segs: self.inner.lock().unwrap().segs.clone(), ..meta.clone() }
    }
}

fn same_file(s: &State, p: &Probe) -> bool {
    s.total == p.total.unwrap_or(0) && s.etag == p.etag && s.last_modified == p.last_modified
}

async fn segmented(client: &Client, url: &str, p: &Probe, part: &Path, st_path: &Path, o: &Options, ctl: &Control) -> Result<()> {
    let total = p.total.unwrap_or(0);
    let resumed = load_state(st_path).filter(|s| s.url == url && same_file(s, p) && part.exists());
    let state = match resumed {
        Some(s) => s,
        None => {
            let dir = part.parent().unwrap_or(Path::new("."));
            if fs4::available_space(dir).is_ok_and(|free| free < total) {
                return Err(Error::DiskSpace);
            }
            let f = File::create(part)?;
            f.set_len(total)?;
            State {
                url: url.into(),
                total,
                etag: p.etag.clone(),
                last_modified: p.last_modified.clone(),
                segs: initial_segments(total, o.connections.clamp(1, 32), o.min_split),
            }
        }
    };
    ctl.progress.downloaded.store(state.downloaded(), Relaxed);

    let workers = o.connections.clamp(1, 32).min(total.div_ceil(o.min_split.max(1)).max(1) as usize);
    let ctx = Arc::new(Ctx {
        client: client.clone(),
        url: url.into(),
        validator: p.validator(),
        file: OpenOptions::new().write(true).open(part)?,
        inner: Mutex::new(Inner { busy: vec![false; state.segs.len()], segs: state.segs.clone() }),
        stop: AtomicBool::new(false),
        ctl: ctl.clone(),
        retries: o.retries,
        min_split: o.min_split,
    });

    let saver = {
        let (ctx, meta, path, every) = (ctx.clone(), state.clone(), st_path.to_path_buf(), o.save_interval);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                let _ = save_state(&path, &ctx.snapshot(&meta));
            }
        })
    };

    let mut set = JoinSet::new();
    for _ in 0..workers {
        set.spawn(ctx.clone().worker());
    }
    let mut first: Option<Error> = None;
    while let Some(r) = set.join_next().await {
        let err = match r {
            Ok(Ok(())) => continue,
            Ok(Err(e)) => e,
            Err(join) => Error::Server(join.to_string()),
        };
        ctx.stop.store(true, Relaxed);
        first.get_or_insert(err);
    }
    saver.abort();
    let _ = saver.await; // wait until the task dropped its handle on the file, or Windows refuses the rename

    if let Some(e) = first {
        save_state(st_path, &ctx.snapshot(&state))?; // keep progress for the next attempt
        return Err(e);
    }
    let left: u64 = ctx.inner.lock().unwrap().segs.iter().map(Seg::remaining).sum();
    if left != 0 {
        return Err(Error::Server(format!("{left} bytes missing after download")));
    }
    // ctx (and its file handle) is dropped when this function returns, before the caller renames the file
    Ok(())
}
