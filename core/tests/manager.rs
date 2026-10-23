//! Queue manager tests: categories, concurrency, pause/resume, schedule, persistence, history, drain events.
mod common;
use common::*;
use dm_core::*;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};
use std::{fs, path::Path};

async fn wait(what: &str, secs: u64, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(secs), "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

fn open(root: &Path) -> Manager {
    Manager::open(root.join("data"), root.join("dl")).unwrap()
}

fn status(m: &Manager, id: u64) -> Status {
    m.get(id).unwrap().task.status
}

fn main_queue(max: usize) -> Queue {
    Queue { max_simultaneous: max, ..Queue::new(MAIN_QUEUE) }
}

#[tokio::test(flavor = "multi_thread")]
async fn files_land_in_category_folders_and_queue_limit_holds() {
    let size = 800_000;
    let (srv, root) = (serve(size), tmp("mgr-cat"));
    let m = open(&root);
    m.save_queue(main_queue(1));
    let ids: Vec<u64> = ["/slow-a.zip", "/slow-b.mp4", "/slow-c.pdf"].iter().map(|p| m.add(&srv.url(p), &AddOpts::default()).unwrap()).collect();

    let mut max_running = 0;
    let t = Instant::now();
    while ids.iter().any(|&id| status(&m, id) != Status::Done) {
        let running = ids.iter().filter(|&&id| status(&m, id) == Status::Downloading).count();
        max_running = max_running.max(running);
        assert!(t.elapsed() < Duration::from_secs(30), "downloads did not finish");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(max_running, 1, "queue allows one at a time");
    for (folder, name) in [("Compressed", "slow-a.zip"), ("Video", "slow-b.mp4"), ("Documents", "slow-c.pdf")] {
        let p = root.join("dl").join(folder).join(name);
        assert_eq!(sha_file(&p), expected(size), "{folder}/{name}");
    }
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_keeps_progress_and_resume_finishes() {
    let size = 3_000_000;
    let (srv, root) = (serve(size), tmp("mgr-pause"));
    let m = open(&root);
    let id = m.add(&srv.url("/slow-p.zip"), &AddOpts::default()).unwrap();
    wait("download started", 10, || m.get(id).unwrap().task.downloaded > 0).await;
    m.pause(id).unwrap();
    wait("paused", 10, || status(&m, id) == Status::Paused).await;
    assert!(root.join("dl/Compressed/slow-p.zip.part.json").exists());
    m.resume(id).unwrap();
    wait("done", 30, || status(&m, id) == Status::Done).await;
    assert_eq!(sha_file(&root.join("dl/Compressed/slow-p.zip")), expected(size));
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn batch_patterns_duplicates_and_invalid_lines() {
    let (srv, root) = (serve(1000), tmp("mgr-batch"));
    let m = open(&root);
    let text = format!("# my list\n{u}/f[1-3].zip\n{u}/f2.zip\nnot a url\nftp://x/y.zip\n", u = srv.url(""));
    let r = m.add_batch(&text, &AddOpts { paused: true, ..AddOpts::default() });
    assert_eq!((r.added.len(), r.duplicates.len(), r.invalid.len()), (3, 1, 2), "{r:?}");
    assert!(matches!(m.add(&srv.url("/f1.zip"), &AddOpts::default()), Err(Error::Duplicate(_))));
    assert!(m.add(&srv.url("/f1.zip"), &AddOpts { force: true, paused: true, ..AddOpts::default() }).is_ok());
    assert_eq!(m.export_links(&Filter::default()).lines().count(), 4);
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn state_survives_restart_and_running_tasks_requeue() {
    let root = tmp("mgr-persist");
    let m = open(&root);
    let id = m.add("http://127.0.0.1:1/never.zip", &AddOpts { paused: true, ..AddOpts::default() }).unwrap();
    m.shutdown().await;

    let state = root.join("data/state.json");
    let text = fs::read_to_string(&state).unwrap().replace("\"Paused\"", "\"Downloading\""); // as if the app had crashed mid-download
    fs::write(&state, text).unwrap();

    let m2 = open(&root);
    assert_eq!(status(&m2, id), Status::Queued);
    m2.shutdown().await;

    fs::write(&state, "{ broken").unwrap();
    let m3 = open(&root);
    assert!(m3.list(&Filter::default()).is_empty());
    assert!(root.join("data/state.json.bad").exists(), "unreadable state is kept, not deleted");
    m3.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn closed_schedule_blocks_start_until_opened() {
    let (srv, root) = (serve(200_000), tmp("mgr-sched"));
    let m = open(&root);
    let closed = Schedule { days: 0, start: None, stop: None };
    m.save_queue(Queue { schedule: Some(closed), ..Queue::new("Night") });
    let id = m.add(&srv.url("/n.zip"), &AddOpts { queue: Some("Night".into()), ..AddOpts::default() }).unwrap();
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert_eq!(status(&m, id), Status::Queued);
    m.save_queue(Queue::new("Night")); // no schedule any more
    wait("done after opening", 10, || status(&m, id) == Status::Done).await;
    assert!(matches!(m.remove_queue(MAIN_QUEUE), Err(Error::Invalid(_))));
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn drained_event_fires_after_completion_but_not_after_pause() {
    let (srv, root) = (serve(300_000), tmp("mgr-drain"));
    let m = open(&root);
    m.save_queue(Queue { on_finish: FinishAction::Exit, ..Queue::new(MAIN_QUEUE) });
    let mut rx = m.subscribe();

    // pausing the only task must not look like "all done" (that would trigger a shutdown action by mistake)
    let slow = serve(4_000_000);
    let p = m.add(&slow.url("/slow-x.zip"), &AddOpts::default()).unwrap();
    wait("started", 10, || m.get(p).unwrap().task.downloaded > 0).await;
    m.pause(p).unwrap();
    wait("paused", 10, || status(&m, p) == Status::Paused).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    while let Ok(e) = rx.try_recv() {
        assert!(!matches!(e, Event::QueueDrained(..)), "pause must not drain the queue");
    }

    m.remove(p, true).unwrap();
    m.add(&srv.url("/d1.zip"), &AddOpts::default()).unwrap();
    m.add(&srv.url("/d2.zip"), &AddOpts::default()).unwrap();
    let t = Instant::now();
    loop {
        match tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
            Ok(Ok(Event::QueueDrained(q, act))) => {
                assert_eq!((q.as_str(), act), (MAIN_QUEUE, FinishAction::Exit));
                break;
            }
            _ => assert!(t.elapsed() < Duration::from_secs(20), "no drain event"),
        }
    }
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn history_search_and_remove_with_files() {
    let size = 100_000;
    let (srv, root) = (serve(size), tmp("mgr-hist"));
    let m = open(&root);
    let a = m.add(&srv.url("/holiday.mp4"), &AddOpts::default()).unwrap();
    let b = m.add(&srv.url("/report.pdf"), &AddOpts::default()).unwrap();
    wait("both done", 15, || status(&m, a) == Status::Done && status(&m, b) == Status::Done).await;
    let hits = m.list(&Filter { search: Some("HOLIDAY".into()), ..Filter::default() });
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].task.category, "Video");
    assert_eq!(m.list(&Filter { category: Some("Documents".into()), ..Filter::default() }).len(), 1);

    let file = root.join("dl/Video/holiday.mp4");
    assert!(file.exists());
    m.remove(a, true).unwrap();
    assert!(!file.exists() && m.get(a).is_none());
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn global_speed_limit_applies_across_tasks() {
    let (srv, root) = (serve(1_000_000), tmp("mgr-limit"));
    let m = open(&root);
    m.set_settings(Settings { speed_limit: Some(500_000), ..m.settings() });
    let t = Instant::now();
    let id = m.add(&srv.url("/lim.zip"), &AddOpts::default()).unwrap();
    wait("done", 30, || status(&m, id) == Status::Done).await;
    assert!(t.elapsed() >= Duration::from_millis(1400), "finished in {:?}", t.elapsed());
    let _ = srv.hits.load(Relaxed);
    m.shutdown().await;
}
