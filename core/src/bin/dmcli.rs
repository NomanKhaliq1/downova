use dm_core::{download, Control, Options};
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: dmcli <url> [-d dir] [-c connections] [--limit KB/s] [--sha256 hex] [-H \"Name: value\"] [--referer url] [--cookie text] [--proxy url] [--max-height 720] [--name file-name] [--ffmpeg path]";

fn parse() -> Result<(String, PathBuf, Options), String> {
    let mut args = std::env::args().skip(1);
    let mut url = None;
    let mut dir = PathBuf::from(".");
    let mut o = Options::default();
    while let Some(a) = args.next() {
        let mut val = |what: &str| args.next().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "-d" => dir = val("-d")?.into(),
            "-c" => o.connections = val("-c")?.parse().map_err(|_| "-c needs a number")?,
            "--limit" => o.speed_limit = Some(val("--limit")?.parse::<u64>().map_err(|_| "--limit needs a number")? * 1024),
            "--sha256" => o.sha256 = Some(val("--sha256")?),
            "--referer" => o.referer = Some(val("--referer")?),
            "--cookie" => o.cookie = Some(val("--cookie")?),
            "--proxy" => o.proxy = Some(val("--proxy")?),
            "--max-height" => o.max_height = Some(val("--max-height")?.parse().map_err(|_| "--max-height needs a number")?),
            "--name" => o.file_name = Some(val("--name")?),
            "--ffmpeg" => o.ffmpeg_path = Some(val("--ffmpeg")?.into()),
            "-H" => {
                let h = val("-H")?;
                let (k, v) = h.split_once(':').ok_or("-H needs \"Name: value\"")?;
                o.headers.push((k.trim().into(), v.trim().into()));
            }
            _ if a.starts_with('-') => return Err(format!("unknown option {a}")),
            _ => url = Some(a),
        }
    }
    Ok((url.ok_or("missing url")?, dir, o))
}

fn mb(b: u64) -> f64 {
    b as f64 / 1_048_576.0
}

#[tokio::main]
async fn main() {
    let (url, dir, o) = match parse() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let ctl = Control::new(&o);
    let cancel = ctl.cancel.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        eprintln!("\nstopping, progress is saved...");
        cancel.store(true, Relaxed);
    });

    let started = Instant::now();
    let shown = ctl.progress.clone();
    let ticker = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let (d, t) = (shown.downloaded.load(Relaxed), shown.total.load(Relaxed));
            let speed = mb(d) / started.elapsed().as_secs_f64().max(0.001);
            if t > 0 {
                eprint!("\r{:5.1}%  {:8.1} / {:.1} MB  avg {:.1} MB/s   ", d as f64 * 100.0 / t as f64, mb(d), mb(t), speed);
            } else {
                eprint!("\r{:8.1} MB  avg {:.1} MB/s   ", mb(d), speed);
            }
        }
    });

    let r = download(&url, &dir, &o, &ctl).await;
    ticker.abort();
    eprintln!();
    match r {
        Ok(p) => println!("saved {} in {:.1}s", p.display(), started.elapsed().as_secs_f64()),
        Err(e) => {
            eprintln!("failed: {e}");
            std::process::exit(1);
        }
    }
}
