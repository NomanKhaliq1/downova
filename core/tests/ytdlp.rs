//! The yt-dlp integration against a stand-in program that prints what the real tool prints (progress lines, title,
//! final file) and fails or stalls when told to. The real yt-dlp is not needed, and nothing is downloaded from the internet.
use dm_core::{download, AddOpts, Control, Error, Manager, Options, Settings, Status, TaskOpts};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use std::{fs, sync::atomic::Ordering::Relaxed};

const FAKE: &str = r#"
use std::io::Write;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let url = a.last().cloned().unwrap_or_default();
    let tpl = a.iter().position(|x| x == "-o").map(|i| a[i + 1].clone()).unwrap_or_default();
    if url.ends_with("/fail") {
        eprintln!("ERROR: [youtube] abc: Video unavailable");
        std::process::exit(1);
    }
    println!("DMTITLE|Fake Title");
    println!("DMPROG|100|1000|NA");
    println!("DMPROG|1000|1000|NA");
    println!("DMPROG|50|NA|200");
    println!("DMPROG|200|NA|200");
    std::io::stdout().flush().unwrap();
    if url.ends_with("/slow") {
        loop { std::thread::sleep(std::time::Duration::from_millis(100)); }
    }
    let path = if tpl.contains("%(") { tpl.replace("%(title).150B [%(id)s]", "Fake Title [abc]") } else { tpl };
    let path = path.replace("%(ext)s", "mp4").replace("%%", "%");
    std::fs::write(&path, b"video").unwrap();
    println!("DMFILE|{path}");
    // the arguments it was given, for the tests to look at
    std::fs::write(std::path::Path::new(&path).with_extension("args.txt"), a[1..].join("\n")).unwrap();
}
"#;

fn fake_exe() -> &'static PathBuf {
    static EXE: OnceLock<PathBuf> = OnceLock::new();
    EXE.get_or_init(|| {
        let dir = tmp("fake-tool");
        let src = dir.join("fake.rs");
        fs::write(&src, FAKE).unwrap();
        let exe = dir.join("yt-dlp.exe");
        let ok = Command::new("rustc").args(["--edition", "2021", "-O"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success();
        assert!(ok, "could not compile the stand-in tool");
        exe
    })
}

fn tmp(name: &str) -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let p = std::env::temp_dir().join(format!("dm-test-ytdlp-{name}-{}-{n}", std::process::id()));
    fs::create_dir_all(&p).unwrap();
    p
}

fn opts() -> Options {
    Options { use_ytdlp: true, ytdlp_path: Some(fake_exe().clone()), ffmpeg_path: Some(PathBuf::from("no-such-ffmpeg.exe")), connections: 3, ..Options::default() }
}

#[tokio::test(flavor = "multi_thread")]
async fn page_download_reports_progress_name_and_the_missing_ffmpeg() {
    let dir = tmp("ok");
    let o = Options { max_height: Some(720), ..opts() };
    let ctl = Control::new(&o);
    let p = download("https://y.test/watch?v=abc", &dir, &o, &ctl).await.unwrap();
    assert_eq!(p.file_name().unwrap(), "Fake Title [abc].mp4");
    assert_eq!(fs::read(&p).unwrap(), b"video");
    assert_eq!((ctl.progress.downloaded.load(Relaxed), ctl.progress.total.load(Relaxed)), (1200, 1200), "video and audio streams add up");
    assert_eq!(*ctl.progress.name.lock().unwrap(), "Fake Title [abc].mp4");
    assert!(ctl.progress.note.lock().unwrap().contains("without ffmpeg"));
    let args = fs::read_to_string(p.with_extension("args.txt")).unwrap();
    assert!(args.contains("b[height<=720]/b"), "no ffmpeg: single-file formats only: {args}");
    assert_eq!(args.lines().last().unwrap(), "https://y.test/watch?v=abc");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_tools_own_error_message_is_shown() {
    let dir = tmp("fail");
    let r = download("https://y.test/fail", &dir, &opts(), &Control::new(&opts())).await;
    assert!(matches!(&r, Err(Error::Server(m)) if m.contains("Video unavailable")), "{r:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_kills_the_tool_quickly() {
    let dir = tmp("slow");
    let o = opts();
    let ctl = Control::new(&o);
    let cancel = ctl.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(600)).await;
        cancel.store(true, Relaxed);
    });
    let t = Instant::now();
    let r = download("https://y.test/slow", &dir, &o, &ctl).await;
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
    assert!(t.elapsed() < Duration::from_secs(5), "took {:?}", t.elapsed());
    assert_eq!(ctl.progress.downloaded.load(Relaxed), 1200, "what had arrived stays visible");
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_tool_gives_a_clear_message() {
    let o = Options { use_ytdlp: true, ytdlp_path: Some(PathBuf::from("no-such-yt-dlp.exe")), ..Options::default() };
    let r = download("https://y.test/watch?v=abc", &tmp("none"), &o, &Control::new(&o)).await;
    assert!(matches!(&r, Err(Error::Invalid(m)) if m.contains("not set up")), "{r:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn manager_sends_page_links_to_the_tool_and_files_them_under_video() {
    let root = tmp("mgr");
    let m = Manager::open(root.join("data"), root.join("dl")).unwrap();
    m.set_settings(Settings { ytdlp_path: Some(fake_exe().clone()), ffmpeg_path: Some(PathBuf::from("none.exe")), ..m.settings() });
    let id = m.add("https://y.test/watch?v=abc", &AddOpts { task: TaskOpts { ytdlp: true, filename: Some("Holiday trip".into()), ..Default::default() }, ..AddOpts::default() }).unwrap();
    let t = Instant::now();
    while m.get(id).unwrap().task.status != Status::Done {
        assert!(t.elapsed() < Duration::from_secs(30), "{:?}", m.get(id).unwrap().task);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let task = m.get(id).unwrap().task;
    assert_eq!((task.category.as_str(), task.name.as_str()), ("Video", "Holiday trip.mp4"));
    assert!(Path::new(&root.join("dl/Video/Holiday trip.mp4")).exists());
    assert!(task.note.contains("without ffmpeg"));
    m.shutdown().await;
}
