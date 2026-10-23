//! Small Windows helpers: open files, start with Windows, open the UI window.
use std::path::Path;
use std::process::Command;

#[cfg(windows)]
const NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn quiet(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(NO_WINDOW)
}

#[cfg(windows)]
pub fn open_file(p: &Path) {
    let _ = Command::new("explorer").arg(p).spawn();
}

#[cfg(windows)]
pub fn reveal(p: &Path) {
    use std::os::windows::process::CommandExt;
    let _ = Command::new("explorer").raw_arg(format!("/select,\"{}\"", p.display())).spawn();
}

/// HKCU Run entry, so no admin rights, no service and no scheduled task are needed.
#[cfg(windows)]
pub fn set_autostart(product_id: &str, on: bool) {
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let mut c = Command::new("reg");
    if on {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        c.args(["add", KEY, "/v", product_id, "/t", "REG_SZ", "/d", &format!("\"{exe}\" --background"), "/f"]);
    } else {
        c.args(["delete", KEY, "/v", product_id, "/f"]);
    }
    let _ = quiet(&mut c).output();
}

/// Open `url` in an app-style window (no tabs or address bar) using Edge or Chrome, else the default browser.
#[cfg(windows)]
pub fn open_ui(url: &str) {
    let pf = std::env::var("ProgramFiles").unwrap_or_default();
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let candidates = [
        format!(r"{pf86}\Microsoft\Edge\Application\msedge.exe"),
        format!(r"{pf}\Microsoft\Edge\Application\msedge.exe"),
        format!(r"{pf}\Google\Chrome\Application\chrome.exe"),
        format!(r"{pf86}\Google\Chrome\Application\chrome.exe"),
        format!(r"{local}\Google\Chrome\Application\chrome.exe"),
    ];
    for exe in candidates.iter().filter(|p| Path::new(p).exists()) {
        if Command::new(exe).arg(format!("--app={url}")).spawn().is_ok() {
            return;
        }
    }
    let _ = Command::new("explorer").arg(url).spawn();
}

#[cfg(not(windows))]
pub fn open_file(_: &Path) {}
#[cfg(not(windows))]
pub fn reveal(_: &Path) {}
#[cfg(not(windows))]
pub fn set_autostart(_: &str, _: bool) {}
#[cfg(not(windows))]
pub fn open_ui(_: &str) {}
