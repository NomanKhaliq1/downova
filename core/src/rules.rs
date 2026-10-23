use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Category {
    pub name: String,
    /// Lower-case, without the dot.
    pub extensions: Vec<String>,
    /// Sub-folder of the download root (or an absolute path).
    pub folder: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Exclusions {
    pub extensions: Vec<String>,
    pub sites: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SiteRule {
    pub host: String,
    pub folder: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub download_root: PathBuf,
    pub categories: Vec<Category>,
    pub other_folder: String,
    pub site_rules: Vec<SiteRule>,
    pub exclusions: Exclusions,
    pub connections: usize,
    /// Downloads running at the same time across all queues.
    pub max_simultaneous: usize,
    /// App-wide cap in bytes per second.
    pub speed_limit: Option<u64>,
    pub user_agent: Option<String>,
    pub proxy: Option<String>,
    /// Run after each finished download; `{path}` is replaced by the quoted file path (virus scan, move, ...).
    pub on_complete_command: Option<String>,
}

fn cat(name: &str, exts: &str) -> Category {
    Category { name: name.into(), extensions: exts.split_whitespace().map(String::from).collect(), folder: name.into() }
}

impl Default for Settings {
    fn default() -> Self {
        Settings::new(PathBuf::from("Downloads"))
    }
}

impl Settings {
    pub fn new(root: PathBuf) -> Self {
        Settings {
            download_root: root,
            categories: vec![
                cat("Compressed", "zip rar 7z tar gz tgz bz2 xz zst cab"),
                cat("Documents", "pdf doc docx xls xlsx ppt pptx txt rtf odt csv epub md"),
                cat("Music", "mp3 wav flac aac m4a ogg opus wma"),
                cat("Video", "mp4 mkv avi mov wmv flv webm m4v mpg mpeg 3gp"),
                cat("Images", "jpg jpeg png gif webp bmp svg ico tiff heic"),
                cat("Programs", "exe msi msix apk dmg deb rpm appimage iso"),
            ],
            other_folder: "Other".into(),
            site_rules: vec![],
            exclusions: Exclusions::default(),
            connections: 8,
            max_simultaneous: 3,
            speed_limit: None,
            user_agent: None,
            proxy: None,
            on_complete_command: None,
        }
    }

    fn folder(&self, f: &str) -> PathBuf {
        let p = Path::new(f);
        if p.is_absolute() { p.to_path_buf() } else { self.download_root.join(p) }
    }

    /// Category name and target folder for a URL. A matching site rule overrides the category folder.
    pub fn place(&self, url: &str) -> (String, PathBuf) {
        let u = Url::parse(url).ok();
        let ext = u.as_ref().and_then(|u| ext_of(u.path()));
        let cat = ext.as_deref().and_then(|e| self.categories.iter().find(|c| c.extensions.iter().any(|x| x == e)));
        let (name, folder) = match cat {
            Some(c) => (c.name.clone(), c.folder.clone()),
            None => ("Other".to_string(), self.other_folder.clone()),
        };
        let host = u.as_ref().and_then(|u| u.host_str()).unwrap_or("");
        match self.site_rules.iter().find(|r| host_matches(host, &r.host)) {
            Some(r) => (name, self.folder(&r.folder)),
            None => (name, self.folder(&folder)),
        }
    }

    /// Should the browser extension leave this download alone?
    pub fn is_excluded(&self, url: &str) -> bool {
        let Ok(u) = Url::parse(url) else { return false };
        let host = u.host_str().unwrap_or("");
        self.exclusions.sites.iter().any(|s| host_matches(host, s))
            || ext_of(u.path()).is_some_and(|e| self.exclusions.extensions.iter().any(|x| x.trim_start_matches('.').eq_ignore_ascii_case(&e)))
    }
}

fn ext_of(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty()).then(|| ext.to_ascii_lowercase())
}

/// `example.com` matches example.com and any subdomain; a leading `*.` is allowed.
pub fn host_matches(host: &str, pattern: &str) -> bool {
    let p = pattern.trim().trim_start_matches("*.").to_ascii_lowercase();
    let h = host.to_ascii_lowercase();
    !p.is_empty() && (h == p || h.ends_with(&format!(".{p}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s() -> Settings {
        Settings::new(PathBuf::from("R"))
    }

    #[test]
    fn categories_by_extension() {
        let s = s();
        assert_eq!(s.place("https://a.com/x/Movie.MP4?token=1"), ("Video".into(), PathBuf::from("R").join("Video")));
        assert_eq!(s.place("https://a.com/setup.exe").0, "Programs");
        assert_eq!(s.place("https://a.com/download").0, "Other");
        assert_eq!(s.place("https://a.com/.hidden").0, "Other");
        assert_eq!(s.place("not a url").0, "Other");
    }

    #[test]
    fn site_rule_overrides_folder_not_category() {
        let mut s = s();
        s.site_rules.push(SiteRule { host: "work.example.com".into(), folder: "Work".into() });
        assert_eq!(s.place("https://work.example.com/a.zip"), ("Compressed".into(), PathBuf::from("R").join("Work")));
        assert_eq!(s.place("https://example.com/a.zip").1, PathBuf::from("R").join("Compressed"));
    }

    #[test]
    fn exclusions() {
        let mut s = s();
        s.exclusions.sites.push("*.bank.com".into());
        s.exclusions.extensions.push(".torrent".into());
        assert!(s.is_excluded("https://my.bank.com/statement.pdf"));
        assert!(!s.is_excluded("https://notbank.com/a.pdf"));
        assert!(s.is_excluded("https://x.org/f.TORRENT"));
        assert!(!s.is_excluded("https://x.org/f.zip"));
    }
}
