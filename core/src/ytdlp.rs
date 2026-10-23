//! Pages like YouTube cannot be downloaded as a plain file: the video lives in signed pieces that change all the time.
//! For those the optional helper tool yt-dlp (https://github.com/yt-dlp/yt-dlp, public domain) does the extraction and
//! we run it, read its progress and report it like any other download. It is never bundled: the user sets it up on purpose.
use crate::engine::{Control, Options};
use crate::error::{Error, Result};
use crate::stream::find_ffmpeg;
use crate::util::sanitize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub const EXE: &str = "yt-dlp.exe";

/// Settings path, then the app's own tools folder, then next to the app, then PATH.
pub fn find(o: &Options) -> Option<PathBuf> {
    if let Some(p) = &o.ytdlp_path {
        return p.is_file().then(|| p.clone());
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    let own = o.tools_dir.iter().map(|d| d.join(EXE));
    let beside = exe_dir.into_iter().flat_map(|d| [d.join(EXE), d.join("tools").join(EXE)]);
    let on_path = std::env::var_os("PATH").into_iter().flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).flat_map(|d| [d.join(EXE), d.join("yt-dlp")]);
    own.chain(beside).chain(on_path).find(|p| p.is_file())
}

/// The command line for one download. Separate from `run` so it can be tested without running anything.
pub fn args(url: &str, dir: &Path, o: &Options, ffmpeg: Option<&Path>) -> Vec<String> {
    let h = o.max_height.map(|h| format!("[height<={h}]")).unwrap_or_default();
    let format = if o.audio_only {
        "ba/b".to_string()
    } else if ffmpeg.is_some() {
        format!("bv*{h}+ba/b{h}/bv*+ba/b")
    } else {
        format!("b{h}/b") // without ffmpeg only ready-made single files can be taken
    };
    let name = match &o.file_name {
        Some(n) => {
            let s = sanitize(n);
            let stem = s.rsplit_once('.').filter(|(a, e)| !a.is_empty() && e.len() <= 4).map_or(s.as_str(), |(a, _)| a).to_string();
            stem.replace('%', "%%") // a literal percent sign, not a yt-dlp field
        }
        None => "%(title).150B [%(id)s]".to_string(),
    };
    let mut a: Vec<String> = ["--no-playlist", "--no-colors", "--no-warnings", "--newline", "--progress", "--windows-filenames", "--no-mtime", "--ignore-config"].map(String::from).to_vec();
    a.extend(["-f".into(), format, "-o".into(), dir.join(format!("{name}.%(ext)s")).to_string_lossy().into_owned()]);
    a.extend(["--progress-template".into(), "download:DMPROG|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s".into()]);
    a.extend(["--print".into(), "pre_process:DMTITLE|%(title)s".into(), "--print".into(), "after_move:DMFILE|%(filepath)s".into()]);
    a.extend(["--concurrent-fragments".into(), o.connections.clamp(1, 16).to_string()]);
    if let Some(ff) = ffmpeg {
        a.extend(["--ffmpeg-location".into(), ff.to_string_lossy().into_owned()]);
        if o.audio_only {
            a.extend(["-x".into(), "--audio-format".into(), "m4a".into()]);
        } else {
            a.extend(["--merge-output-format".into(), "mp4".into()]);
        }
    }
    if let Some(l) = o.speed_limit {
        a.extend(["--limit-rate".into(), l.to_string()]);
    }
    if let Some(p) = &o.proxy {
        a.extend(["--proxy".into(), p.clone()]);
    }
    if let Some(r) = &o.referer {
        a.extend(["--referer".into(), r.clone()]);
    }
    a.push(url.into());
    a
}

fn num(s: &str) -> Option<u64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0).map(|v| v as u64)
}

/// Progress across several streams (video, then audio): when the byte count drops a new stream started.
#[derive(Default)]
pub struct Tally {
    base: u64,
    last: u64,
    last_total: u64,
}

impl Tally {
    /// Feed `DMPROG|downloaded|total|estimate`; returns (downloaded, total) over everything so far.
    pub fn feed(&mut self, line: &str) -> Option<(u64, u64)> {
        let mut f = line.strip_prefix("DMPROG|")?.split('|');
        let d = num(f.next()?)?;
        let t = f.next().and_then(num).or_else(|| f.next().and_then(num)).unwrap_or(0);
        if d < self.last {
            self.base += self.last_total.max(self.last);
        }
        self.last = d;
        self.last_total = t.max(d);
        Some((self.base + d, self.base + self.last_total))
    }
}

fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).creation_flags(0x0800_0000).output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).output();
    }
}

pub async fn run(url: &str, dir: &Path, o: &Options, ctl: &Control) -> Result<PathBuf> {
    let exe = find(o).ok_or_else(|| Error::Invalid("the helper tool yt-dlp is not set up yet (Settings > Streams)".into()))?;
    std::fs::create_dir_all(dir)?;
    let ffmpeg = find_ffmpeg(o);
    let mut cmd = Command::new(&exe);
    cmd.args(args(url, dir, o, ffmpeg.as_deref())).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).env("PYTHONUTF8", "1").env("PYTHONIOENCODING", "utf-8");
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| Error::Server(format!("could not start yt-dlp: {e}")))?;
    let pid = child.id();
    let mut out = BufReader::new(child.stdout.take().expect("piped")).lines();
    let mut err = BufReader::new(child.stderr.take().expect("piped")).lines();
    let (mut tally, mut file, mut errors) = (Tally::default(), None::<PathBuf>, Vec::<String>::new());
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    let (mut out_open, mut err_open) = (true, true);
    while out_open || err_open {
        tokio::select! {
            l = out.next_line(), if out_open => match l {
                Ok(Some(l)) => {
                    if let Some((d, t)) = tally.feed(&l) {
                        ctl.progress.downloaded.store(d, Relaxed);
                        ctl.progress.total.store(t, Relaxed);
                    } else if let Some(t) = l.strip_prefix("DMTITLE|") {
                        *ctl.progress.name.lock().unwrap() = t.trim().to_string();
                    } else if let Some(p) = l.strip_prefix("DMFILE|") {
                        file = Some(PathBuf::from(p.trim()));
                    }
                }
                _ => out_open = false,
            },
            l = err.next_line(), if err_open => match l {
                Ok(Some(l)) => { if !l.trim().is_empty() { errors.push(l); if errors.len() > 20 { errors.remove(0); } } }
                _ => err_open = false,
            },
            _ = tick.tick() => {
                if ctl.cancel.load(Relaxed) {
                    if let Some(pid) = pid { kill_tree(pid); }
                    let _ = child.wait().await;
                    return Err(Error::Cancelled); // yt-dlp keeps its .part files and continues from them next time
                }
            }
        }
    }
    let status = child.wait().await?;
    if !status.success() {
        let why = errors.iter().rev().find(|l| l.contains("ERROR")).or(errors.last()).map(|l| l.trim_start_matches("ERROR: ").to_string()).unwrap_or_else(|| format!("yt-dlp stopped with {status}"));
        return Err(Error::Server(why));
    }
    let path = file.filter(|p| p.is_file()).ok_or_else(|| Error::Server("yt-dlp finished but the file was not found".into()))?;
    *ctl.progress.name.lock().unwrap() = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if ffmpeg.is_none() && !o.audio_only {
        *ctl.progress.note.lock().unwrap() = "Saved without ffmpeg: only formats that come as one file are available (often 360p or 720p). Set ffmpeg in Settings > Streams for the best quality.".into();
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_adds_up_over_two_streams() {
        let mut t = Tally::default();
        assert_eq!(t.feed("DMPROG|100|1000|NA"), Some((100, 1000)));
        assert_eq!(t.feed("DMPROG|1000|1000|NA"), Some((1000, 1000)));
        assert_eq!(t.feed("DMPROG|50|NA|200"), Some((1050, 1200)), "audio stream starts: counted on top, estimate used");
        assert_eq!(t.feed("DMPROG|200|NA|200"), Some((1200, 1200)));
        assert_eq!(t.feed("[download] 5%"), None);
        assert_eq!(t.feed("DMPROG|NA|NA|NA"), None);
    }

    #[test]
    fn command_line_depends_on_ffmpeg_height_and_name() {
        let o = Options { max_height: Some(720), file_name: Some("My 50% clip.mp4".into()), connections: 4, ..Options::default() };
        let with = args("https://y.test/watch?v=1", Path::new("D:\\v"), &o, Some(Path::new("C:\\ff.exe")));
        let without = args("https://y.test/watch?v=1", Path::new("D:\\v"), &o, None);
        let pair = |a: &[String], k: &str| a.iter().position(|x| x == k).map(|i| a[i + 1].clone());
        assert_eq!(pair(&with, "-f").unwrap(), "bv*[height<=720]+ba/b[height<=720]/bv*+ba/b");
        assert_eq!(pair(&without, "-f").unwrap(), "b[height<=720]/b");
        assert!(pair(&with, "-o").unwrap().ends_with("My 50%% clip.%(ext)s"), "{:?}", pair(&with, "-o"));
        assert_eq!(pair(&with, "--merge-output-format").unwrap(), "mp4");
        assert!(pair(&without, "--merge-output-format").is_none());
        assert_eq!(pair(&with, "--concurrent-fragments").unwrap(), "4");
        assert_eq!(with.last().unwrap(), "https://y.test/watch?v=1", "the link is the last argument and never an option");
        let audio = args("u", Path::new("d"), &Options { audio_only: true, ..Options::default() }, Some(Path::new("ff")));
        assert_eq!(pair(&audio, "-f").unwrap(), "ba/b");
        assert!(audio.contains(&"-x".to_string()));
    }

    #[test]
    fn a_link_starting_with_a_dash_cannot_become_an_option() {
        // yt-dlp reads "--exec ..." as an option, so the manager only passes http(s) links; args() keeps the link last
        let a = args("https://x.test/--exec", Path::new("d"), &Options::default(), None);
        assert_eq!(a.last().unwrap(), "https://x.test/--exec");
    }
}
