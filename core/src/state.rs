use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

/// Byte range [start, end] (inclusive). `pos` is the next byte to fetch; done when pos > end.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Seg {
    pub start: u64,
    pub end: u64,
    pub pos: u64,
}

impl Seg {
    pub fn remaining(&self) -> u64 {
        (self.end + 1).saturating_sub(self.pos)
    }
}

/// What is written to `<file>.part.json` so a download can resume after a crash or reboot.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct State {
    pub url: String,
    pub total: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub segs: Vec<Seg>,
}

impl State {
    pub fn downloaded(&self) -> u64 {
        self.segs.iter().map(|s| s.pos - s.start).sum()
    }
}

/// Split `total` bytes (> 0) into at most `n` equal ranges, none smaller than `min_split`.
pub fn initial_segments(total: u64, n: usize, min_split: u64) -> Vec<Seg> {
    let n = (total / min_split.max(1)).clamp(1, n.max(1) as u64);
    let chunk = total.div_ceil(n);
    (0..n)
        .map(|i| Seg { start: i * chunk, end: ((i + 1) * chunk).min(total) - 1, pos: i * chunk })
        .filter(|s| s.start <= s.end)
        .collect()
}

pub fn load_state(path: &Path) -> Option<State> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// Write to a temp file then rename, so a crash never leaves half a state file.
pub fn save_state(path: &Path, state: &State) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec(state)?)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_cover_everything_without_overlap() {
        for (total, n, min) in [(10, 4, 1), (1, 8, 1), (5_000_000, 8, 1 << 20), (7, 3, 1), (100, 8, 1000)] {
            let segs = initial_segments(total, n, min);
            assert_eq!(segs[0].start, 0);
            assert_eq!(segs.last().unwrap().end, total - 1);
            for w in segs.windows(2) {
                assert_eq!(w[0].end + 1, w[1].start);
            }
            assert!(segs.len() <= n);
        }
    }

    #[test]
    fn small_files_get_one_segment() {
        assert_eq!(initial_segments(100, 8, 1 << 20).len(), 1);
    }
}
