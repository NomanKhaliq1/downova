//! The relay against a real background-app API (dm_server) on a loopback port.
#[path = "../../core/tests/common/mod.rs"]
mod common;
use common::*;
use dm_core::Manager;
use dm_host::{handle, read_msg, serve as relay, write_msg, Config, VERSION};
use dm_server::{new_token, router, AppState};
use serde_json::{json, Value};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

struct Fx {
    cfg: Config,
    mgr: Manager,
}

async fn start(name: &str) -> Fx {
    let root = tmp(name);
    std::fs::create_dir_all(root.join("ui")).unwrap();
    let mgr = Manager::open(root.join("data"), root.join("dl")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let token = new_token();
    let state = Arc::new(AppState {
        mgr: mgr.clone(),
        token: token.clone(),
        product_id: "testapp".into(),
        ui_dir: root.join("ui"),
        clip: broadcast::channel(4).0,
        hosts: vec![format!("127.0.0.1:{port}")],
        quit: Arc::new(tokio::sync::Notify::new()),
    });
    tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
    std::fs::write(root.join("api.json"), json!({"port": port, "token": token, "pid": 1}).to_string()).unwrap();
    let cfg = Config { data_dir: root.clone(), product_id: "testapp".into(), app_path: PathBuf::from("does-not-exist.exe"), start_wait: Duration::from_millis(300) };
    Fx { cfg, mgr }
}

fn ask(cfg: &Config, msgs: &[Value]) -> Vec<Value> {
    let mut input = Vec::new();
    msgs.iter().for_each(|m| write_msg(&mut input, m).unwrap());
    let mut out = Vec::new();
    relay(cfg, Cursor::new(input), &mut out).unwrap();
    let mut r = Cursor::new(out);
    std::iter::from_fn(|| read_msg(&mut r).unwrap()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn hello_ping_capture_and_batch_add_work() {
    let (srv, fx) = (serve(100_000), start("relay-ok").await);
    let r = ask(
        &fx.cfg,
        &[
            json!({"id": 1, "cmd": "hello", "version": VERSION}),
            json!({"id": 2, "cmd": "api", "method": "GET", "path": "/api/ping"}),
            json!({"id": 3, "cmd": "api", "method": "POST", "path": "/api/capture", "body": {"url": srv.url("/a.zip"), "referer": "https://x.test/"}}),
            json!({"id": 4, "cmd": "api", "method": "POST", "path": "/api/tasks", "body": {"urls": format!("{}/b1.zip\n{}/b2.zip", srv.url(""), srv.url("")), "paused": true}}),
            json!({"id": 5, "cmd": "api", "method": "GET", "path": "/api/settings"}),
        ],
    );
    assert_eq!(r.len(), 5);
    assert_eq!((r[0]["ok"].clone(), r[0]["app"]["app"].clone()), (json!(true), json!("testapp")));
    assert_eq!(r[1]["body"]["app"], "testapp");
    assert_eq!((r[2]["status"].clone(), r[2]["body"]["captured"].clone()), (json!(200), json!(true)));
    assert_eq!(r[3]["body"]["added"].as_array().unwrap().len(), 2);
    assert!(r[4]["body"]["categories"].is_array());
    assert_eq!(fx.mgr.list(&Default::default()).len(), 3);
    for (i, m) in r.iter().enumerate() {
        assert_eq!(m["id"], i as u64 + 1, "ids are echoed");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn anything_outside_the_allow_list_is_refused() {
    let fx = start("relay-deny").await;
    let r = ask(
        &fx.cfg,
        &[
            json!({"id": 1, "cmd": "api", "method": "POST", "path": "/api/quit"}),
            json!({"id": 2, "cmd": "api", "method": "DELETE", "path": "/api/tasks/1"}),
            json!({"id": 3, "cmd": "api", "method": "GET", "path": "/api/capture"}),
            json!({"id": 4, "cmd": "api", "method": "PUT", "path": "/api/settings", "body": {}}),
            json!({"id": 5, "cmd": "api", "method": "POST", "path": "/api/tasks/1/open", "body": {"what": "file"}}),
            json!({"id": 6, "cmd": "api", "method": "GET", "path": "/api/events"}),
            json!({"id": 7, "cmd": "format-disk"}),
        ],
    );
    let errors: Vec<&str> = r.iter().map(|m| m["error"].as_str().unwrap()).collect();
    assert_eq!(errors, ["not_allowed", "not_allowed", "not_allowed", "not_allowed", "not_allowed", "not_allowed", "unknown_command"]);
    assert!(r.iter().all(|m| m["ok"] == false));
}

#[tokio::test(flavor = "multi_thread")]
async fn version_mismatch_is_reported_with_both_versions() {
    let fx = start("relay-ver").await;
    let r = handle(&fx.cfg, &json!({"id": 9, "cmd": "hello", "version": "0.0.1-old"}));
    assert_eq!((r["error"].clone(), r["host_version"].clone(), r["extension_version"].clone()), (json!("version_mismatch"), json!(VERSION), json!("0.0.1-old")));
}

#[tokio::test(flavor = "multi_thread")]
async fn app_down_gives_a_clean_error_not_a_hang() {
    let fx = start("relay-down").await;
    std::fs::write(fx.cfg.data_dir.join("api.json"), json!({"port": 1, "token": "x"}).to_string()).unwrap(); // nothing listens on port 1
    let started = std::time::Instant::now();
    let r = ask(&fx.cfg, &[json!({"id": 1, "cmd": "hello", "version": VERSION}), json!({"id": 2, "cmd": "api", "method": "GET", "path": "/api/ping"})]);
    assert_eq!((r[0]["error"].clone(), r[1]["error"].clone()), (json!("app_not_running"), json!("app_not_running")));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn framing_rejects_oversize_and_handles_eof() {
    let mut huge = Cursor::new((100_000_000u32).to_le_bytes().to_vec());
    assert!(read_msg(&mut huge).is_err());
    assert!(read_msg(&mut Cursor::new(Vec::<u8>::new())).unwrap().is_none());
}
