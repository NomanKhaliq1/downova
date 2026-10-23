//! Drives the real HTTP API against a real Manager and the local range-capable test server.
#[path = "../../core/tests/common/mod.rs"]
mod common;
use common::*;
use dm_core::Manager;
use dm_server::{new_token, router, AppState};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

struct Api {
    base: String,
    token: String,
    c: reqwest::Client,
    root: std::path::PathBuf,
    mgr: Manager,
}

async fn start(name: &str) -> Api {
    let root = tmp(name);
    std::fs::create_dir_all(root.join("ui")).unwrap();
    std::fs::write(root.join("ui/index.html"), "<h1>ui</h1>").unwrap();
    let mgr = Manager::open(root.join("data"), root.join("dl")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let token = new_token();
    let state = Arc::new(AppState {
        mgr: mgr.clone(),
        token: token.clone(),
        product_id: "testapp".into(),
        ui_dir: root.join("ui"),
        clip: broadcast::channel(8).0,
        hosts: vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")],
        quit: Arc::new(tokio::sync::Notify::new()),
    });
    tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
    Api { base: format!("http://127.0.0.1:{port}"), token, c: reqwest::Client::new(), root, mgr }
}

impl Api {
    fn req(&self, m: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.c.request(m, format!("{}{path}", self.base)).header("Authorization", format!("Bearer {}", self.token))
    }
    async fn get(&self, path: &str) -> (u16, Value) {
        let r = self.req(reqwest::Method::GET, path).send().await.unwrap();
        (r.status().as_u16(), serde_json::from_str(&r.text().await.unwrap()).unwrap_or(Value::Null))
    }
    async fn send(&self, m: reqwest::Method, path: &str, body: Value) -> (u16, Value) {
        let r = self.req(m, path).header("content-type", "application/json").body(body.to_string()).send().await.unwrap();
        (r.status().as_u16(), serde_json::from_str(&r.text().await.unwrap()).unwrap_or(Value::Null))
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.send(reqwest::Method::POST, path, body).await
    }
    async fn wait_done(&self, n: usize) {
        let t = Instant::now();
        loop {
            let (_, v) = self.get("/api/tasks?status=Done").await;
            if v.as_array().is_some_and(|a| a.len() >= n) {
                return;
            }
            assert!(t.elapsed() < Duration::from_secs(20), "downloads did not finish");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn token_and_host_are_enforced() {
    let a = start("api-auth").await;
    let c = &a.c;
    assert_eq!(c.get(format!("{}/api/ping", a.base)).send().await.unwrap().status(), 200);
    assert_eq!(c.get(format!("{}/api/tasks", a.base)).send().await.unwrap().status(), 401);
    assert_eq!(c.get(format!("{}/api/tasks", a.base)).header("Authorization", "Bearer nope").send().await.unwrap().status(), 401);
    assert_eq!(c.get(format!("{}/api/tasks?token={}", a.base, a.token)).send().await.unwrap().status(), 200);
    // a page on another site that points evil.com at 127.0.0.1 must not get through
    let r = c.get(format!("{}/api/ping", a.base)).header("Host", "evil.com").send().await.unwrap();
    assert_eq!(r.status(), 403);
}

#[tokio::test(flavor = "multi_thread")]
async fn add_batch_download_and_list() {
    let size = 300_000;
    let (srv, a) = (serve(size), start("api-add").await);
    let (code, v) = a.post("/api/tasks", json!({"urls": format!("{}/f[1-2].zip\n{}/f1.zip\nnot a link", srv.url(""), srv.url(""))})).await;
    assert_eq!(code, 200);
    assert_eq!((v["added"].as_array().unwrap().len(), v["duplicates"].as_array().unwrap().len(), v["invalid"].as_array().unwrap().len()), (2, 1, 1));
    a.wait_done(2).await;
    assert_eq!(sha_file(&a.root.join("dl/Compressed/f1.zip")), expected(size));
    let (_, one) = a.get("/api/tasks?q=F2").await;
    assert_eq!(one.as_array().unwrap().len(), 1);
    assert_eq!(one[0]["category"], "Compressed");
    assert_eq!(a.post("/api/tasks", json!({"urls": "  "})).await.0, 400);
    assert_eq!(a.get("/api/tasks/9999").await.0, 404);
    let r = a.req(reqwest::Method::GET, "/api/export").send().await.unwrap();
    assert_eq!(r.text().await.unwrap().lines().count(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn capture_respects_exclusions_and_dedupes() {
    let (srv, a) = (serve(50_000), start("api-cap").await);
    let url = srv.url("/c.zip");
    let (_, first) = a.post("/api/capture", json!({"url": url, "referer": "https://example.com/"})).await;
    assert_eq!(first["captured"], true);
    let (_, again) = a.post("/api/capture", json!({"url": url})).await;
    assert_eq!((again["duplicate"].clone(), again["id"].clone()), (json!(true), first["id"].clone()));

    let (_, mut s) = a.get("/api/settings").await;
    s["exclusions"]["sites"] = json!(["127.0.0.1"]);
    assert_eq!(a.send(reqwest::Method::PUT, "/api/settings", s).await.0, 200);
    let (_, skipped) = a.post("/api/capture", json!({"url": srv.url("/other.zip")})).await;
    assert_eq!(skipped["captured"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_and_queues_roundtrip() {
    let a = start("api-set").await;
    let (_, mut s) = a.get("/api/settings").await;
    s["connections"] = json!(99);
    s["clipboard_monitor"] = json!(true);
    let (code, saved) = a.send(reqwest::Method::PUT, "/api/settings", s).await;
    assert_eq!((code, saved["connections"].clone(), saved["clipboard_monitor"].clone()), (200, json!(32), json!(true)));
    assert_eq!(a.get("/api/settings").await.1["connections"], 32);

    let night = json!({"name": "ignored", "max_simultaneous": 2, "enabled": true, "schedule": {"days": 127, "start": 1380, "stop": 360}, "on_finish": "Shutdown"});
    assert_eq!(a.send(reqwest::Method::PUT, "/api/queues/Night", night).await.0, 200);
    let (_, qs) = a.get("/api/queues").await;
    let n = qs.as_array().unwrap().iter().find(|q| q["name"] == "Night").unwrap();
    assert_eq!((n["max_simultaneous"].clone(), n["on_finish"].clone()), (json!(2), json!("Shutdown")));
    assert_eq!(a.send(reqwest::Method::DELETE, "/api/queues/Main", json!({})).await.0, 400);
    assert_eq!(a.send(reqwest::Method::DELETE, "/api/queues/Night", json!({})).await.0, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_resume_reorder_and_delete_over_http() {
    let (srv, a) = (serve(3_000_000), start("api-ctl").await);
    let (_, v) = a.post("/api/tasks", json!({"urls": srv.url("/slow-1.zip")})).await;
    let id = v["added"][0].as_u64().unwrap();
    let t = Instant::now();
    while a.get(&format!("/api/tasks/{id}")).await.1["downloaded"].as_u64().unwrap_or(0) == 0 {
        assert!(t.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert_eq!(a.post(&format!("/api/tasks/{id}/pause"), json!({})).await.0, 200);
    let t = Instant::now();
    while a.get(&format!("/api/tasks/{id}")).await.1["status"] != "Paused" {
        assert!(t.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert_eq!(a.post("/api/tasks/reorder", json!({"ids": [id]})).await.0, 200);
    assert_eq!(a.post(&format!("/api/tasks/{id}/open"), json!({"what": "file"})).await.0, 400, "not finished, nothing to open");
    assert_eq!(a.send(reqwest::Method::DELETE, &format!("/api/tasks/{id}?files=true"), json!({})).await.0, 200);
    assert_eq!(a.get(&format!("/api/tasks/{id}")).await.0, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn event_stream_carries_progress_and_task_changes() {
    let (srv, a) = (serve(2_000_000), start("api-sse").await);
    let mut r = a.c.get(format!("{}/api/events?token={}", a.base, a.token)).send().await.unwrap();
    assert_eq!(r.status(), 200);
    a.post("/api/tasks", json!({"urls": srv.url("/slow-s.zip")})).await;
    let (mut seen_task, mut seen_progress) = (false, false);
    let mut text = String::new();
    let t = Instant::now();
    while !(seen_task && seen_progress) {
        assert!(t.elapsed() < Duration::from_secs(10), "stream said only: {text}");
        if let Ok(Ok(Some(chunk))) = tokio::time::timeout(Duration::from_secs(2), r.chunk()).await {
            text.push_str(&String::from_utf8_lossy(&chunk));
        }
        seen_task = text.contains("event: task");
        seen_progress = text.contains("event: progress") && text.contains("\"speed\"");
    }
    a.mgr.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn static_files_are_public_and_confined() {
    let a = start("api-ui").await;
    let r = a.c.get(format!("{}/", a.base)).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
    assert_eq!(a.c.get(format!("{}/missing.js", a.base)).send().await.unwrap().status(), 404);
    assert_eq!(a.c.get(format!("{}/..%5Cdata/state.json", a.base)).send().await.unwrap().status(), 404);
}
