#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("disk: {0}")]
    Io(#[from] std::io::Error),
    #[error("state file: {0}")]
    Json(#[from] serde_json::Error),
    /// Permanent server answer (404, 403, bad header...). Retrying will not help.
    #[error("server: {0}")]
    Server(String),
    /// Temporary server answer (5xx, 429, short body). Worth retrying.
    #[error("temporary server problem: {0}")]
    Transient(String),
    #[error("the file changed on the server, restart the download")]
    Changed,
    #[error("cancelled")]
    Cancelled,
    #[error("checksum does not match")]
    Checksum,
    #[error("not enough free disk space")]
    DiskSpace,
    #[error("already in the list (task {0})")]
    Duplicate(u64),
    #[error("no such task or queue")]
    NotFound,
    #[error("invalid input: {0}")]
    Invalid(String),
}

impl Error {
    pub fn retryable(&self) -> bool {
        matches!(self, Error::Http(_) | Error::Transient(_))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
