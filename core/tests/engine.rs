//! End-to-end engine tests against the local test server.
mod common;
use common::*;
use dm_core::{download, Control, Error, Options};
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};
use std::fs;


#[tokio::test(flavor = "multi_thread")]
async fn ranged_download_is_byte_exact() {
    let size = 5_000_123;
    let (srv, dir) = (serve(size), tmp("ranged"));
    let path = run(&srv.url("/data"), &dir, &opts()).await.unwrap();
    assert_eq!(path.file_name().unwrap(), "data");
    assert_eq!(sha_file(&path), expected(size));
    assert!(!dir.join("data.part").exists() && !dir.join("data.part.json").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn server_without_range_falls_back_to_one_stream() {
    let size = 1_500_000;
    let (srv, dir) = (serve(size), tmp("norange"));
    let path = run(&srv.url("/norange"), &dir, &opts()).await.unwrap();
    assert_eq!(sha_file(&path), expected(size));
}

#[tokio::test(flavor = "multi_thread")]
async fn dropped_connections_are_retried() {
    let size = 3_000_000;
    let (srv, dir) = (serve(size), tmp("flaky"));
    let o = Options { retries: 12, ..opts() };
    let path = run(&srv.url("/flaky"), &dir, &o).await.unwrap();
    assert_eq!(sha_file(&path), expected(size));
}

#[tokio::test(flavor = "multi_thread")]
async fn redirect_and_content_disposition_name() {
    let size = 400_000;
    let (srv, dir) = (serve(size), tmp("names"));
    let a = run(&srv.url("/redirect"), &dir, &opts()).await.unwrap();
    assert_eq!(a.file_name().unwrap(), "data");
    let b = run(&srv.url("/name"), &dir, &opts()).await.unwrap();
    assert_eq!(b.file_name().unwrap(), "my file.bin");
    let c = run(&srv.url("/name"), &dir, &opts()).await.unwrap(); // same name again -> numbered
    assert_eq!(c.file_name().unwrap(), "my file (1).bin");
}

#[tokio::test(flavor = "multi_thread")]
async fn cookie_gate_and_http_errors() {
    let size = 300_000;
    let (srv, dir) = (serve(size), tmp("cookie"));
    assert!(matches!(run(&srv.url("/cookie"), &dir, &opts()).await, Err(Error::Server(_))));
    let o = Options { cookie: Some("k=v".into()), ..opts() };
    let path = run(&srv.url("/cookie"), &dir, &o).await.unwrap();
    assert_eq!(sha_file(&path), expected(size));
    assert!(matches!(run(&srv.url("/missing"), &dir, &opts()).await, Err(Error::Server(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_then_resume_gives_identical_file() {
    let size = 4_000_000;
    let (srv, dir) = (serve(size), tmp("resume"));
    let o = Options { connections: 2, ..opts() };
    let ctl = Control::new(&o);
    let cancel = ctl.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        cancel.store(true, Relaxed);
    });
    let r = download(&srv.url("/slow"), &dir, &o, &ctl).await;
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
    let state = fs::read_to_string(dir.join("slow.part.json")).expect("progress must be saved on cancel");
    assert!(state.contains("\"pos\""));

    let ctl2 = Control::new(&o);
    let path = download(&srv.url("/slow"), &dir, &o, &ctl2).await.unwrap();
    assert_eq!(sha_file(&path), expected(size));
    assert!(!dir.join("slow.part.json").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn checksum_is_enforced() {
    let size = 600_000;
    let (srv, dir) = (serve(size), tmp("sha"));
    let bad = Options { sha256: Some("00".repeat(32)), ..opts() };
    assert!(matches!(run(&srv.url("/data"), &dir, &bad).await, Err(Error::Checksum)));
    assert!(!dir.join("data").exists());
    let good = Options { sha256: Some(expected(size)), ..opts() };
    assert!(run(&srv.url("/data"), &dir, &good).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn speed_limit_is_respected() {
    let size = 1_000_000;
    let (srv, dir) = (serve(size), tmp("limit"));
    let o = Options { speed_limit: Some(500_000), ..opts() };
    let t = Instant::now();
    run(&srv.url("/data"), &dir, &o).await.unwrap();
    assert!(t.elapsed() >= Duration::from_millis(1400), "finished in {:?}", t.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
async fn idle_connections_split_the_slowest_segment() {
    let size = 6_000_000;
    let (srv, dir) = (serve(size), tmp("skew"));
    let o = Options { connections: 3, min_split: 512 * 1024, ..opts() };
    let path = run(&srv.url("/skew"), &dir, &o).await.unwrap();
    assert_eq!(sha_file(&path), expected(size));
    // probe + 3 first requests = 4; anything above means a fast connection took over part of the slow segment
    assert!(srv.hits.load(Relaxed) > 4, "hits = {}", srv.hits.load(Relaxed));
}
