//! The background app: engine + local API + tray icon in one small process.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use dm_core::{power, Event, FinishAction, Manager};
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
    format!("http://127.0.0.1:{port}/#token={token}&view={view}")
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
fn tray_icon(hex: &str) -> Icon {
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
    Icon::from_rgba(px, n as u32, n as u32).expect("valid icon buffer")
}
fn main() {
    let brand: Brand = toml::from_str(include_str!("../../../brand/brand.toml")).expect("brand/brand.toml");
    let background = std::env::args().any(|a| a == "--background");
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    // DM_DATA_DIR / DM_DOWNLOAD_DIR let tests run against throwaway folders without touching the user's
    let data_dir = std::env::var_os("DM_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| appdata.join(&brand.product_id));
    std::fs::create_dir_all(&data_dir).expect("create data folder");
    let api_file = data_dir.join("api.json");

    if let Some((port, token)) = running_instance(&api_file, &brand.product_id) {
        if !background {
            system::open_ui(&ui_url(port, &token, "main"));
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

    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    std_listener.set_nonblocking(true).unwrap();
    let port = std_listener.local_addr().unwrap().port();
    let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
    let token = new_token();
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

    let _tray = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip(&brand.name).with_icon(tray_icon(&brand.primary_color)).build().expect("tray icon");

    if !background {
        system::open_ui(&ui_url(port, &token, "main"));
    }

    let rt_handle = rt.handle().clone();
    let (mgr2, api2) = (mgr.clone(), api_file.clone());
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
            TaoEvent::UserEvent(UserEvent::Quit) => {
                rt_handle.block_on(mgr2.shutdown()); // keeps partial files and writes the state
                let _ = std::fs::remove_file(&api2);
                *flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}
