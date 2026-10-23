//! The background app: engine + local API + tray icon in one small process.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use dm_core::{power, Event, Filter, FinishAction, Manager, Status};
use dm_server::{clipboard, new_token, router, system, AppState};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tao::event::Event as TaoEvent;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tokio::sync::broadcast;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};

#[derive(Deserialize)]
struct Brand {
    name: String,
    product_id: String,
    primary_color: String,
}

enum UserEvent {
    Menu(MenuEvent),
    Tray(TrayIconEvent),
    /// Downloads running or waiting, and the tray tooltip that lists them.
    Status(usize, String),
    Quit,
}

fn log(dir: &Path, msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("dmd.log")) {
        let _ = writeln!(f, "{msg}");
    }
}

/// Is an instance of this product already answering on the port in api.json?
fn running_instance(api_file: &Path, product_id: &str) -> Option<(u16, String)> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(api_file).ok()?).ok()?;
    let (port, token) = (v["port"].as_u64()? as u16, v["token"].as_str()?.to_string());
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(800))).ok()?;
    write!(s, "GET /api/ping HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").ok()?;
    let mut body = String::new();
    s.read_to_string(&mut body).ok()?;
    body.contains(&format!("\"app\":\"{product_id}\"")).then_some((port, token))
}

fn ui_url(port: u16, token: &str, view: &str) -> String {
    // "streams" opens Settings on the streams tab (where the YouTube helper is set up); other views are a hash hint for the page
    let query = if view == "streams" { "?screen=settings&tab=streams" } else { "" };
    format!("http://127.0.0.1:{port}/{query}#token={token}&view={view}")
}

fn find_ui_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let beside = exe.parent().map(|p| p.join("ui"));
    // dev layout: target/{debug,release}/dmd.exe -> <repo>/ui
    let dev = exe.ancestors().nth(3).map(|p| p.join("ui"));
    std::env::var_os("DM_UI_DIR").map(PathBuf::from).into_iter().chain(beside).chain(dev).find(|p| p.join("index.html").exists()).unwrap_or_else(|| PathBuf::from("ui"))
}

/// The brand mark (two interlocking corner brackets, same geometry as brand/assets/logo.svg) on a rounded tile
/// in the brand colour, drawn in code so the tray icon always matches brand.toml.
fn tray_icon(hex: &str, count: usize) -> Icon {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x2563eb);
    let rgb = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
    let mark = |x: f32, y: f32| (4.0..19.0).contains(&x) && (4.0..9.0).contains(&y) || (4.0..9.0).contains(&x) && (4.0..19.0).contains(&y) || (13.0..28.0).contains(&x) && (23.0..28.0).contains(&y) || (23.0..28.0).contains(&x) && (13.0..28.0).contains(&y);
    let n = 32usize;
    let aa = 3; // 3x3 samples per pixel for smooth edges
    let mut px = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let mut sum = [0f32; 4];
            for sy in 0..aa {
                for sx in 0..aa {
                    let fx = (x as f32 + (sx as f32 + 0.5) / aa as f32) / n as f32 * 32.0;
                    let fy = (y as f32 + (sy as f32 + 0.5) / aa as f32) / n as f32 * 32.0;
                    let (dx, dy) = (((fx - 16.0).abs() - 9.0).max(0.0), ((fy - 16.0).abs() - 9.0).max(0.0));
                    if dx * dx + dy * dy < 36.0 {
                        let c = if mark(fx, fy) { [255.0; 3] } else { [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32] };
                        sum = [sum[0] + c[0], sum[1] + c[1], sum[2] + c[2], sum[3] + 255.0];
                    }
                }
            }
            let i = (y * n + x) * 4;
            let covered = sum[3] / 255.0;
            if covered > 0.0 {
                px[i..i + 3].iter_mut().zip(&sum[..3]).for_each(|(o, s)| *o = (s / covered) as u8);
                px[i + 3] = (sum[3] / (aa * aa) as f32) as u8;
            }
        }
    }
    if count > 0 {
        badge(&mut px, n, count.min(99));
    }
    Icon::from_rgba(px, n as u32, n as u32).expect("valid icon buffer")
}

/// A red dot with the number in the lower right corner, like the unread count on app icons.
fn badge(px: &mut [u8], n: usize, count: usize) {
    // 3x5 digits, one row per entry, high bit on the left
    const DIGITS: [[u8; 5]; 10] = [[7, 5, 5, 5, 7], [2, 6, 2, 2, 7], [7, 1, 7, 4, 7], [7, 1, 7, 1, 7], [5, 5, 7, 1, 1], [7, 4, 7, 1, 7], [7, 4, 7, 5, 7], [7, 1, 1, 1, 1], [7, 5, 7, 5, 7], [7, 5, 7, 1, 7]];
    let (cx, cy, r) = (23.5f32, 23.5f32, 8.5f32);
    let mut put = |x: usize, y: usize, c: [u8; 3], a: f32| {
        let i = (y * n + x) * 4;
        for k in 0..3 {
            px[i + k] = (px[i + k] as f32 * (1.0 - a) + c[k] as f32 * a) as u8;
        }
        px[i + 3] = px[i + 3].max((a * 255.0) as u8);
    };
    for y in 14..n {
        for x in 14..n {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            put(x, y, [0xe5, 0x39, 0x35], (r - d + 0.5).clamp(0.0, 1.0));
        }
    }
    let digits: Vec<usize> = count.to_string().bytes().map(|b| (b - b'0') as usize).collect();
    let width = digits.len() * 6 + digits.len() - 1; // each digit 3 px wide at scale 2, 1 px apart
    let (left, top) = (cx as usize + 1 - width / 2 - 1, cy as usize - 4);
    for (j, &d) in digits.iter().enumerate() {
        for (row, bits) in DIGITS[d].iter().enumerate() {
            for col in 0..3 {
                if bits >> (2 - col) & 1 == 1 {
                    for (sx, sy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        put(left + j * 7 + col * 2 + sx, top + row * 2 + sy, [255; 3], 1.0);
                    }
                }
            }
        }
    }
}

#[test]
fn badge_fits_every_count() {
    for c in 0..=150 {
        let _ = tray_icon("#2563eb", c); // panics if a digit is drawn outside the 32 px icon
    }
    assert_eq!(speed(1_500_000), "1.5 MB/s");
}

/// "1.2 MB/s" style speed for the tray tooltip.
fn speed(b: u64) -> String {
    match b {
        0..1_000 => format!("{b} B/s"),
        1_000..1_000_000 => format!("{:.0} KB/s", b as f64 / 1e3),
        _ => format!("{:.1} MB/s", b as f64 / 1e6),
    }
}

/// Tray tooltip: how many downloads run, the total speed, then one line per file while it fits
/// (Windows cuts tooltips after 127 characters).
fn tooltip(brand: &str, tasks: &[dm_core::TaskView]) -> String {
    if tasks.is_empty() {
        return brand.to_string();
    }
    let running = tasks.iter().filter(|t| t.task.status == Status::Downloading).count();
    let mut tip = format!("{brand}: {running} downloading, {} waiting · {}", tasks.len() - running, speed(tasks.iter().map(|t| t.speed).sum()));
    for t in tasks {
        let name: String = t.task.name.chars().take(26).collect();
        let pct = if t.task.total > 0 { format!(" {}%", t.task.downloaded * 100 / t.task.total) } else { String::new() };
        let line = format!("\n{name}{pct}");
        if tip.chars().count() + line.chars().count() > 127 {
            break;
        }
        tip.push_str(&line);
    }
    tip
}
fn main() {
    let brand: Brand = toml::from_str(include_str!("../../../brand/brand.toml")).expect("brand/brand.toml");
    let background = std::env::args().any(|a| a == "--background");
    let args: Vec<String> = std::env::args().collect();
    let view = args.iter().position(|a| a == "--view").and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| "main".into());
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    // DM_DATA_DIR / DM_DOWNLOAD_DIR let tests run against throwaway folders without touching the user's
    let data_dir = std::env::var_os("DM_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| appdata.join(&brand.product_id));
    std::fs::create_dir_all(&data_dir).expect("create data folder");
    let api_file = data_dir.join("api.json");

    if let Some((port, token)) = running_instance(&api_file, &brand.product_id) {
        if !background {
            system::open_ui(&ui_url(port, &token, &view));
        }
        return;
    }

    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("runtime");
    let _guard = rt.enter();
    let downloads = std::env::var_os("DM_DOWNLOAD_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Downloads")))
        .unwrap_or_else(|| PathBuf::from("Downloads"));
    let mgr = match Manager::open(data_dir.clone(), downloads) {
        Ok(m) => m,
        Err(e) => {
            log(&data_dir, &format!("cannot open state: {e}"));
            return;
        }
    };

    // the browser helper registration can disappear (cleaners, profile resets, another tool): put it back if it is missing
    if std::env::var_os("DM_DATA_DIR").is_none() {
        if let Some(host) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("dmhost.exe"))).filter(|p| p.exists()) {
            let mut c = std::process::Command::new(host);
            c.arg("repair");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.creation_flags(0x0800_0000);
            }
            let _ = c.spawn();
        }
    }

    // reuse the last port and token when free, so a window left open across a restart reconnects on Retry
    let last: Option<(u16, String)> = std::fs::read(&api_file)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| Some((v["port"].as_u64()? as u16, v["token"].as_str()?.to_string())));
    let (std_listener, token) = match last.and_then(|(p, t)| std::net::TcpListener::bind(("127.0.0.1", p)).ok().map(|l| (l, t))) {
        Some(x) => x,
        None => (std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback"), new_token()),
    };
    std_listener.set_nonblocking(true).unwrap();
    let port = std_listener.local_addr().unwrap().port();
    let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
    let (clip_tx, _) = broadcast::channel(8);
    let quit = Arc::new(tokio::sync::Notify::new());
    let state = Arc::new(AppState {
        quit: quit.clone(),
        mgr: mgr.clone(),
        token: token.clone(),
        product_id: brand.product_id.clone(),
        ui_dir: find_ui_dir(),
        clip: clip_tx.clone(),
        hosts: vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")],
    });
    let tmp = api_file.with_extension("json.tmp");
    let info = serde_json::json!({"port": port, "token": token, "pid": std::process::id()});
    if std::fs::write(&tmp, info.to_string()).and_then(|_| std::fs::rename(&tmp, &api_file)).is_err() {
        log(&data_dir, "cannot write api.json");
    }
    {
        let dd = data_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, router(state)).await {
                log(&dd, &format!("server stopped: {e}"));
            }
        });
    }
    clipboard::spawn(mgr.clone(), clip_tx);

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    {
        let proxy = proxy.clone();
        tokio::spawn(async move {
            quit.notified().await;
            let _ = proxy.send_event(UserEvent::Quit);
        });
    }

    // queue finished -> carry out its action (Exit is ours, the rest is in dm_core::power)
    {
        let (mut rx, proxy) = (mgr.subscribe(), proxy.clone());
        tokio::spawn(async move {
            while let Ok(e) = rx.recv().await {
                if let Event::QueueDrained(_, action) = e {
                    if action == FinishAction::Exit {
                        let _ = proxy.send_event(UserEvent::Quit);
                    } else {
                        power::perform(&action);
                    }
                }
            }
        });
    }

    let menu = Menu::new();
    let open = MenuItem::new(format!("Open {}", brand.name), true, None);
    let add = MenuItem::new("Add link...", true, None);
    let pause = MenuItem::new("Pause all", true, None);
    let resume = MenuItem::new("Resume all", true, None);
    let quit = MenuItem::new("Quit", true, None);
    menu.append_items(&[&open, &add, &PredefinedMenuItem::separator(), &pause, &resume, &PredefinedMenuItem::separator(), &quit]).unwrap();
    let ids = (open.id().clone(), add.id().clone(), pause.id().clone(), resume.id().clone(), quit.id().clone());

    let p = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = p.send_event(UserEvent::Menu(e));
    }));
    let p = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |e| {
        let _ = p.send_event(UserEvent::Tray(e));
    }));

    let tray = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip(&brand.name).with_icon(tray_icon(&brand.primary_color, 0)).build().expect("tray icon");

    // once a second: how many downloads are running or waiting, for the tray number and tooltip
    {
        let (mgr, proxy, name) = (mgr.clone(), proxy.clone(), brand.name.clone());
        tokio::spawn(async move {
            let mut last = String::new();
            loop {
                let mut tasks: Vec<_> = mgr.list(&Filter::default()).into_iter().filter(|t| matches!(t.task.status, Status::Downloading | Status::Queued)).collect();
                tasks.sort_by_key(|t| (t.task.status != Status::Downloading, t.task.id));
                let tip = tooltip(&name, &tasks);
                if tip != last {
                    let _ = proxy.send_event(UserEvent::Status(tasks.len(), tip.clone()));
                    last = tip;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }
    let mut shown = 0;

    if !background {
        system::open_ui(&ui_url(port, &token, &view));
    }

    let rt_handle = rt.handle().clone();
    let mgr2 = mgr.clone();
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        let _keep = &rt; // the runtime lives as long as the event loop
        match event {
            TaoEvent::UserEvent(UserEvent::Menu(e)) => {
                if e.id == ids.0 {
                    system::open_ui(&ui_url(port, &token, "main"));
                } else if e.id == ids.1 {
                    system::open_ui(&ui_url(port, &token, "add"));
                } else if e.id == ids.2 {
                    mgr2.pause_all();
                } else if e.id == ids.3 {
                    mgr2.resume_all();
                } else if e.id == ids.4 {
                    let _ = proxy.send_event(UserEvent::Quit);
                }
            }
            TaoEvent::UserEvent(UserEvent::Tray(TrayIconEvent::DoubleClick { .. })) => system::open_ui(&ui_url(port, &token, "main")),
            TaoEvent::UserEvent(UserEvent::Status(count, tip)) => {
                let _ = tray.set_tooltip(Some(tip));
                if count != shown {
                    let _ = tray.set_icon(Some(tray_icon(&brand.primary_color, count)));
                    shown = count;
                }
            }
            TaoEvent::UserEvent(UserEvent::Quit) => {
                rt_handle.block_on(mgr2.shutdown()); // keeps partial files and writes the state
                // api.json stays: readers ping before trusting it, and the next start reuses its port and token
                *flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}
