//! Setting up and checking the optional helper tool yt-dlp. Nothing here runs unless the user asks for it.
use crate::error::{Error, Result};
use crate::ytdlp::EXE;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const RELEASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";

fn hidden(c: &mut Command) -> &mut Command {
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    c
}

/// `yt-dlp --version`, e.g. "2026.10.01"; None when it cannot be run.
pub async fn ytdlp_version(path: &Path) -> Option<String> {
    let mut c = Command::new(path);
    c.arg("--version").kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(20), hidden(&mut c).output()).await.ok()?.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|v| !v.is_empty())
}

/// The line for `file` in a SHA2-256SUMS text ("<hash>  <file>").
pub fn expected_hash(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut p = l.split_whitespace();
        let (h, f) = (p.next()?, p.next()?);
        (f.trim_start_matches('*') == file && h.len() == 64).then(|| h.to_ascii_lowercase())
    })
}

/// Download the latest yt-dlp.exe from the project's GitHub releases into `dir`, after checking its SHA-256
/// against the checksum list published with the release. Returns the path of the installed file.
pub async fn install_ytdlp(dir: &Path) -> Result<PathBuf> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(300)).connect_timeout(Duration::from_secs(20)).build()?;
    let get = |name: &str| {
        let c = client.clone();
        let url = format!("{RELEASE}/{name}");
        async move {
            let r = c.get(&url).send().await?;
            if !r.status().is_success() {
                return Err(Error::Server(format!("{url}: {}", r.status())));
            }
            Ok::<_, Error>(r.bytes().await?)
        }
    };
    let sums = get("SHA2-256SUMS").await?;
    let want = expected_hash(&String::from_utf8_lossy(&sums), EXE).ok_or_else(|| Error::Server("the release has no checksum for yt-dlp.exe".into()))?;
    let exe = get(EXE).await?;
    let got: String = Sha256::digest(&exe).iter().map(|b| format!("{b:02x}")).collect();
    if got != want {
        return Err(Error::Checksum);
    }
    std::fs::create_dir_all(dir)?;
    let (tmp, dest) = (dir.join(format!("{EXE}.part")), dir.join(EXE));
    std::fs::write(&tmp, &exe)?;
    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}

/// `yt-dlp -U` (YouTube changes often; the tool updates itself). Returns the version afterwards.
pub async fn update_ytdlp(path: &Path) -> Result<String> {
    let mut c = Command::new(path);
    c.arg("-U").kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(120), hidden(&mut c).output()).await.map_err(|_| Error::Server("the update took too long".into()))??;
    if !out.status.success() {
        return Err(Error::Server(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("update failed").to_string()));
    }
    ytdlp_version(path).await.ok_or_else(|| Error::Server("yt-dlp does not start after the update".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_checksum_line_for_the_exe() {
        let h = "a".repeat(64);
        let sums = format!("{}  yt-dlp\n{}  yt-dlp.exe\n{}  yt-dlp_x86.exe\n", "b".repeat(64), h.to_uppercase(), "c".repeat(64));
        assert_eq!(expected_hash(&sums, "yt-dlp.exe"), Some(h));
        assert_eq!(expected_hash(&sums, "missing.exe"), None);
        assert_eq!(expected_hash("short  yt-dlp.exe", "yt-dlp.exe"), None, "a hash of the wrong length is refused");
    }
}
