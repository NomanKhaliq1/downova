use std::path::Path;

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// File name from a Content-Disposition header value (filename* wins over filename).
pub fn content_disposition_name(v: &str) -> Option<String> {
    let lower = v.to_ascii_lowercase();
    if let Some(i) = lower.find("filename*=") {
        let rest = v[i + 10..].split(';').next()?.trim();
        // RFC 5987: charset'lang'percent-encoded
        let enc = rest.splitn(3, '\'').nth(2).unwrap_or(rest);
        let name = percent_decode(enc.trim_matches('"'));
        if !name.is_empty() {
            return Some(name);
        }
    }
    let i = lower.find("filename=")?;
    let name = v[i + 9..].split(';').next()?.trim().trim_matches('"').to_string();
    (!name.is_empty()).then_some(name)
}

/// Last path segment of a URL, percent-decoded.
pub fn url_name(url: &reqwest::Url) -> Option<String> {
    let seg = url.path_segments()?.next_back()?;
    let name = percent_decode(seg);
    (!name.is_empty()).then_some(name)
}

/// Make a name safe for Windows (and every other file system).
pub fn sanitize(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_control() || "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    s = s.trim().trim_end_matches(['.', ' ']).to_string();
    if s.chars().count() > 180 {
        let ext = Path::new(&s).extension().and_then(|e| e.to_str()).filter(|e| e.len() <= 10).map(str::to_string);
        let stem: String = s.chars().take(170).collect();
        s = match ext {
            Some(e) => format!("{stem}.{e}"),
            None => stem,
        };
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if s.is_empty() {
        "download".into()
    } else if reserved {
        format!("_{s}")
    } else {
        s
    }
}

/// `name`, `name (1)`, `name (2)` ... inserting the counter before the extension.
pub fn numbered(name: &str, n: u32) -> String {
    if n == 0 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_variants() {
        assert_eq!(content_disposition_name(r#"attachment; filename="my file.bin""#).as_deref(), Some("my file.bin"));
        assert_eq!(content_disposition_name("attachment; filename=a.zip").as_deref(), Some("a.zip"));
        assert_eq!(
            content_disposition_name("attachment; filename=\"fallback.txt\"; filename*=UTF-8''na%C3%AFve%20file.txt").as_deref(),
            Some("naïve file.txt")
        );
        assert_eq!(content_disposition_name("inline"), None);
    }

    #[test]
    fn sanitize_rules() {
        assert_eq!(sanitize("a/b:c*.txt"), "a_b_c_.txt");
        assert_eq!(sanitize("  report.pdf.. "), "report.pdf");
        assert_eq!(sanitize("con.txt"), "_con.txt");
        assert_eq!(sanitize("COM1"), "_COM1");
        assert_eq!(sanitize(""), "download");
        assert!(sanitize(&"x".repeat(400)).chars().count() <= 180);
    }

    #[test]
    fn numbering_keeps_extension() {
        assert_eq!(numbered("a.zip", 0), "a.zip");
        assert_eq!(numbered("a.tar.gz", 2), "a.tar (2).gz");
        assert_eq!(numbered("noext", 1), "noext (1)");
    }

    #[test]
    fn percent_decode_edges() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%4"), "%4");
    }
}
