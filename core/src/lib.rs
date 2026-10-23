//! Download engine: probe, segmented download, resume, retry, speed limit.
pub mod engine;
pub mod error;
pub mod limiter;
pub mod probe;
pub mod state;
pub mod util;

pub use engine::{download, Control, Options, Progress};
pub use error::{Error, Result};
