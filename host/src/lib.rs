//! Native messaging relay: the browser extension talks to this tiny exe over stdio,
//! and it forwards a short allow-list of calls to the background app's local API.
//! The extension never learns the API token, and a compromised extension cannot reach anything outside the list.
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Config {
    pub data_dir: PathBuf,
    pub product_id: String,
    /// The background app to start when it is not running.
    pub app_path: PathBuf,
    /// How long to wait for a freshly started app.
    pub start_wait: Duration,
}

// ---- framing: 4-byte little-endian length + JSON ----

pub fn read_msg<R: Read>(r: &mut R) -> io::Result<Option<Value>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > 64 * 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn write_msg<W: Write>(w: &mut W, v: &Value) -> io::Result<()> {
    let b = v.to_string().into_bytes();
    w.write_all(&(b.len() as u32).to_le_bytes())?;
    w.write_all(&b)?;
    w.flush()
}

// ---- talking to the app ----

fn http(port: u16, token: Option<&str>, method: &str, path: &str, body: Option<&str>) -> io::Result<(u16, String)> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(800))?;
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    let body = body.unwrap_or("");
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    let text = String::from_utf8_lossy(&raw);
    let (head, rest) = text.split_once("\r\n\r\n").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no http body"))?;
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no status"))?;
    // axum answers with Content-Length; chunked bodies are not expected for these endpoints
    Ok((status, rest.to_string()))
}

fn running(cfg: &Config) -> Option<(u16, String)> {
    let info: Value = serde_json::from_slice(&std::fs::read(cfg.data_dir.join("api.json")).ok()?).ok()?;
    let (port, token) = (info["port"].as_u64()? as u16, info["token"].as_str()?.to_string());
    let (code, body) = http(port, None, "GET", "/api/ping", None).ok()?;
    (code == 200 && body.contains(&format!("\"app\":\"{}\"", cfg.product_id))).then_some((port, token))
}

fn spawn_app(path: &Path, args: &[&str]) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP: the app must outlive the browser and this relay
        Command::new(path).args(args).creation_flags(0x0000_0008 | 0x0000_0200).spawn().map(|_| ())
    }
    #[cfg(not(windows))]
    Command::new(path).args(args).spawn().map(|_| ())
}

fn ensure_running(cfg: &Config) -> Option<(u16, String)> {
    if let Some(x) = running(cfg) {
        return Some(x);
    }
    spawn_app(&cfg.app_path, &["--background"]).ok()?;
    let tries = (cfg.start_wait.as_millis() / 150).max(1);
    for _ in 0..tries {
        std::thread::sleep(Duration::from_millis(150));
        if let Some(x) = running(cfg) {
            return Some(x);
        }
    }
    None
}

/// The only calls the extension may make. Everything else is refused.
fn allowed(method: &str, path: &str) -> bool {
    matches!((method, path), ("GET", "/api/ping") | ("GET", "/api/settings") | ("POST", "/api/capture") | ("POST", "/api/tasks"))
}

fn fail(id: &Value, error: &str) -> Value {
    json!({"id": id, "ok": false, "error": error})
}

pub fn handle(cfg: &Config, msg: &Value) -> Value {
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    match msg["cmd"].as_str().unwrap_or("") {
        "hello" => {
            // extension and relay are released together; a mismatch means one of them is stale
            let theirs = msg["version"].as_str().unwrap_or("");
            if theirs != VERSION {
                return json!({"id": id, "ok": false, "error": "version_mismatch", "host_version": VERSION, "extension_version": theirs});
            }
            match ensure_running(cfg) {
                Some((port, _)) => {
                    let app = http(port, None, "GET", "/api/ping", None).ok().and_then(|(_, b)| serde_json::from_str::<Value>(&b).ok()).unwrap_or(Value::Null);
                    json!({"id": id, "ok": true, "host_version": VERSION, "app": app})
                }
                None => fail(&id, "app_not_running"),
            }
        }
        "open_ui" => match spawn_app(&cfg.app_path, &[]) {
            Ok(()) => json!({"id": id, "ok": true}),
            Err(_) => fail(&id, "app_not_found"),
        },
        "api" => {
            let (method, path) = (msg["method"].as_str().unwrap_or(""), msg["path"].as_str().unwrap_or(""));
            if !allowed(method, path) {
                return fail(&id, "not_allowed");
            }
            let Some((port, token)) = ensure_running(cfg) else { return fail(&id, "app_not_running") };
            let body = msg.get("body").filter(|b| !b.is_null()).map(|b| b.to_string());
            match http(port, Some(&token), method, path, body.as_deref()) {
                Ok((status, text)) => json!({"id": id, "ok": true, "status": status, "body": serde_json::from_str::<Value>(&text).unwrap_or(Value::Null)}),
                Err(_) => fail(&id, "app_not_running"),
            }
        }
        _ => fail(&id, "unknown_command"),
    }
}

/// Serve one browser connection until it closes stdin.
pub fn serve<R: Read, W: Write>(cfg: &Config, mut input: R, mut output: W) -> io::Result<()> {
    while let Some(msg) = read_msg(&mut input)? {
        write_msg(&mut output, &handle(cfg, &msg))?;
    }
    Ok(())
}
