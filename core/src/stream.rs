//! Streaming downloads (HLS and DASH): pick a quality, fetch the pieces in parallel (decrypting AES-128),
//! keep finished pieces on disk so a stopped download resumes, then join them into one file.
use crate::dash::{self, Kind};
use crate::engine::{backoff, Control, Options};
use crate::error::{Error, Result};
use crate::hls::{self, Key, Playlist};
use crate::probe::{status_error, Probe};
use crate::util::{numbered, sanitize};
use aes::Aes128;
use cbc::cipher::{block_padding::Pkcs7, BlockModeDecrypt, KeyIvInit};
use futures_util::StreamExt;
use reqwest::header::RANGE;
use reqwest::{Client, StatusCode, Url};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

type Aes128Cbc = cbc::Decryptor<Aes128>;

/// A response is a stream manifest if the server says so or the link looks like one.
pub fn is_manifest(p: &Probe) -> bool {
    let ct = p.content_type.as_deref().unwrap_or("").to_ascii_lowercase();
    let path = Url::parse(&p.final_url).map(|u| u.path().to_ascii_lowercase()).unwrap_or_default();
    ct.contains("mpegurl") || ct.contains("dash+xml") || path.ends_with(".m3u8") || path.ends_with(".mpd")
}

struct Piece {
    url: Url,
    range: Option<(u64, u64)>,
    key: Option<Key>,
    seq: u64,
}

struct TrackPlan {
    label: &'static str,
    ext: &'static str,
    /// The init piece (if any) comes first.
    pieces: Vec<Piece>,
}

struct Plan {
    tracks: Vec<TrackPlan>,
    est_bytes: u64,
}

async fn get_text(client: &Client, url: &Url) -> Result<(String, Url)> {
    let r = client.get(url.clone()).send().await?;
    if !r.status().is_success() {
        return Err(status_error(r.status()));
    }
    let final_url = r.url().clone();
    let bytes = r.bytes().await?;
    if bytes.len() > 8 << 20 {
        return Err(Error::Invalid("the playlist is unreasonably large".into()));
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), final_url))
}

fn hls_track(label: &'static str, m: &hls::Media, audio: bool) -> Result<TrackPlan> {
    if !m.ended {
        return Err(Error::Invalid("this is a live stream and cannot be downloaded yet".into()));
    }
    let ext = match (m.fmp4, audio) {
        (true, true) => "m4a",
        (true, false) => "mp4",
        (false, true) => "aac",
        (false, false) => "ts",
    };
    let piece = |s: &hls::Segment| Piece { url: s.url.clone(), range: s.range, key: s.key.clone(), seq: s.seq };
    Ok(TrackPlan { label, ext, pieces: m.init.iter().chain(m.segments.iter()).map(piece).collect() })
}

async fn hls_media(client: &Client, url: &Url) -> Result<hls::Media> {
    let (text, final_url) = get_text(client, url).await?;
    match hls::parse(&text, &final_url)? {
        Playlist::Media(m) => Ok(m),
        Playlist::Master { .. } => Err(Error::Invalid("the playlist points to another playlist instead of video pieces".into())),
    }
}

async fn plan_hls(client: &Client, text: &str, base: &Url, o: &Options) -> Result<Plan> {
    match hls::parse(text, base)? {
        Playlist::Media(m) => {
            let secs: f64 = m.segments.iter().map(|s| s.duration).sum();
            Ok(Plan { tracks: vec![hls_track("video", &m, false)?], est_bytes: (secs * 500_000.0 / 8.0) as u64 })
        }
        Playlist::Master { variants, audio } => {
            let v = hls::pick_variant(&variants, o.max_height).ok_or_else(|| Error::Invalid("the playlist has no video".into()))?;
            let media = hls_media(client, &v.url).await?;
            let secs: f64 = media.segments.iter().map(|s| s.duration).sum();
            let mut tracks = vec![hls_track("video", &media, false)?];
            if let Some(a) = hls::audio_for(v, &audio) {
                let am = hls_media(client, a.url.as_ref().expect("audio_for only returns renditions with a link")).await?;
                tracks.push(hls_track("audio", &am, true)?);
            }
            Ok(Plan { tracks, est_bytes: (secs * v.bandwidth as f64 / 8.0) as u64 })
        }
    }
}

fn dash_track(t: &dash::Track) -> TrackPlan {
    let piece = |u: &Url, seq: u64| Piece { url: u.clone(), range: None, key: None, seq };
    let pieces = t.init.iter().map(|u| piece(u, 0)).chain(t.segments.iter().enumerate().map(|(i, u)| piece(u, i as u64))).collect();
    TrackPlan { label: if t.kind == Kind::Video { "video" } else { "audio" }, ext: if t.kind == Kind::Video { "mp4" } else { "m4a" }, pieces }
}

fn plan_dash(text: &str, base: &Url, o: &Options) -> Result<Plan> {
    let m = dash::parse(text, base)?;
    let (v, a) = dash::pick(&m, o.max_height);
    let chosen: Vec<&dash::Track> = v.into_iter().chain(a).collect();
    let est = chosen.iter().map(|t| (t.bandwidth as f64 * t.duration.unwrap_or(0.0) / 8.0) as u64).sum();
    Ok(Plan { tracks: chosen.into_iter().map(dash_track).collect(), est_bytes: est })
}

// ---- ffmpeg (optional) ----

pub fn find_ffmpeg(o: &Options) -> Option<PathBuf> {
    if let Some(p) = &o.ffmpeg_path {
        return p.exists().then(|| p.clone());
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    let beside = exe_dir.into_iter().flat_map(|d| [d.join("ffmpeg.exe"), d.join("tools").join("ffmpeg.exe")]);
    let on_path = std::env::var_os("PATH").into_iter().flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).flat_map(|d| [d.join("ffmpeg.exe"), d.join("ffmpeg")]);
    beside.chain(on_path).find(|p| p.is_file())
}

fn ffmpeg_copy(ffmpeg: &Path, inputs: &[&Path], out: &Path) -> std::result::Result<(), String> {
    let mut c = std::process::Command::new(ffmpeg);
    c.args(["-y", "-v", "error"]);
    for i in inputs {
        c.arg("-i").arg(i);
    }
    c.args(["-c", "copy"]).arg(out);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    match c.output() {
        Ok(o) if o.status.success() && out.exists() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().chars().take(200).collect()),
        Err(e) => Err(e.to_string()),
    }
}

// ---- naming ----

fn stem_of(o: &Options, url: &Url) -> String {
    let raw = o.file_name.clone().or_else(|| url.path_segments().and_then(|mut s| s.next_back().map(str::to_string))).unwrap_or_default();
    let name = sanitize(&raw);
    let stem = match name.rsplit_once('.') {
        Some((s, ext)) if matches!(ext.to_ascii_lowercase().as_str(), "m3u8" | "mpd" | "mp4" | "ts" | "mkv" | "webm") && !s.is_empty() => s.to_string(),
        _ => name,
    };
    if stem.is_empty() || stem == "download" { "video".into() } else { stem }
}

/// Reuse `<stem>.parts` when it belongs to this URL (resume), else take the first free `<stem> (n)`.
fn pick_stem(dir: &Path, stem: &str, ext: &str, url: &str) -> String {
    for n in 0.. {
        let cand = numbered(stem, n);
        let parts = dir.join(format!("{cand}.parts"));
        if parts.exists() {
            if fs::read_to_string(parts.join("url.txt")).is_ok_and(|u| u == url) {
                return cand;
            }
            continue;
        }
        if !dir.join(format!("{cand}.{ext}")).exists() {
            return cand;
        }
    }
    unreachable!()
}

// ---- fetching pieces ----

struct Shared {
    client: Client,
    ctl: Control,
    stop: AtomicBool,
    keys: Mutex<HashMap<Url, Vec<u8>>>,
    retries: u32,
    done: AtomicU64,
    count: u64,
}

impl Shared {
    fn stopped(&self) -> bool {
        self.stop.load(Relaxed) || self.ctl.cancel.load(Relaxed)
    }

    async fn key_bytes(&self, k: &Key) -> Result<Vec<u8>> {
        if let Some(b) = self.keys.lock().unwrap().get(&k.url) {
            return Ok(b.clone());
        }
        let r = self.client.get(k.url.clone()).send().await?;
        if !r.status().is_success() {
            return Err(status_error(r.status()));
        }
        let b = r.bytes().await?.to_vec();
        if b.len() != 16 {
            return Err(Error::Server("the decryption key has the wrong size".into()));
        }
        self.keys.lock().unwrap().insert(k.url.clone(), b.clone());
        Ok(b)
    }

    /// One attempt: the whole piece, counted into the progress as it arrives (taken back by the caller on failure).
    async fn fetch_once(&self, p: &Piece, got: &mut u64) -> Result<Vec<u8>> {
        let mut req = self.client.get(p.url.clone());
        if let Some((off, len)) = p.range {
            req = req.header(RANGE, format!("bytes={off}-{}", off + len - 1));
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !(status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT) {
            return Err(status_error(status));
        }
        let mut body = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if self.stopped() {
                return Err(Error::Cancelled);
            }
            self.ctl.throttle(chunk.len() as u64).await;
            self.ctl.progress.downloaded.fetch_add(chunk.len() as u64, Relaxed);
            *got += chunk.len() as u64;
            body.extend_from_slice(&chunk);
        }
        if let (Some((off, len)), StatusCode::OK) = (p.range, status) {
            // the server ignored the range and sent the whole resource: cut our part out
            body = body.get(off as usize..(off + len) as usize).ok_or_else(|| Error::Server("short answer for a byte range".into()))?.to_vec();
        }
        if let Some(k) = &p.key {
            let key = self.key_bytes(k).await?;
            let iv = k.iv.unwrap_or_else(|| {
                let mut iv = [0u8; 16];
                iv[8..].copy_from_slice(&p.seq.to_be_bytes());
                iv
            });
            body = Aes128Cbc::new_from_slices(&key, &iv)
                .map_err(|_| Error::Server("bad decryption key".into()))?
                .decrypt_padded_vec::<Pkcs7>(&body)
                .map_err(|_| Error::Server("could not decrypt a piece (wrong key?)".into()))?;
        }
        Ok(body)
    }

    async fn fetch(&self, p: &Piece) -> Result<Vec<u8>> {
        let mut fails = 0;
        loop {
            if self.stopped() {
                return Err(Error::Cancelled);
            }
            let mut got = 0;
            match self.fetch_once(p, &mut got).await {
                Ok(b) => {
                    // decrypted size may be a little smaller than what was counted while receiving
                    self.ctl.progress.downloaded.fetch_sub(got.saturating_sub(b.len() as u64), Relaxed);
                    return Ok(b);
                }
                Err(e) if e.retryable() && fails < self.retries => {
                    self.ctl.progress.downloaded.fetch_sub(got, Relaxed);
                    fails += 1;
                    tokio::time::sleep(backoff(fails)).await;
                }
                Err(e) => {
                    self.ctl.progress.downloaded.fetch_sub(got, Relaxed);
                    return Err(e);
                }
            }
        }
    }

    fn piece_finished(&self) {
        let done = self.done.fetch_add(1, Relaxed) + 1;
        let have = self.ctl.progress.downloaded.load(Relaxed);
        // the estimate from bandwidth x duration is replaced by what the finished pieces actually weigh
        self.ctl.progress.total.store(have.max(have / done * self.count), Relaxed);
    }
}

fn seg_path(dir: &Path, i: usize) -> PathBuf {
    dir.join(format!("{i:05}.seg"))
}

/// Download one stream (HLS or DASH). Returns the path of the finished file.
pub async fn run(client: &Client, url: &str, dir: &Path, o: &Options, ctl: &Control) -> Result<PathBuf> {
    let manifest_url = Url::parse(url).map_err(|_| Error::Invalid("not a valid link".into()))?;
    let (text, base) = get_text(client, &manifest_url).await?;
    let plan = if text.trim_start_matches('\u{feff}').trim_start().starts_with("#EXTM3U") {
        plan_hls(client, &text, &base, o).await?
    } else if text.contains("<MPD") {
        plan_dash(&text, &base, o)?
    } else {
        return Err(Error::Invalid("this is neither an HLS nor a DASH manifest".into()));
    };
    if plan.tracks.is_empty() || plan.tracks.iter().any(|t| t.pieces.is_empty()) {
        return Err(Error::Invalid("the stream has no pieces to download".into()));
    }

    // what the finished file will be called
    let ffmpeg = find_ffmpeg(o);
    let video = &plan.tracks[0];
    let final_ext = match (plan.tracks.len(), video.ext, ffmpeg.is_some()) {
        (1, "ts", true) | (2, _, true) => "mp4",
        (_, e, _) => e,
    };
    fs::create_dir_all(dir)?;
    let stem = pick_stem(dir, &stem_of(o, &manifest_url), final_ext, url);
    let parts = dir.join(format!("{stem}.parts"));
    fs::create_dir_all(&parts)?;
    fs::write(parts.join("url.txt"), url)?;
    let final_path = dir.join(format!("{stem}.{final_ext}"));
    *ctl.progress.name.lock().unwrap() = format!("{stem}.{final_ext}");

    // pieces already on disk from an earlier run count as done
    let exts: Vec<(&'static str, &'static str)> = plan.tracks.iter().map(|t| (t.label, t.ext)).collect();
    let count: u64 = plan.tracks.iter().map(|t| t.pieces.len() as u64).sum();
    let sh = Arc::new(Shared { client: client.clone(), ctl: ctl.clone(), stop: AtomicBool::new(false), keys: Mutex::new(HashMap::new()), retries: o.retries, done: AtomicU64::new(0), count });
    ctl.progress.total.store(plan.est_bytes, Relaxed);
    // every distinct key is fetched once up front (also fails early when the key is not available)
    for k in plan.tracks.iter().flat_map(|t| t.pieces.iter()).filter_map(|p| p.key.as_ref()) {
        sh.key_bytes(k).await?;
    }
    let mut todo: Vec<(PathBuf, Arc<Piece>)> = vec![];
    for t in plan.tracks {
        let tdir = parts.join(t.label);
        fs::create_dir_all(&tdir)?;
        for (i, p) in t.pieces.into_iter().enumerate() {
            let path = seg_path(&tdir, i);
            match fs::metadata(&path) {
                Ok(m) => {
                    ctl.progress.downloaded.fetch_add(m.len(), Relaxed);
                    sh.done.fetch_add(1, Relaxed);
                }
                Err(_) => todo.push((path, Arc::new(p))),
            }
        }
    }
    let gate = Arc::new(Semaphore::new(o.connections.clamp(1, 16)));
    let mut set = JoinSet::new();
    for (path, piece) in todo {
        let (sh, gate) = (sh.clone(), gate.clone());
        set.spawn(async move {
            let _permit = gate.acquire().await.map_err(|_| Error::Cancelled)?;
            let bytes = sh.fetch(&piece).await?;
            let tmp = path.with_extension("seg.tmp");
            fs::write(&tmp, &bytes)?; // shortcut: blocking write of one piece, fine for local disks
            fs::rename(&tmp, &path)?;
            sh.piece_finished();
            Ok::<(), Error>(())
        });
    }
    let mut first: Option<Error> = None;
    while let Some(r) = set.join_next().await {
        let err = match r {
            Ok(Ok(())) => continue,
            Ok(Err(e)) => e,
            Err(j) => Error::Server(j.to_string()),
        };
        sh.stop.store(true, Relaxed);
        first.get_or_insert(err);
    }
    if let Some(e) = first {
        return Err(e); // finished pieces stay in the .parts folder for the next attempt
    }

    // join the pieces of each track, in order
    let mut files: Vec<(&'static str, PathBuf)> = vec![]; // (extension, joined file)
    for (label, ext) in exts {
        let mut segs: Vec<PathBuf> = fs::read_dir(parts.join(label))?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "seg")).collect();
        segs.sort();
        let joined = parts.join(format!("{label}.joined"));
        let mut out = fs::File::create(&joined)?;
        for s in &segs {
            out.write_all(&fs::read(s)?)?;
        }
        out.flush()?;
        files.push((ext, joined));
    }
    let result = finish(&files, &final_path, ffmpeg.as_deref(), ctl)?;
    let _ = fs::remove_dir_all(&parts);
    *ctl.progress.name.lock().unwrap() = result.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(result)
}

/// Turn the joined tracks into the final file. Returns the path that really holds the video.
fn finish(files: &[(&'static str, PathBuf)], final_path: &Path, ffmpeg: Option<&Path>, ctl: &Control) -> Result<PathBuf> {
    let note = |s: String| *ctl.progress.note.lock().unwrap() = s;
    let (vext, video) = (&files[0].0, &files[0].1);
    // video kept as it is, under its own extension (used when ffmpeg is missing or fails)
    let keep_video = || -> Result<PathBuf> {
        let p = final_path.with_extension(vext);
        fs::rename(video, &p)?;
        Ok(p)
    };
    match (files.len(), ffmpeg) {
        (1, Some(ff)) if *vext == "ts" => match ffmpeg_copy(ff, &[video], final_path) {
            Ok(()) => Ok(final_path.to_path_buf()),
            Err(e) => {
                note(format!("Saved as .ts because ffmpeg could not convert it to .mp4: {e}"));
                keep_video()
            }
        },
        (1, _) => keep_video(),
        (_, Some(ff)) => match ffmpeg_copy(ff, &[video, &files[1].1], final_path) {
            Ok(()) => Ok(final_path.to_path_buf()),
            Err(e) => {
                let p = keep_video()?;
                fs::rename(&files[1].1, p.with_extension(format!("audio.{}", files[1].0)))?;
                note(format!("Video and audio are saved as separate files because ffmpeg could not merge them: {e}"));
                Ok(p)
            }
        },
        (_, None) => {
            let p = keep_video()?;
            fs::rename(&files[1].1, p.with_extension(format!("audio.{}", files[1].0)))?;
            note("Video and audio are saved as separate files. Install ffmpeg (or set its path in settings) to merge them into one.".into());
            Ok(p)
        }
    }
}