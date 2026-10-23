//! HLS (.m3u8) playlist parsing: master playlists, media playlists, AES-128 keys, init segments, byte ranges.
use crate::error::{Error, Result};
use reqwest::Url;

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub url: Url,
    pub bandwidth: u64,
    pub height: Option<u32>,
    pub audio_group: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioRendition {
    pub group: String,
    pub url: Option<Url>,
    pub default: bool,
}

/// AES-128 (CBC) key reference. `iv` is None when the playlist leaves it out: then the segment number is the IV.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub url: Url,
    pub iv: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub url: Url,
    pub duration: f64,
    pub seq: u64,
    pub key: Option<Key>,
    /// (offset, length) inside the resource, for #EXT-X-BYTERANGE.
    pub range: Option<(u64, u64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    pub segments: Vec<Segment>,
    pub init: Option<Segment>,
    /// #EXT-X-ENDLIST seen: a finished recording, not a live stream.
    pub ended: bool,
    pub fmp4: bool,
}

#[derive(Debug, PartialEq)]
pub enum Playlist {
    Master { variants: Vec<Variant>, audio: Vec<AudioRendition> },
    Media(Media),
}

/// `KEY=value,KEY2="quoted, with comma"` -> pairs (keys upper-cased, quotes removed).
pub fn attrs(s: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let (mut key, mut val, mut in_key, mut quoted) = (String::new(), String::new(), true, false);
    let mut flush = |k: &mut String, v: &mut String| {
        if !k.trim().is_empty() {
            out.push((k.trim().to_ascii_uppercase(), v.trim().to_string()));
        }
        k.clear();
        v.clear();
    };
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            '=' if in_key && !quoted => in_key = false,
            ',' if !quoted => {
                flush(&mut key, &mut val);
                in_key = true;
            }
            _ if in_key => key.push(c),
            _ => val.push(c),
        }
    }
    flush(&mut key, &mut val);
    out
}

fn get<'a>(a: &'a [(String, String)], k: &str) -> Option<&'a str> {
    a.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn bad(msg: &str) -> Error {
    Error::Invalid(msg.into())
}

fn join(base: &Url, rel: &str) -> Result<Url> {
    base.join(rel.trim()).map_err(|_| bad("playlist contains a broken link"))
}

fn parse_iv(s: &str) -> Option<[u8; 16]> {
    let h = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if h.len() > 32 || h.is_empty() || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let padded = format!("{h:0>32}");
    let mut iv = [0u8; 16];
    for (i, b) in iv.iter_mut().enumerate() {
        *b = u8::from_str_radix(&padded[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(iv)
}

/// "len@offset" or "len" (offset continues after the previous range).
fn parse_range(s: &str, prev_end: Option<u64>) -> Option<(u64, u64)> {
    let (len, off) = match s.split_once('@') {
        Some((l, o)) => (l.trim().parse().ok()?, o.trim().parse().ok()?),
        None => (s.trim().parse().ok()?, prev_end?),
    };
    Some((off, len))
}

pub fn parse(text: &str, base: &Url) -> Result<Playlist> {
    let text = text.trim_start_matches('\u{feff}');
    if !text.trim_start().starts_with("#EXTM3U") {
        return Err(bad("this is not an HLS playlist"));
    }
    let mut variants = vec![];
    let mut audio = vec![];
    let mut pending_variant: Option<(u64, Option<u32>, Option<String>)> = None;

    let mut segments = vec![];
    let mut init = None;
    let (mut ended, mut seq, mut key) = (false, 0u64, None::<Key>);
    let (mut duration, mut range, mut last_range_end) = (0.0f64, None::<(u64, u64)>, None::<u64>);

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let a = attrs(rest);
            let bw = get(&a, "AVERAGE-BANDWIDTH").or_else(|| get(&a, "BANDWIDTH")).and_then(|v| v.parse().ok()).unwrap_or(0);
            let h = get(&a, "RESOLUTION").and_then(|r| r.split_once('x')).and_then(|(_, h)| h.parse().ok());
            pending_variant = Some((bw, h, get(&a, "AUDIO").map(str::to_string)));
        } else if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA:") {
            let a = attrs(rest);
            if get(&a, "TYPE") == Some("AUDIO") {
                audio.push(AudioRendition {
                    group: get(&a, "GROUP-ID").unwrap_or("").to_string(),
                    url: get(&a, "URI").map(|u| join(base, u)).transpose()?,
                    default: get(&a, "DEFAULT") == Some("YES"),
                });
            }
        } else if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            seq = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("#EXT-X-KEY:") {
            let a = attrs(rest);
            key = match get(&a, "METHOD").unwrap_or("NONE") {
                "NONE" => None,
                "AES-128" => Some(Key {
                    url: join(base, get(&a, "URI").ok_or_else(|| bad("encrypted stream without a key link"))?)?,
                    iv: get(&a, "IV").and_then(parse_iv),
                }),
                other => return Err(bad(&format!("this stream is protected ({other}) and cannot be downloaded"))),
            };
        } else if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            let a = attrs(rest);
            let r = get(&a, "BYTERANGE").and_then(|r| parse_range(r, None));
            init = Some(Segment { url: join(base, get(&a, "URI").ok_or_else(|| bad("init segment without a link"))?)?, duration: 0.0, seq: 0, key: key.clone(), range: r });
        } else if let Some(rest) = line.strip_prefix("#EXTINF:") {
            duration = rest.split(',').next().and_then(|d| d.trim().parse().ok()).unwrap_or(0.0);
        } else if let Some(rest) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            range = parse_range(rest, last_range_end);
        } else if line == "#EXT-X-ENDLIST" {
            ended = true;
        } else if !line.starts_with('#') {
            if let Some((bw, h, ag)) = pending_variant.take() {
                variants.push(Variant { url: join(base, line)?, bandwidth: bw, height: h, audio_group: ag });
            } else {
                last_range_end = range.map(|(o, l)| o + l);
                segments.push(Segment { url: join(base, line)?, duration, seq, key: key.clone(), range: range.take() });
                seq += 1;
            }
        }
    }

    if !variants.is_empty() {
        return Ok(Playlist::Master { variants, audio });
    }
    if segments.is_empty() {
        return Err(bad("the playlist has no video pieces"));
    }
    let fmp4 = init.is_some() || segments[0].url.path().rsplit('.').next().is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "m4s" | "mp4" | "cmfv" | "cmfa" | "m4a" | "m4v"));
    Ok(Playlist::Media(Media { segments, init, ended, fmp4 }))
}

/// Highest bandwidth at or below `max_height`; when nothing fits the cap, the smallest one.
pub fn pick_variant(variants: &[Variant], max_height: Option<u32>) -> Option<&Variant> {
    let fits: Vec<&Variant> = variants.iter().filter(|v| max_height.is_none_or(|m| v.height.is_none_or(|h| h <= m))).collect();
    if fits.is_empty() {
        variants.iter().min_by_key(|v| (v.height.unwrap_or(u32::MAX), v.bandwidth))
    } else {
        fits.into_iter().max_by_key(|v| (v.bandwidth, v.height.unwrap_or(0)))
    }
}

/// The separate audio playlist for a variant, if its audio group has one (otherwise audio is inside the video pieces).
pub fn audio_for<'a>(v: &Variant, audio: &'a [AudioRendition]) -> Option<&'a AudioRendition> {
    let g = v.audio_group.as_deref()?;
    let mut group = audio.iter().filter(|a| a.group == g && a.url.is_some());
    let first = group.next()?;
    Some(if first.default { first } else { group.find(|a| a.default).unwrap_or(first) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://cdn.test/live/a/master.m3u8").unwrap()
    }

    const MASTER: &str = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English, main\",DEFAULT=YES,URI=\"audio/en.m3u8\"\n\
        #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,AUDIO=\"aud\"\nlow/index.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2800000,RESOLUTION=1280x720,AUDIO=\"aud\"\nhttps://other.test/hi/index.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,AUDIO=\"aud\"\nfull/index.m3u8\n";

    #[test]
    fn master_with_relative_and_absolute_links() {
        let Playlist::Master { variants, audio } = parse(MASTER, &base()).unwrap() else { panic!("master expected") };
        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0].url.as_str(), "https://cdn.test/live/a/low/index.m3u8");
        assert_eq!(variants[1].url.as_str(), "https://other.test/hi/index.m3u8");
        assert_eq!(variants[2].height, Some(1080));
        assert_eq!(audio[0].url.as_ref().unwrap().as_str(), "https://cdn.test/live/a/audio/en.m3u8");
        assert_eq!(audio_for(&variants[0], &audio).unwrap().group, "aud");
    }

    #[test]
    fn quality_choice() {
        let Playlist::Master { variants, .. } = parse(MASTER, &base()).unwrap() else { panic!() };
        assert_eq!(pick_variant(&variants, None).unwrap().height, Some(1080));
        assert_eq!(pick_variant(&variants, Some(720)).unwrap().height, Some(720));
        assert_eq!(pick_variant(&variants, Some(200)).unwrap().height, Some(360), "nothing fits: take the smallest");
    }

    #[test]
    fn attribute_values_may_contain_commas() {
        let a = attrs("TYPE=AUDIO,NAME=\"a, b\",URI=\"x.m3u8\"");
        assert_eq!(get(&a, "NAME"), Some("a, b"));
        assert_eq!(get(&a, "URI"), Some("x.m3u8"));
    }

    #[test]
    fn media_playlist_with_key_iv_and_sequence() {
        let t = "#EXTM3U\r\n#EXT-X-MEDIA-SEQUENCE:7\r\n#EXT-X-KEY:METHOD=AES-128,URI=\"k.bin\",IV=0x000102030405060708090a0b0c0d0e0f\r\n#EXTINF:4.5,\r\ns1.ts\r\n#EXT-X-KEY:METHOD=NONE\r\n#EXTINF:3,\r\ns2.ts\r\n#EXT-X-ENDLIST\r\n";
        let Playlist::Media(m) = parse(t, &base()).unwrap() else { panic!("media expected") };
        assert!(m.ended && !m.fmp4);
        assert_eq!((m.segments[0].seq, m.segments[1].seq), (7, 8));
        let k = m.segments[0].key.as_ref().unwrap();
        assert_eq!(k.url.as_str(), "https://cdn.test/live/a/k.bin");
        assert_eq!(k.iv.unwrap()[15], 0x0f);
        assert!(m.segments[1].key.is_none(), "METHOD=NONE switches encryption off");
        assert_eq!(m.segments[0].duration, 4.5);
    }

    #[test]
    fn missing_iv_stays_none_and_short_iv_is_padded() {
        let t = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:1,\na.ts\n#EXT-X-ENDLIST\n";
        let Playlist::Media(m) = parse(t, &base()).unwrap() else { panic!() };
        assert!(m.segments[0].key.as_ref().unwrap().iv.is_none());
        assert_eq!(parse_iv("0x1").unwrap()[15], 1);
        assert!(parse_iv("zz").is_none());
    }

    #[test]
    fn fmp4_with_init_segment_and_byteranges() {
        let t = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"720@0\"\n#EXT-X-BYTERANGE:1000@720\n#EXTINF:2,\nmedia.mp4\n#EXT-X-BYTERANGE:500\n#EXTINF:2,\nmedia.mp4\n#EXT-X-ENDLIST\n";
        let Playlist::Media(m) = parse(t, &base()).unwrap() else { panic!() };
        assert!(m.fmp4);
        assert_eq!(m.init.as_ref().unwrap().range, Some((0, 720)));
        assert_eq!(m.segments[0].range, Some((720, 1000)));
        assert_eq!(m.segments[1].range, Some((1720, 500)), "an offset-less range continues after the previous one");
    }

    #[test]
    fn live_streams_are_flagged_and_drm_is_refused() {
        let live = "#EXTM3U\n#EXTINF:4,\na.ts\n#EXTINF:4,\nb.ts\n";
        let Playlist::Media(m) = parse(live, &base()).unwrap() else { panic!() };
        assert!(!m.ended);
        let drm = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\n#EXTINF:4,\na.ts\n";
        assert!(matches!(parse(drm, &base()), Err(Error::Invalid(m)) if m.contains("SAMPLE-AES")));
    }

    #[test]
    fn rubbish_is_rejected() {
        assert!(parse("<html>nope</html>", &base()).is_err());
        assert!(parse("#EXTM3U\n#EXT-X-ENDLIST\n", &base()).is_err());
        assert!(parse("\u{feff}#EXTM3U\n#EXTINF:1,\na.ts\n#EXT-X-ENDLIST", &base()).is_ok(), "a byte-order mark is fine");
    }
}
