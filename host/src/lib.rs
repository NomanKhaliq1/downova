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

/// Ask a running app to save and exit (used by the installer before upgrading or removing). True when it is gone.
pub fn quit_app(cfg: &Config) -> bool {
    let Some((port, token)) = running(cfg) else { return true };
    let _ = http(port, Some(&token), "POST", "/api/quit", Some("{}"));
    (0..80).any(|_| {
        std::thread::sleep(Duration::from_millis(150));
        running(cfg).is_none()
    })
}

/// The only calls the extension may make. Everything else is refused.
fn allowed(method: &str, path: &str) -> bool {
    if matches!((method, path), ("GET", "/api/ping") | ("GET", "/api/settings") | ("POST", "/api/capture") | ("POST", "/api/tasks") | ("GET", "/api/tools")) {
        return true;
    }
    // one task by number: read it, pause it, resume it (for the extension's small progress window)
    let Some(rest) = path.strip_prefix("/api/tasks/") else { return false };
    let (id, act) = rest.split_once('/').unwrap_or((rest, ""));
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && matches!((method, act), ("GET", "") | ("POST", "pause") | ("POST", "resume"))
}

/// Windows "Save As" window for the extension's download dialog. Empty string when the user cancels.
/// shortcut: blocks this relay while open (other extension calls wait), move to a thread if that hurts.
fn pick_path(start: &str) -> io::Result<String> {
    const SCRIPT: &str = "[Console]::OutputEncoding=[Text.Encoding]::UTF8;Add-Type -AssemblyName System.Windows.Forms;\
        $d=New-Object System.Windows.Forms.SaveFileDialog;$d.OverwritePrompt=$false;$p=$env:DM_PICK;\
        if($p){$d.FileName=[IO.Path]::GetFileName($p);$dir=[IO.Path]::GetDirectoryName($p);if($dir -and (Test-Path -LiteralPath $dir)){$d.InitialDirectory=$dir}};\
        $f=New-Object System.Windows.Forms.Form -Property @{TopMost=$true;ShowInTaskbar=$false};\
        if($d.ShowDialog($f) -eq 'OK'){[Console]::Out.Write($d.FileName)}";
    powershell(SCRIPT, start)
}

/// Windows' icon for a file type (32 px PNG, base64), taken from an empty temp file with that ending.
fn file_icon(ext: &str) -> io::Result<String> {
    if ext.is_empty() || ext.len() > 10 || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad extension"));
    }
    const SCRIPT: &str = "Add-Type -AssemblyName System.Drawing;$t=Join-Path $env:TEMP ('downova-icon.'+$env:DM_PICK);\
        [IO.File]::WriteAllBytes($t,[byte[]]@());try{$b=[System.Drawing.Icon]::ExtractAssociatedIcon($t).ToBitmap();\
        $m=New-Object IO.MemoryStream;$b.Save($m,[System.Drawing.Imaging.ImageFormat]::Png);[Console]::Out.Write([Convert]::ToBase64String($m.ToArray()))}\
        finally{Remove-Item -LiteralPath $t -ErrorAction SilentlyContinue}";
    powershell(SCRIPT, ext)
}

/// Run a short PowerShell script; `arg` reaches it as $env:DM_PICK, never inside the script text.
fn powershell(script: &str, arg: &str) -> io::Result<String> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-STA", "-Command", script]).env("DM_PICK", arg);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    // output() gives the child no stdin, so it cannot eat the browser's messages
    let out = cmd.output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
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
        "open_ui" => match {
            // only known views may be asked for; the extension cannot pass anything else on a command line
            let view = msg["view"].as_str().filter(|v| matches!(*v, "main" | "add" | "streams"));
            match view {
                Some(v) => spawn_app(&cfg.app_path, &["--view", v]),
                None => spawn_app(&cfg.app_path, &[]),
            }
        } {
            Ok(()) => json!({"id": id, "ok": true}),
            Err(_) => fail(&id, "app_not_found"),
        },
        "pick_path" => match pick_path(msg["path"].as_str().unwrap_or("")) {
            Ok(path) => json!({"id": id, "ok": true, "path": path}),
            Err(_) => fail(&id, "dialog_failed"),
        },
        "file_icon" => match file_icon(msg["ext"].as_str().unwrap_or("")) {
            Ok(png) => json!({"id": id, "ok": true, "png": png}),
            Err(_) => fail(&id, "no_icon"),
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
