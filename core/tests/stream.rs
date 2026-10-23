//! HLS and DASH downloads against a local server: quality choice, AES-128, byte ranges, retries, resume, separate tracks.
use aes::Aes128;
use cbc::cipher::{block_padding::Pkcs7, BlockModeEncrypt, KeyIvInit};
use dm_core::{download, AddOpts, Control, Error, Filter, Manager, Options, Settings, Status};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{fs, thread};
use tiny_http::{Header, Request, Response, Server, StatusCode};

const SEG: usize = 100_000;
const KEY: [u8; 16] = *b"0123456789abcdef";
const IV: [u8; 16] = [7; 16];

fn seg_bytes(tag: u8, i: usize) -> Vec<u8> {
    (0..SEG).map(|k| (k as u8).wrapping_add(tag).wrapping_add(i as u8)).collect()
}

fn concat(tag: u8, n: usize) -> Vec<u8> {
    (0..n).flat_map(|i| seg_bytes(tag, i)).collect()
}

fn encrypt(data: &[u8], iv: [u8; 16]) -> Vec<u8> {
    cbc::Encryptor::<Aes128>::new_from_slices(&KEY, &iv).unwrap().encrypt_padded_vec::<Pkcs7>(data)
}

fn playlist(segs: usize, ext: &str, extra: &str) -> String {
    let mut s = format!("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n{extra}");
    for i in 0..segs {
        s += &format!("#EXTINF:4.0,\nseg{i}.{ext}\n");
    }
    s + "#EXT-X-ENDLIST\n"
}

struct Srv {
    port: u16,
    hits: Arc<Mutex<Vec<String>>>,
}

impl Srv {
    fn url(&self, p: &str) -> String {
        format!("http://127.0.0.1:{}{p}", self.port)
    }
    fn count(&self, needle: &str) -> usize {
        self.hits.lock().unwrap().iter().filter(|h| h.contains(needle)).count()
    }
}

fn h(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn serve() -> Srv {
    let server = Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let hits = Arc::new(Mutex::new(vec![]));
    let flaky = Arc::new(AtomicUsize::new(0));
    let hits2 = hits.clone();
    thread::spawn(move || {
        for req in server.incoming_requests() {
            let (hits, flaky) = (hits2.clone(), flaky.clone());
            thread::spawn(move || handle(req, hits, flaky));
        }
    });
    Srv { port, hits }
}

fn reply(req: Request, status: u16, ctype: &str, body: Vec<u8>) {
    let _ = req.respond(Response::from_data(body).with_status_code(StatusCode(status)).with_header(h("Content-Type", ctype)));
}

fn handle(req: Request, hits: Arc<Mutex<Vec<String>>>, flaky: Arc<AtomicUsize>) {
    let path = req.url().to_string();
    let range = req.headers().iter().find(|x| x.field.equiv("Range")).map(|x| x.value.as_str().to_string());
    hits.lock().unwrap().push(format!("{path} {}", range.clone().unwrap_or_default()));
    let m3u8 = "application/vnd.apple.mpegurl";
    let seg_no = |p: &str| p.rsplit('/').next().and_then(|f| f.strip_prefix("seg")).and_then(|f| f.split('.').next()).and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
    match path.as_str() {
        "/s/master.m3u8" => reply(req, 200, m3u8, "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=400000,RESOLUTION=640x360\nv360/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\nv720/index.m3u8\n".into()),
        "/s/v720/index.m3u8" => reply(req, 200, m3u8, playlist(6, "ts", "").into()),
        "/s/v360/index.m3u8" => reply(req, 200, m3u8, playlist(3, "ts", "").into()),
        "/s/live/index.m3u8" => reply(req, 200, m3u8, playlist(3, "ts", "").replace("#EXT-X-ENDLIST\n", "").into()),
        "/s/slow/index.m3u8" => reply(req, 200, m3u8, playlist(8, "ts", "").into()),
        "/s/flaky/index.m3u8" => reply(req, 200, m3u8, playlist(3, "ts", "").into()),
        "/s/enc/index.m3u8" => reply(req, 200, m3u8, playlist(4, "ts", "#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x07070707070707070707070707070707\n").into()),
        "/s/enc2/index.m3u8" => reply(req, 200, m3u8, playlist(4, "ts", "#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n").into()),
        "/s/fmp4/index.m3u8" => reply(req, 200, m3u8, playlist(3, "m4s", "#EXT-X-MAP:URI=\"init.mp4\"\n").into()),
        "/s/drm/index.m3u8" => reply(req, 200, m3u8, playlist(3, "ts", "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\n").into()),
        "/s/enc/key.bin" | "/s/enc2/key.bin" => reply(req, 200, "application/octet-stream", KEY.to_vec()),
        "/s/fmp4/init.mp4" => reply(req, 200, "video/mp4", vec![0xAA; 500]),
        "/s/dash.mpd" => reply(
            req,
            200,
            "application/dash+xml",
            r#"<MPD type="static" mediaPresentationDuration="PT12S"><Period>
              <AdaptationSet mimeType="video/mp4"><SegmentTemplate timescale="1" duration="4" initialization="v/init.mp4" media="v/$Number$.m4s"/><Representation id="v1" bandwidth="900000" height="720"/></AdaptationSet>
              <AdaptationSet mimeType="audio/mp4"><SegmentTemplate timescale="1" duration="4" initialization="a/init.mp4" media="a/$Number$.m4s"/><Representation id="a1" bandwidth="128000"/></AdaptationSet>
            </Period></MPD>"#
                .into(),
        ),
        p if p.starts_with("/s/dash/") || p.starts_with("/s/v/") || p.starts_with("/s/a/") => {
            // DASH pieces live next to the manifest: /s/v/init.mp4, /s/v/1.m4s ...
            let tag = if p.starts_with("/s/a/") { 90 } else { 40 };
            if p.ends_with("init.mp4") { reply(req, 200, "video/mp4", vec![tag; 300]) } else { reply(req, 200, "video/mp4", seg_bytes(tag, p.rsplit('/').next().and_then(|f| f.split('.').next()).and_then(|n| n.parse().ok()).unwrap_or(0))) }
        }
        p if p.ends_with(".ts") || p.ends_with(".m4s") => {
            let dir = p.rsplit_once('/').map(|x| x.0).unwrap_or("");
            let i = seg_no(p);
            let tag = match dir {
                "/s/v720" => 10,
                "/s/v360" => 20,
                "/s/fmp4" => 60,
                "/s/slow" => 70,
                "/s/flaky" => 80,
                _ => 30,
            };
            if dir == "/s/flaky" && i == 1 && flaky.fetch_add(1, Relaxed) < 2 {
                return reply(req, 503, "text/plain", vec![]); // temporary failure, must be retried
            }
            if dir == "/s/slow" {
                thread::sleep(Duration::from_millis(250));
            }
            let plain = seg_bytes(tag, i);
            match dir {
                "/s/enc" => reply(req, 200, "video/mp2t", encrypt(&plain, IV)),
                "/s/enc2" => {
                    let mut iv = [0u8; 16];
                    iv[8..].copy_from_slice(&(i as u64).to_be_bytes());
                    reply(req, 200, "video/mp2t", encrypt(&plain, iv))
                }
                _ => reply(req, 200, "video/mp2t", plain),
            }
        }
        _ => reply(req, 404, "text/plain", vec![]),
    }
}

fn tmp(name: &str) -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let p = std::env::temp_dir().join(format!("dm-stream-{name}-{}-{n}", std::process::id()));
    fs::create_dir_all(&p).unwrap();
    p
}

/// No ffmpeg on purpose: results must not depend on what is installed on the machine running the tests.
fn opts() -> Options {
    Options { connections: 4, ffmpeg_path: Some(PathBuf::from("no-such-ffmpeg.exe")), ..Options::default() }
}

async fn run(url: &str, dir: &Path, o: &Options) -> Result<PathBuf, Error> {
    download(url, dir, o, &Control::new(o)).await
}

/// Compare big byte buffers without printing them: length, then the first position that differs.
fn same(got: &[u8], want: &[u8], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: length");
    if let Some(i) = got.iter().zip(want).position(|(a, b)| a != b) {
        panic!("{what}: first difference at byte {i}");
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn master_playlist_downloads_the_best_quality_in_order() {
    let (srv, dir) = (serve(), tmp("best"));
    let p = run(&srv.url("/s/master.m3u8"), &dir, &opts()).await.unwrap();
    assert_eq!(p.file_name().unwrap(), "master.ts");
    same(&fs::read(&p).unwrap(), &concat(10, 6), "720p pieces, joined in playlist order");
    assert_eq!(names(&dir), ["master.ts"], "no leftovers");
}

#[tokio::test(flavor = "multi_thread")]
async fn quality_cap_and_custom_name() {
    let (srv, dir) = (serve(), tmp("cap"));
    let o = Options { max_height: Some(480), file_name: Some("My Show: part 1.m3u8".into()), ..opts() };
    let p = run(&srv.url("/s/master.m3u8"), &dir, &o).await.unwrap();
    assert_eq!(p.file_name().unwrap(), "My Show_ part 1.ts");
    same(&fs::read(&p).unwrap(), &concat(20, 3), "360p pieces");
    let again = run(&srv.url("/s/master.m3u8"), &dir, &o).await.unwrap();
    assert_eq!(again.file_name().unwrap(), "My Show_ part 1 (1).ts", "a second copy gets a number");
}

#[tokio::test(flavor = "multi_thread")]
async fn aes128_streams_are_decrypted_with_explicit_and_implicit_iv() {
    let (srv, dir) = (serve(), tmp("aes"));
    let a = run(&srv.url("/s/enc/index.m3u8"), &dir, &opts()).await.unwrap();
    same(&fs::read(&a).unwrap(), &concat(30, 4), "a");
    let dir2 = tmp("aes2");
    let b = run(&srv.url("/s/enc2/index.m3u8"), &dir2, &opts()).await.unwrap();
    same(&fs::read(&b).unwrap(), &concat(30, 4), "IV taken from the piece number");
    assert_eq!(srv.count("/s/enc/key.bin"), 1, "the key is fetched once, not once per piece");
}

#[tokio::test(flavor = "multi_thread")]
async fn init_segment_comes_first_and_gives_an_mp4() {
    let (srv, dir) = (serve(), tmp("fmp4"));
    let p = run(&srv.url("/s/fmp4/index.m3u8"), &dir, &opts()).await.unwrap();
    assert_eq!(p.extension().unwrap(), "mp4");
    let mut want = vec![0xAA; 500];
    want.extend(concat(60, 3));
    same(&fs::read(&p).unwrap(), &want, "p");
}

#[tokio::test(flavor = "multi_thread")]
async fn temporary_server_errors_are_retried() {
    let (srv, dir) = (serve(), tmp("flaky"));
    let p = run(&srv.url("/s/flaky/index.m3u8"), &dir, &opts()).await.unwrap();
    same(&fs::read(&p).unwrap(), &concat(80, 3), "p");
    assert!(srv.count("/s/flaky/seg1.ts") >= 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn live_and_protected_streams_are_refused_clearly() {
    let (srv, dir) = (serve(), tmp("refuse"));
    let live = run(&srv.url("/s/live/index.m3u8"), &dir, &opts()).await;
    assert!(matches!(&live, Err(Error::Invalid(m)) if m.contains("live")), "{live:?}");
    let drm = run(&srv.url("/s/drm/index.m3u8"), &dir, &opts()).await;
    assert!(matches!(&drm, Err(Error::Invalid(m)) if m.contains("protected")), "{drm:?}");
    assert!(names(&dir).is_empty(), "nothing is left behind after a refusal");
}

#[tokio::test(flavor = "multi_thread")]
async fn stopped_download_keeps_finished_pieces_and_resumes() {
    let (srv, dir) = (serve(), tmp("resume"));
    let o = Options { connections: 2, ..opts() };
    let ctl = Control::new(&o);
    let cancel = ctl.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        cancel.store(true, Relaxed);
    });
    let r = download(&srv.url("/s/slow/index.m3u8"), &dir, &o, &ctl).await;
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
    let parts: Vec<_> = fs::read_dir(dir.join("index.parts/video")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".seg")).collect();
    assert!(!parts.is_empty() && parts.len() < 8, "some pieces done, not all: {parts:?}");

    let ctl2 = Control::new(&o);
    let p = download(&srv.url("/s/slow/index.m3u8"), &dir, &o, &ctl2).await.unwrap();
    same(&fs::read(&p).unwrap(), &concat(70, 8), "p");
    for n in &parts {
        let i: usize = n.trim_end_matches(".seg").parse().unwrap();
        assert_eq!(srv.count(&format!("/s/slow/seg{i}.ts")), 1, "finished piece {i} must not be fetched again");
    }
    assert_eq!(names(&dir), ["index.ts"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn dash_video_and_audio_without_ffmpeg_are_kept_as_two_files() {
    let (srv, dir) = (serve(), tmp("dash"));
    let o = opts();
    let ctl = Control::new(&o);
    let p = download(&srv.url("/s/dash.mpd"), &dir, &o, &ctl).await.unwrap();
    assert_eq!(p.file_name().unwrap(), "dash.mp4");
    let mut video = vec![40; 300];
    video.extend((1..=3).flat_map(|i| seg_bytes(40, i)));
    same(&fs::read(&p).unwrap(), &video, "p");
    let mut audio = vec![90; 300];
    audio.extend((1..=3).flat_map(|i| seg_bytes(90, i)));
    assert_eq!(fs::read(dir.join("dash.audio.m4a")).unwrap(), audio);
    assert!(ctl.progress.note.lock().unwrap().contains("separate files"));
}

#[tokio::test(flavor = "multi_thread")]
async fn progress_reports_real_numbers_for_streams() {
    let (srv, dir) = (serve(), tmp("progress"));
    let o = opts();
    let ctl = Control::new(&o);
    download(&srv.url("/s/v720/index.m3u8"), &dir, &o, &ctl).await.unwrap();
    let (d, t) = (ctl.progress.downloaded.load(Relaxed), ctl.progress.total.load(Relaxed));
    assert_eq!(d, (6 * SEG) as u64);
    assert_eq!(t, d, "the estimate settles on the real size once everything is in");
    assert_eq!(*ctl.progress.name.lock().unwrap(), "index.ts");
}

#[tokio::test(flavor = "multi_thread")]
async fn manager_handles_a_stream_link_like_any_other() {
    let (srv, root) = (serve(), tmp("mgr"));
    let m = Manager::open(root.join("data"), root.join("dl")).unwrap();
    // the tiny_http test server has a small thread pool that idle keep-alive connections can fill up
    m.set_settings(Settings { connections: 3, ..m.settings() });
    let id = m.add(&srv.url("/s/v720/index.m3u8"), &AddOpts { task: dm_core::TaskOpts { filename: Some("Holiday".into()), ..Default::default() }, ..AddOpts::default() }).unwrap();
    let t = Instant::now();
    while m.get(id).unwrap().task.status != Status::Done {
        assert!(t.elapsed() < Duration::from_secs(20), "stream did not finish: {:?}", m.get(id).unwrap().task);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let task = m.get(id).unwrap().task;
    assert_eq!(task.category, "Video");
    assert_eq!(task.name, "Holiday.ts");
    assert_eq!(fs::read(root.join("dl/Video/Holiday.ts")).unwrap(), concat(10, 6));
    assert_eq!(m.list(&Filter { search: Some("holiday".into()), ..Filter::default() }).len(), 1);
    m.shutdown().await;
}
