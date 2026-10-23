//! Queues, scheduler, categories and history on top of the engine.
use crate::engine::{download, Control, Options};
use crate::error::{Error, Result};
use crate::limiter::Limiter;
use crate::pattern::expand;
use crate::rules::Settings;
use crate::schedule::{local_now, FinishAction, Schedule};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

pub const MAIN_QUEUE: &str = "Main";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Queued,
    Downloading,
    Paused,
    Done,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct TaskOpts {
    pub connections: Option<usize>,
    pub speed_limit: Option<u64>,
    pub referer: Option<String>,
    pub cookie: Option<String>,
    pub headers: Vec<(String, String)>,
    pub user_agent: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Task {
    pub id: u64,
    pub url: String,
    pub dir: PathBuf,
    pub category: String,
    pub queue: String,
    pub status: Status,
    /// Empty until the engine has picked a file name.
    pub name: String,
    pub path: Option<PathBuf>,
    pub total: u64,
    pub downloaded: u64,
    pub error: Option<String>,
    pub added: u64,
    pub finished: Option<u64>,
    pub priority: i32,
    pub opts: TaskOpts,
}

/// A task plus live numbers for the UI.
#[derive(Serialize, Clone, Debug)]
pub struct TaskView {
    #[serde(flatten)]
    pub task: Task,
    pub speed: u64,
    pub eta_secs: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Queue {
    pub name: String,
    pub max_simultaneous: usize,
    pub enabled: bool,
    pub schedule: Option<Schedule>,
    pub on_finish: FinishAction,
}

impl Queue {
    pub fn new(name: &str) -> Self {
        Queue { name: name.into(), max_simultaneous: 3, enabled: true, schedule: None, on_finish: FinishAction::None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Added(u64),
    Changed(u64),
    Removed(u64),
    Finished(u64),
    /// Everything in the queue is done; the app should carry out the queue's finish action.
    QueueDrained(String, FinishAction),
}

#[derive(Default, Clone, Debug)]
pub struct AddOpts {
    pub queue: Option<String>,
    /// Overrides category and site rules.
    pub dir: Option<PathBuf>,
    /// Add as paused instead of starting right away.
    pub paused: bool,
    /// Add even if the URL is already in the list.
    pub force: bool,
    pub task: TaskOpts,
}

#[derive(Default, Debug)]
pub struct BatchResult {
    pub added: Vec<u64>,
    pub duplicates: Vec<u64>,
    pub invalid: Vec<String>,
}

#[derive(Default, Clone, Debug)]
pub struct Filter {
    pub status: Option<Status>,
    pub category: Option<String>,
    pub queue: Option<String>,
    /// Matches the name or the URL, ignoring case.
    pub search: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Persisted {
    settings: Settings,
    tasks: Vec<Task>,
    queues: Vec<Queue>,
    next_id: u64,
}

struct Run {
    ctl: Control,
    speed: f64,
    last: (Instant, u64),
}

struct State {
    settings: Settings,
    tasks: BTreeMap<u64, Task>,
    queues: Vec<Queue>,
    next_id: u64,
    running: HashMap<u64, Run>,
    /// Stopped because its queue's schedule closed: goes back to Queued, not Paused.
    requeue: HashSet<u64>,
    /// Removal requested while running; the bool says whether to delete files.
    to_remove: HashMap<u64, bool>,
    busy_queues: HashSet<String>,
    /// Queues where something completed since they last went idle.
    completed_in: HashSet<String>,
    dirty: bool,
    last_save: Instant,
}

struct Shared {
    state: Mutex<State>,
    events: broadcast::Sender<Event>,
    path: PathBuf,
    global: Arc<Limiter>,
    stop: AtomicBool,
}

#[derive(Clone)]
pub struct Manager(Arc<Shared>);

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn valid_url(u: &str) -> bool {
    Url::parse(u).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

impl Manager {
    /// Load (or create) the state in `data_dir` and start the scheduler. Needs a running tokio runtime.
    pub fn open(data_dir: PathBuf, download_root: PathBuf) -> Result<Manager> {
        std::fs::create_dir_all(&data_dir)?;
        let path = data_dir.join("state.json");
        let loaded = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Persisted>(&bytes) {
                Ok(p) => Some(p),
                Err(_) => {
                    // never silently drop a user's list: keep the unreadable file next to the new one
                    let _ = std::fs::rename(&path, data_dir.join("state.json.bad"));
                    None
                }
            },
            Err(_) => None,
        };
        let mut p = loaded.unwrap_or_else(|| Persisted {
            settings: Settings::new(download_root),
            tasks: vec![],
            queues: vec![],
            next_id: 1,
        });
        if !p.queues.iter().any(|q| q.name == MAIN_QUEUE) {
            p.queues.insert(0, Queue::new(MAIN_QUEUE));
        }
        for t in &mut p.tasks {
            if t.status == Status::Downloading {
                t.status = Status::Queued; // the app died while it was running
            }
        }
        let global = Arc::new(Limiter::new(p.settings.speed_limit));
        let (events, _) = broadcast::channel(256);
        let m = Manager(Arc::new(Shared {
            state: Mutex::new(State {
                settings: p.settings,
                tasks: p.tasks.into_iter().map(|t| (t.id, t)).collect(),
                queues: p.queues,
                next_id: p.next_id,
                running: HashMap::new(),
                requeue: HashSet::new(),
                to_remove: HashMap::new(),
                busy_queues: HashSet::new(),
                completed_in: HashSet::new(),
                dirty: true,
                last_save: Instant::now(),
            }),
            events,
            path,
            global,
            stop: AtomicBool::new(false),
        }));
        let m2 = m.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(500));
            while !m2.0.stop.load(Relaxed) {
                iv.tick().await;
                m2.tick();
            }
        });
        Ok(m)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.0.events.subscribe()
    }

    fn emit(&self, e: Event) {
        let _ = self.0.events.send(e);
    }

    /// Stop everything, keep partial files, write the state. Call before the app exits.
    pub async fn shutdown(&self) {
        self.0.stop.store(true, Relaxed);
        {
            let s = self.0.state.lock().unwrap();
            s.running.values().for_each(|r| r.ctl.cancel.store(true, Relaxed));
        }
        for _ in 0..100 {
            if self.0.state.lock().unwrap().running.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut s = self.0.state.lock().unwrap();
        self.save(&mut s);
    }

    fn save(&self, s: &mut State) {
        let p = Persisted {
            settings: s.settings.clone(),
            tasks: s.tasks.values().cloned().collect(),
            queues: s.queues.clone(),
            next_id: s.next_id,
        };
        let tmp = self.0.path.with_extension("json.tmp");
        // shortcut: whole-file JSON, fine for thousands of tasks; move to SQLite if history gets huge
        if serde_json::to_vec(&p).ok().and_then(|b| std::fs::write(&tmp, b).ok()).is_some() && std::fs::rename(&tmp, &self.0.path).is_ok() {
            s.dirty = false;
        }
        s.last_save = Instant::now();
    }

    // ---- adding ----

    pub fn add(&self, url: &str, o: &AddOpts) -> Result<u64> {
        let url = url.trim();
        if !valid_url(url) {
            return Err(Error::Invalid(format!("not an http(s) link: {url}")));
        }
        let id = {
            let mut s = self.0.state.lock().unwrap();
            if !o.force {
                if let Some(t) = s.tasks.values().find(|t| t.url == url) {
                    return Err(Error::Duplicate(t.id));
                }
            }
            let queue = o.queue.clone().unwrap_or_else(|| MAIN_QUEUE.into());
            if !s.queues.iter().any(|q| q.name == queue) {
                return Err(Error::NotFound);
            }
            let (category, mut dir) = s.settings.place(url);
            if let Some(d) = &o.dir {
                dir = d.clone();
            }
            let id = s.next_id;
            s.next_id += 1;
            s.tasks.insert(
                id,
                Task {
                    id,
                    url: url.into(),
                    dir,
                    category,
                    queue,
                    status: if o.paused { Status::Paused } else { Status::Queued },
                    name: String::new(),
                    path: None,
                    total: 0,
                    downloaded: 0,
                    error: None,
                    added: now_secs(),
                    finished: None,
                    priority: 0,
                    opts: o.task.clone(),
                },
            );
            s.dirty = true;
            id
        };
        self.emit(Event::Added(id));
        Ok(id)
    }

    /// One link or pattern per line (`#` starts a comment line). Patterns like `f[001-050].jpg` expand.
    pub fn add_batch(&self, text: &str, o: &AddOpts) -> BatchResult {
        let mut r = BatchResult::default();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let urls = match expand(line) {
                Ok(u) => u,
                Err(_) => {
                    r.invalid.push(line.into());
                    continue;
                }
            };
            for u in urls {
                match self.add(&u, o) {
                    Ok(id) => r.added.push(id),
                    Err(Error::Duplicate(id)) => r.duplicates.push(id),
                    Err(_) => r.invalid.push(u),
                }
            }
        }
        r
    }

    pub fn export_links(&self, f: &Filter) -> String {
        self.list(f).into_iter().map(|v| v.task.url).collect::<Vec<_>>().join("\n")
    }

    // ---- control ----

    pub fn pause(&self, id: u64) -> Result<()> {
        let mut s = self.0.state.lock().unwrap();
        let st = s.tasks.get(&id).ok_or(Error::NotFound)?.status;
        match st {
            Status::Downloading => {
                if let Some(r) = s.running.get(&id) {
                    r.ctl.cancel.store(true, Relaxed);
                }
                s.requeue.remove(&id);
            }
            Status::Queued => {
                s.tasks.get_mut(&id).unwrap().status = Status::Paused;
                s.dirty = true;
                drop(s);
                self.emit(Event::Changed(id));
            }
            _ => {}
        }
        Ok(())
    }

    /// Resume a paused task or retry a failed one.
    pub fn resume(&self, id: u64) -> Result<()> {
        let mut s = self.0.state.lock().unwrap();
        let t = s.tasks.get_mut(&id).ok_or(Error::NotFound)?;
        if matches!(t.status, Status::Paused | Status::Failed) {
            t.status = Status::Queued;
            t.error = None;
            s.dirty = true;
            drop(s);
            self.emit(Event::Changed(id));
        }
        Ok(())
    }

    pub fn pause_all(&self) {
        let ids: Vec<u64> = self.0.state.lock().unwrap().tasks.keys().copied().collect();
        ids.into_iter().for_each(|id| {
            let _ = self.pause(id);
        });
    }

    pub fn resume_all(&self) {
        let ids: Vec<u64> = self.0.state.lock().unwrap().tasks.values().filter(|t| t.status == Status::Paused).map(|t| t.id).collect();
        ids.into_iter().for_each(|id| {
            let _ = self.resume(id);
        });
    }

    pub fn remove(&self, id: u64, delete_files: bool) -> Result<()> {
        let mut s = self.0.state.lock().unwrap();
        if !s.tasks.contains_key(&id) {
            return Err(Error::NotFound);
        }
        if let Some(r) = s.running.get(&id) {
            r.ctl.cancel.store(true, Relaxed); // the run handler finishes the removal
            s.to_remove.insert(id, delete_files);
            return Ok(());
        }
        Self::drop_task(&mut s, id, delete_files);
        drop(s);
        self.emit(Event::Removed(id));
        Ok(())
    }

    fn drop_task(s: &mut State, id: u64, delete_files: bool) {
        if let Some(t) = s.tasks.remove(&id) {
            if delete_files && !t.name.is_empty() {
                let _ = match (&t.status, &t.path) {
                    (Status::Done, Some(p)) => std::fs::remove_file(p),
                    _ => std::fs::remove_file(t.dir.join(format!("{}.part", t.name))),
                };
                let _ = std::fs::remove_file(t.dir.join(format!("{}.part.json", t.name)));
            }
            s.dirty = true;
        }
    }

    /// Highest first: `ids[0]` runs before `ids[1]`, and so on.
    pub fn reorder(&self, ids: &[u64]) {
        let mut s = self.0.state.lock().unwrap();
        for (i, id) in ids.iter().enumerate() {
            if let Some(t) = s.tasks.get_mut(id) {
                t.priority = (ids.len() - i) as i32;
            }
        }
        s.dirty = true;
    }

    pub fn set_task_queue(&self, id: u64, queue: &str) -> Result<()> {
        let mut s = self.0.state.lock().unwrap();
        if !s.queues.iter().any(|q| q.name == queue) {
            return Err(Error::NotFound);
        }
        s.tasks.get_mut(&id).ok_or(Error::NotFound)?.queue = queue.into();
        s.dirty = true;
        Ok(())
    }

    // ---- queues and settings ----

    pub fn queues(&self) -> Vec<Queue> {
        self.0.state.lock().unwrap().queues.clone()
    }

    /// Create a queue, or replace the one with the same name.
    pub fn save_queue(&self, q: Queue) {
        let mut s = self.0.state.lock().unwrap();
        match s.queues.iter_mut().find(|x| x.name == q.name) {
            Some(x) => *x = q,
            None => s.queues.push(q),
        }
        s.dirty = true;
    }

    /// Tasks of a removed queue move to Main. Main itself cannot be removed.
    pub fn remove_queue(&self, name: &str) -> Result<()> {
        if name == MAIN_QUEUE {
            return Err(Error::Invalid("the Main queue cannot be removed".into()));
        }
        let mut s = self.0.state.lock().unwrap();
        s.queues.retain(|q| q.name != name);
        s.tasks.values_mut().filter(|t| t.queue == name).for_each(|t| t.queue = MAIN_QUEUE.into());
        s.dirty = true;
        Ok(())
    }

    pub fn settings(&self) -> Settings {
        self.0.state.lock().unwrap().settings.clone()
    }

    pub fn set_settings(&self, new: Settings) {
        self.0.global.set_rate(new.speed_limit);
        let mut s = self.0.state.lock().unwrap();
        s.settings = new;
        s.dirty = true;
    }

    // ---- reading ----

    pub fn list(&self, f: &Filter) -> Vec<TaskView> {
        let s = self.0.state.lock().unwrap();
        let q = f.search.as_ref().map(|q| q.to_lowercase());
        s.tasks
            .values()
            .filter(|t| f.status.is_none_or(|x| x == t.status))
            .filter(|t| f.category.as_ref().is_none_or(|c| c == &t.category))
            .filter(|t| f.queue.as_ref().is_none_or(|c| c == &t.queue))
            .filter(|t| q.as_ref().is_none_or(|q| t.name.to_lowercase().contains(q) || t.url.to_lowercase().contains(q)))
            .map(|t| Self::view(&s, t))
            .collect()
    }

    pub fn get(&self, id: u64) -> Option<TaskView> {
        let s = self.0.state.lock().unwrap();
        s.tasks.get(&id).map(|t| Self::view(&s, t))
    }

    fn view(s: &State, t: &Task) -> TaskView {
        let speed = s.running.get(&t.id).map_or(0, |r| r.speed as u64);
        let eta_secs = (speed > 0 && t.total > t.downloaded).then(|| (t.total - t.downloaded) / speed);
        TaskView { task: t.clone(), speed, eta_secs }
    }

    // ---- the scheduler tick ----

    fn tick(&self) {
        let (day, minute) = local_now();
        let mut starts = vec![];
        let mut events = vec![];
        {
            let mut guard = self.0.state.lock().unwrap();
            let s = &mut *guard;

            // copy live numbers into the tasks and smooth the speed
            let now = Instant::now();
            for (id, r) in s.running.iter_mut() {
                let d = r.ctl.progress.downloaded.load(Relaxed);
                let total = r.ctl.progress.total.load(Relaxed);
                let dt = now.duration_since(r.last.0).as_secs_f64();
                if dt >= 0.5 {
                    let inst = d.saturating_sub(r.last.1) as f64 / dt;
                    r.speed = if r.speed == 0.0 { inst } else { r.speed * 0.6 + inst * 0.4 };
                    r.last = (now, d);
                }
                if let Some(t) = s.tasks.get_mut(id) {
                    t.downloaded = d;
                    t.total = total;
                    if t.name.is_empty() {
                        t.name = r.ctl.progress.name.lock().unwrap().clone();
                    }
                }
            }

            for q in s.queues.clone() {
                let open = q.enabled && q.schedule.as_ref().is_none_or(|sc| sc.is_open(day, minute));
                let in_queue = |s: &State, id: &u64| s.tasks.get(id).is_some_and(|t| t.queue == q.name);
                let running: Vec<u64> = s.running.keys().filter(|id| in_queue(s, id)).copied().collect();

                if !open {
                    for id in running {
                        s.requeue.insert(id);
                        s.running[&id].ctl.cancel.store(true, Relaxed);
                    }
                } else {
                    let free = q.max_simultaneous.saturating_sub(running.len()).min(s.settings.max_simultaneous.saturating_sub(s.running.len()));
                    let mut ready: Vec<u64> = s.tasks.values().filter(|t| t.queue == q.name && t.status == Status::Queued).map(|t| t.id).collect();
                    ready.sort_by_key(|id| (std::cmp::Reverse(s.tasks[id].priority), *id));
                    for id in ready.into_iter().take(free) {
                        let t = s.tasks.get_mut(&id).unwrap();
                        t.status = Status::Downloading;
                        let o = Self::options(&s.settings, &t.opts);
                        let ctl = Control::new(&o).with_global(self.0.global.clone());
                        starts.push((id, t.url.clone(), t.dir.clone(), o, ctl.clone()));
                        s.running.insert(id, Run { ctl, speed: 0.0, last: (now, 0) });
                        s.dirty = true;
                        events.push(Event::Changed(id));
                    }
                }

                // drained: the queue was busy, now has nothing queued or running, and something completed meanwhile
                let busy = s.tasks.values().any(|t| t.queue == q.name && matches!(t.status, Status::Queued | Status::Downloading));
                if busy {
                    s.busy_queues.insert(q.name.clone());
                } else if s.busy_queues.remove(&q.name) && s.completed_in.remove(&q.name) {
                    events.push(Event::QueueDrained(q.name.clone(), q.on_finish.clone()));
                }
            }

            if s.dirty && s.last_save.elapsed() >= Duration::from_secs(1) {
                self.save(s);
            }
        }
        events.into_iter().for_each(|e| self.emit(e));
        for (id, url, dir, o, ctl) in starts {
            tokio::spawn(self.clone().run(id, url, dir, o, ctl));
        }
    }

    fn options(s: &Settings, t: &TaskOpts) -> Options {
        Options {
            connections: t.connections.unwrap_or(s.connections),
            headers: t.headers.clone(),
            user_agent: t.user_agent.clone().or_else(|| s.user_agent.clone()),
            referer: t.referer.clone(),
            cookie: t.cookie.clone(),
            proxy: s.proxy.clone(),
            speed_limit: t.speed_limit,
            sha256: t.sha256.clone(),
            ..Options::default()
        }
    }

    async fn run(self, id: u64, url: String, dir: PathBuf, o: Options, ctl: Control) {
        let r = download(&url, &dir, &o, &ctl).await;
        let mut events = vec![Event::Changed(id)];
        let mut hook = None;
        {
            let mut guard = self.0.state.lock().unwrap();
            let s = &mut *guard; // plain &mut State so the fields can be borrowed separately
            s.running.remove(&id);
            let requeue = s.requeue.remove(&id);
            let mut completed = false;
            if let Some(t) = s.tasks.get_mut(&id) {
                match &r {
                    Ok(p) => {
                        t.status = Status::Done;
                        t.path = Some(p.clone());
                        t.name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        t.total = std::fs::metadata(p).map_or(t.total, |m| m.len());
                        t.downloaded = t.total;
                        t.error = None;
                        t.finished = Some(now_secs());
                        completed = true;
                        events.push(Event::Finished(id));
                        hook = s.settings.on_complete_command.clone().map(|c| (c, p.clone()));
                    }
                    Err(Error::Cancelled) => t.status = if requeue { Status::Queued } else { Status::Paused },
                    Err(e) => {
                        t.status = Status::Failed;
                        t.error = Some(e.to_string());
                        t.finished = Some(now_secs());
                        completed = true;
                    }
                }
                let q = t.queue.clone();
                if completed {
                    s.completed_in.insert(q);
                }
            }
            if let Some(del) = s.to_remove.remove(&id) {
                Self::drop_task(s, id, del);
                events = vec![Event::Removed(id)];
            }
            s.dirty = true;
        }
        events.into_iter().for_each(|e| self.emit(e));
        if let Some((cmd, path)) = hook {
            run_hook(&cmd, &path);
        }
    }
}

/// Fire-and-forget user command after a download; `{path}` becomes the quoted file path.
fn run_hook(cmd: &str, path: &std::path::Path) {
    let line = cmd.replace("{path}", &format!("\"{}\"", path.display()));
    #[cfg(windows)]
    let mut c = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", &line]);
        c
    };
    #[cfg(not(windows))]
    let mut c = {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", &line]);
        c
    };
    let _ = c.spawn();
}
