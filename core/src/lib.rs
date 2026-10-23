//! Download engine: probe, segmented download, resume, retry, speed limit.
//! On top of it: queues, scheduler, categories, history.
pub mod engine;
pub mod error;
pub mod limiter;
pub mod manager;
pub mod pattern;
pub mod power;
pub mod probe;
pub mod rules;
pub mod schedule;
pub mod state;
pub mod util;

pub use engine::{download, Control, Options, Progress};
pub use error::{Error, Result};
pub use manager::{AddOpts, BatchResult, Event, Filter, Manager, Queue, Status, Task, TaskOpts, TaskView, MAIN_QUEUE};
pub use rules::Settings;
pub use schedule::{FinishAction, Schedule};
