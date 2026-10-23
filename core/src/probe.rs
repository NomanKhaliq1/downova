use crate::error::{Error, Result};
use crate::util::{content_disposition_name, sanitize, url_name};
use reqwest::header::{HeaderMap, CONTENT_DISPOSITION, CONTENT_RANGE, CONTENT_TYPE, ETAG, LAST_MODIFIED, RANGE};
use reqwest::{Client, StatusCode};

#[derive(Debug, Clone)]
pub struct Probe {
    pub final_url: String,
    pub total: Option<u64>,
    /// Server honours Range requests and told us the full size.
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub filename: String,
    pub content_type: Option<String>,
}

impl Probe {
    /// Value for `If-Range`: a strong ETag, else Last-Modified.
    pub fn validator(&self) -> Option<String> {
        match &self.etag {
            Some(e) if !e.starts_with("W/") => Some(e.clone()),
            _ => self.last_modified.clone(),
        }
    }
}

pub fn status_error(s: StatusCode) -> Error {
    if s.is_server_error() || s == StatusCode::TOO_MANY_REQUESTS || s == StatusCode::REQUEST_TIMEOUT {
        Error::Transient(s.to_string())
    } else {
        Error::Server(s.to_string())
    }
}

fn header(h: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    h.get(name)?.to_str().ok().map(str::to_string)
}

/// "bytes 0-0/1234" -> (0, Some(1234)); "bytes 5-9/*" -> (5, None)
pub fn parse_content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let rest = v.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let start = range.split('-').next()?.trim().parse().ok()?;
    Some((start, total.trim().parse().ok()))
}

/// One tiny ranged GET: tells us size, resume support, validators and the file name.
pub async fn probe(client: &Client, url: &str) -> Result<Probe> {
    let resp = client.get(url).header(RANGE, "bytes=0-0").send().await?;
    let status = resp.status();
    let h = resp.headers().clone();
    let final_url = resp.url().clone();

    let (total, resumable) = if status == StatusCode::PARTIAL_CONTENT {
        let total = header(&h, CONTENT_RANGE).and_then(|v| parse_content_range(&v)).and_then(|(_, t)| t);
        (total, total.is_some())
    } else if status == StatusCode::RANGE_NOT_SATISFIABLE {
        (Some(0), false) // empty file
    } else if status.is_success() {
        (resp.content_length(), false)
    } else {
        return Err(status_error(status));
    };
    drop(resp);

    let filename = header(&h, CONTENT_DISPOSITION)
        .and_then(|v| content_disposition_name(&v))
        .or_else(|| url_name(&final_url))
        .unwrap_or_default();
    Ok(Probe {
        final_url: final_url.to_string(),
        total,
        resumable,
        etag: header(&h, ETAG),
        last_modified: header(&h, LAST_MODIFIED),
        filename: sanitize(&filename),
        content_type: header(&h, CONTENT_TYPE),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range() {
        assert_eq!(parse_content_range("bytes 0-0/1234"), Some((0, Some(1234))));
        assert_eq!(parse_content_range("bytes 5-9/*"), Some((5, None)));
        assert_eq!(parse_content_range("garbage"), None);
    }
}
