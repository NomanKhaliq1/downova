//! Opt-in clipboard watcher: when the user copies one http(s) link, tell the UI so it can offer to download it.
//! Nothing is ever added without the user's click.
use dm_core::{Filter, Manager};
use std::time::Duration;
use tokio::sync::broadcast;

#[cfg(windows)]
mod win {
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard};
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;

    pub fn sequence() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    pub fn read_text() -> Option<String> {
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return None;
            }
            let h = GetClipboardData(CF_UNICODETEXT);
            let text = if h.is_null() {
                None
            } else {
                let p = GlobalLock(h) as *const u16;
                if p.is_null() {
                    None
                } else {
                    let mut n = 0;
                    while n < 4096 && *p.add(n) != 0 {
                        n += 1;
                    }
                    let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                    GlobalUnlock(h);
                    Some(s)
                }
            };
            CloseClipboard();
            text
        }
    }
}

#[cfg(not(windows))]
mod win {
    pub fn sequence() -> u32 {
        0
    }
    pub fn read_text() -> Option<String> {
        None
    }
}

/// A single http(s) link and nothing else.
pub fn as_link(text: &str) -> Option<String> {
    let t = text.trim();
    let rest = t.strip_prefix("https://").or_else(|| t.strip_prefix("http://"))?;
    (t.len() < 2048 && !t.contains(char::is_whitespace) && !rest.is_empty() && !rest.starts_with('/')).then(|| t.to_string())
}

pub fn spawn(mgr: Manager, tx: broadcast::Sender<String>) {
    tokio::spawn(async move {
        let mut last = win::sequence();
        loop {
            tokio::time::sleep(Duration::from_millis(700)).await;
            if !mgr.settings().clipboard_monitor {
                last = win::sequence(); // links copied while the feature was off must not pop up later
                continue;
            }
            let seq = win::sequence();
            if seq == last {
                continue;
            }
            last = seq;
            let Some(link) = win::read_text().as_deref().and_then(as_link) else { continue };
            let known = mgr.list(&Filter { search: Some(link.clone()), ..Filter::default() }).iter().any(|t| t.task.url == link);
            if !known {
                let _ = tx.send(link);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::as_link;

    #[test]
    fn only_single_links_count() {
        assert_eq!(as_link(" https://a.com/f.zip \n").as_deref(), Some("https://a.com/f.zip"));
        assert_eq!(as_link("see https://a.com/f.zip"), None);
        assert_eq!(as_link("two https://a.com https://b.com"), None);
        assert_eq!(as_link("ftp://a.com/f"), None);
        assert_eq!(as_link("https://"), None);
        assert_eq!(as_link("plain words"), None);
    }
}
