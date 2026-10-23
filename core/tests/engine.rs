//! End-to-end tests against a local server that can do ranges, drop connections, redirect, gate on cookies and throttle.
use dm_core::{download, Control, Error, Options};
use sha2::{Digest, Sha256};
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{fs, thread};
use tiny_http::{Header, Request, Response, Server, StatusCode};

fn pat(i: u64) -> u8 {
    (i.wrapping_mul(31) % 251) as u8
}

struct Pat {
    pos: u64,
    end: u64, // exclusive
    delay: Option<Duration>,
    fail_after: Option<u64>,
    served: u64,
}

impl Read for Pat {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.end {
            return Ok(0);
        }
        if self.fail_after.is_some_and(|f| self.served >= f) {
            return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "cut"));
        }
        if let Some(d) = self.delay {
            thread::sleep(d);
        }
        let n = buf.len().min(16384).min((self.end - self.pos) as usize);
        for (k, b) in buf[..n].iter_mut().enumerate() {
            *b = pat(self.pos + k as u64);
        }
        self.pos += n as u64;
        self.served += n as u64;
        Ok(n)
    }
}

fn h(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn parse_range(v: &str, size: u64) -> Option<(u64, u64)> {
    let (a, b) = v.strip_prefix("bytes=")?.split_once('-')?;
    let a: u64 = a.parse().ok()?;
    let b = if b.is_empty() { size - 1 } else { b.parse::<u64>().ok()?.min(size - 1) };
    Some((a, b))
}

struct Srv {
    port: u16,
    hits: Arc<AtomicUsize>,
}

impl Srv {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

fn serve(size: u64) -> Srv {
    let server = Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let flaky = Arc::new(AtomicUsize::new(0));
    let hits2 = hits.clone();
    thread::spawn(move || {
        for req in server.incoming_requests() {
            let (hits, flaky) = (hits2.clone(), flaky.clone());
            thread::spawn(move || handle(req, size, hits, flaky));
        }
    });
    Srv { port, hits }
}

fn handle(req: Request, size: u64, hits: Arc<AtomicUsize>, flaky: Arc<AtomicUsize>) {
    hits.fetch_add(1, Relaxed);
    let path = req.url().to_string();
    let header = |name: &'static str| req.headers().iter().find(|x| x.field.equiv(name)).map(|x| x.value.as_str().to_string());
    if path == "/redirect" {
        let _ = req.respond(Response::empty(StatusCode(302)).with_header(h("Location", "/data")));
        return;
    }
    if path == "/cookie" && !header("Cookie").is_some_and(|c| c.contains("k=v")) {
        let _ = req.respond(Response::empty(StatusCode(403)));
        return;
    }
    if path == "/missing" {
        let _ = req.respond(Response::empty(StatusCode(404)));
        return;
    }
    let range = if path == "/norange" { None } else { header("Range").and_then(|r| parse_range(&r, size)) };
    let (start, end) = range.unwrap_or((0, size - 1));
    let delay = match path.as_str() {
        "/slow" => Some(Duration::from_millis(3)),
        "/skew" if start == 0 => Some(Duration::from_millis(8)),
        _ => None,
    };
    let fail_after = (path == "/flaky" && flaky.fetch_add(1, Relaxed) >= 1 && flaky.load(Relaxed) <= 8).then_some(300_000);
    let len = end - start + 1;
    let mut headers = vec![h("Content-Type", "application/octet-stream")];
    if path != "/norange" {
        headers.push(h("Accept-Ranges", "bytes"));
    }
    if path == "/name" {
        headers.push(h("Content-Disposition", "attachment; filename=\"my file.bin\""));
    }
    let status = if range.is_some() {
        headers.push(h("Content-Range", &format!("bytes {start}-{end}/{size}")));
        206
    } else {
        200
    };
    let body = Pat { pos: start, end: end + 1, delay, fail_after, served: 0 };
    let _ = req.respond(Response::new(StatusCode(status), headers, body, Some(len as usize), None));
}

fn expected(size: u64) -> String {
    let mut s = Sha256::new();
    let mut buf = vec![0u8; 65536];
    let mut pos = 0;
    while pos < size {
        let n = ((size - pos) as usize).min(buf.len());
        for (k, b) in buf[..n].iter_mut().enumerate() {
            *b = pat(pos + k as u64);
        }
        s.update(&buf[..n]);
        pos += n as u64;
    }
    s.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn sha_file(p: &std::path::Path) -> String {
    let mut s = Sha256::new();
    s.update(fs::read(p).unwrap());
    s.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn tmp(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let p = std::env::temp_dir().join(format!("dm-test-{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&p).unwrap();
    p
}

fn opts() -> Options {
    Options { connections: 4, min_split: 256 * 1024, save_interval: Duration::from_millis(20), ..Options::default() }
}

async fn run(url: &str, dir: &std::path::Path, o: &Options) -> Result<PathBuf, Error> {
    download(url, dir, o, &Control::new(o)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn ranged_download_is_byte_exact() {
    let size = 5_000_123;
    let (srv, dir) = (serve(size), tmp("ranged"));
    let path = run(&srv.url("/data"), &dir, &opts()).await.unwrap();
    assert_eq!(path.file_name().unwrap(), "data");
    assert_eq!(sha_file(&path), expected(size));
    assert!(!dir.join("data.part").exists() && !dir.join("data.part.json").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn server_without_range_falls_back_to_one_stream() {
    let size = 1_500_000;
    let (srv, dir) = (serve(size), tmp("norange"));
    let path = run(&srv.url("/norange"), &dir, &opts()).await.unwrap();
    assert_eq!(sha_file(&path), expected(size));
}

#[tokio::test(flavor = "multi_thread")]
async fn dropped_connections_are_retried() {
    let size = 3_000_000;
    let (srv, dir) = (serve(size), tmp("flaky"));
    let o = Options { retries: 12, ..opts() };
    let path = run(&srv.url("/flaky"), &dir, &o).await.unwrap();
    assert_eq!(sha_file(&path), expected(size));
}

#[tokio::test(flavor = "multi_thread")]
async fn redirect_and_content_disposition_name() {
    let size = 400_000;
    let (srv, dir) = (serve(size), tmp("names"));
    let a = run(&srv.url("/redirect"), &dir, &opts()).await.unwrap();
    assert_eq!(a.file_name().unwrap(), "data");
    let b = run(&srv.url("/name"), &dir, &opts()).await.unwrap();
    assert_eq!(b.file_name().unwrap(), "my file.bin");
    let c = run(&srv.url("/name"), &dir, &opts()).await.unwrap(); // same name again -> numbered
    assert_eq!(c.file_name().unwrap(), "my file (1).bin");
}

#[tokio::test(flavor = "multi_thread")]
async fn cookie_gate_and_http_errors() {
    let size = 300_000;
    let (srv, dir) = (serve(size), tmp("cookie"));
    assert!(matches!(run(&srv.url("/cookie"), &dir, &opts()).await, Err(Error::Server(_))));
    let o = Options { cookie: Some("k=v".into()), ..opts() };
    let path = run(&srv.url("/cookie"), &dir, &o).await.unwrap();
    assert_eq!(sha_file(&path), expected(size));
    assert!(matches!(run(&srv.url("/missing"), &dir, &opts()).await, Err(Error::Server(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_then_resume_gives_identical_file() {
    let size = 4_000_000;
    let (srv, dir) = (serve(size), tmp("resume"));
    let o = Options { connections: 2, ..opts() };
    let ctl = Control::new(&o);
    let cancel = ctl.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        cancel.store(true, Relaxed);
    });
    let r = download(&srv.url("/slow"), &dir, &o, &ctl).await;
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
    let state = fs::read_to_string(dir.join("slow.part.json")).expect("progress must be saved on cancel");
    assert!(state.contains("\"pos\""));

    let ctl2 = Control::new(&o);
    let path = download(&srv.url("/slow"), &dir, &o, &ctl2).await.unwrap();
    assert_eq!(sha_file(&path), expected(size));
    assert!(!dir.join("slow.part.json").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn checksum_is_enforced() {
    let size = 600_000;
    let (srv, dir) = (serve(size), tmp("sha"));
    let bad = Options { sha256: Some("00".repeat(32)), ..opts() };
    assert!(matches!(run(&srv.url("/data"), &dir, &bad).await, Err(Error::Checksum)));
    assert!(!dir.join("data").exists());
    let good = Options { sha256: Some(expected(size)), ..opts() };
    assert!(run(&srv.url("/data"), &dir, &good).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn speed_limit_is_respected() {
    let size = 1_000_000;
    let (srv, dir) = (serve(size), tmp("limit"));
    let o = Options { speed_limit: Some(500_000), ..opts() };
    let t = Instant::now();
    run(&srv.url("/data"), &dir, &o).await.unwrap();
    assert!(t.elapsed() >= Duration::from_millis(1400), "finished in {:?}", t.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
async fn idle_connections_split_the_slowest_segment() {
    let size = 6_000_000;
    let (srv, dir) = (serve(size), tmp("skew"));
    let o = Options { connections: 3, min_split: 512 * 1024, ..opts() };
    let path = run(&srv.url("/skew"), &dir, &o).await.unwrap();
    assert_eq!(sha_file(&path), expected(size));
    // probe + 3 first requests = 4; anything above means a fast connection took over part of the slow segment
    assert!(srv.hits.load(Relaxed) > 4, "hits = {}", srv.hits.load(Relaxed));
}
