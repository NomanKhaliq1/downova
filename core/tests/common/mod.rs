//! Local test server (ranges, dropped connections, redirects, cookie gate, throttling) shared by the test files.
#![allow(dead_code)]
use dm_core::{download, Control, Error, Options};
use sha2::{Digest, Sha256};
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Duration;
use std::{fs, thread};
use tiny_http::{Header, Request, Response, Server, StatusCode};

pub fn pat(i: u64) -> u8 {
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

pub fn h(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

pub fn parse_range(v: &str, size: u64) -> Option<(u64, u64)> {
    let (a, b) = v.strip_prefix("bytes=")?.split_once('-')?;
    let a: u64 = a.parse().ok()?;
    let b = if b.is_empty() { size - 1 } else { b.parse::<u64>().ok()?.min(size - 1) };
    Some((a, b))
}

pub struct Srv {
    pub port: u16,
    pub hits: Arc<AtomicUsize>,
}

impl Srv {
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

pub fn serve(size: u64) -> Srv {
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

pub fn handle(req: Request, size: u64, hits: Arc<AtomicUsize>, flaky: Arc<AtomicUsize>) {
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
        p if p.starts_with("/slow") => Some(Duration::from_millis(3)),
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

pub fn expected(size: u64) -> String {
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

pub fn sha_file(p: &std::path::Path) -> String {
    let mut s = Sha256::new();
    s.update(fs::read(p).unwrap());
    s.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn tmp(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let p = std::env::temp_dir().join(format!("dm-test-{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&p).unwrap();
    p
}

pub fn opts() -> Options {
    Options { connections: 4, min_split: 256 * 1024, save_interval: Duration::from_millis(20), ..Options::default() }
}

pub async fn run(url: &str, dir: &std::path::Path, o: &Options) -> Result<PathBuf, Error> {
    download(url, dir, o, &Control::new(o)).await
}
