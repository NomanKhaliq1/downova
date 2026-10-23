use crate::error::{Error, Result};

const MAX_URLS: usize = 10_000;

enum Part {
    Lit(String),
    Range(Vec<String>),
}

/// "[001-005]" "[1-10:2]" "[a-e]" ranges inside a URL; several groups multiply out.
/// Anything in brackets that is not a range (an IPv6 host, for example) stays literal.
fn parse_range(body: &str) -> Result<Option<Vec<String>>> {
    let (range, step) = match body.split_once(':') {
        Some((r, s)) => match s.parse::<u64>() {
            Ok(n) if n > 0 => (r, n),
            _ => return Ok(None),
        },
        None => (body, 1),
    };
    let Some((a, b)) = range.split_once('-') else { return Ok(None) };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    if digits(a) && digits(b) {
        let (lo, hi): (u64, u64) = match (a.parse(), b.parse()) {
            (Ok(x), Ok(y)) => (x, y),
            _ => return Ok(None),
        };
        if lo > hi {
            return Ok(None);
        }
        if (hi - lo) / step + 1 > MAX_URLS as u64 {
            return Err(Error::Invalid(format!("[{body}] makes more than {MAX_URLS} links")));
        }
        let width = if a.len() > 1 && a.starts_with('0') { a.len() } else { 0 };
        return Ok(Some((lo..=hi).step_by(step as usize).map(|n| format!("{n:0width$}")).collect()));
    }
    let letter = |s: &str| s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic();
    if letter(a) && letter(b) && a.as_bytes()[0].is_ascii_lowercase() == b.as_bytes()[0].is_ascii_lowercase() {
        let (lo, hi) = (a.as_bytes()[0], b.as_bytes()[0]);
        if lo <= hi {
            return Ok(Some((lo..=hi).step_by(step as usize).map(|c| (c as char).to_string()).collect()));
        }
    }
    Ok(None)
}

pub fn expand(input: &str) -> Result<Vec<String>> {
    let mut parts = vec![];
    let mut rest = input;
    while let Some(i) = rest.find('[') {
        let Some(j) = rest[i..].find(']') else { break };
        match parse_range(&rest[i + 1..i + j])? {
            Some(vals) => {
                parts.push(Part::Lit(rest[..i].to_string()));
                parts.push(Part::Range(vals));
            }
            None => parts.push(Part::Lit(rest[..i + j + 1].to_string())),
        }
        rest = &rest[i + j + 1..];
    }
    parts.push(Part::Lit(rest.to_string()));

    let mut out = vec![String::new()];
    for p in parts {
        match p {
            Part::Lit(s) => out.iter_mut().for_each(|o| o.push_str(&s)),
            Part::Range(vals) => {
                if out.len() * vals.len() > MAX_URLS {
                    return Err(Error::Invalid(format!("more than {MAX_URLS} links")));
                }
                out = out.iter().flat_map(|o| vals.iter().map(move |v| format!("{o}{v}"))).collect();
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_padded_and_plain() {
        assert_eq!(expand("a/f[001-003].jpg").unwrap(), ["a/f001.jpg", "a/f002.jpg", "a/f003.jpg"]);
        assert_eq!(expand("a/[8-11]").unwrap(), ["a/8", "a/9", "a/10", "a/11"]);
    }

    #[test]
    fn steps_letters_and_two_groups() {
        assert_eq!(expand("p[1-7:3]").unwrap(), ["p1", "p4", "p7"]);
        assert_eq!(expand("x[a-c]").unwrap(), ["xa", "xb", "xc"]);
        assert_eq!(expand("[1-2]-[a-b]").unwrap(), ["1-a", "1-b", "2-a", "2-b"]);
    }

    #[test]
    fn non_ranges_stay_literal() {
        assert_eq!(expand("http://[::1]:8080/f.zip").unwrap(), ["http://[::1]:8080/f.zip"]);
        assert_eq!(expand("a[5-2]").unwrap(), ["a[5-2]"]);
        assert_eq!(expand("plain").unwrap(), ["plain"]);
        assert_eq!(expand("broken[1-3").unwrap(), ["broken[1-3"]);
    }

    #[test]
    fn huge_ranges_are_refused() {
        assert!(expand("f[1-999999999]").is_err());
        assert!(expand("[1-200][1-200]").is_err());
    }
}
