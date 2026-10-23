//! MPEG-DASH (.mpd) manifests: SegmentTemplate (number or timeline), SegmentList and single-file BaseURL tracks.
use crate::error::{Error, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use reqwest::Url;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Video,
    Audio,
}

/// One downloadable track (a Representation).
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub kind: Kind,
    pub id: String,
    pub bandwidth: u64,
    pub height: Option<u32>,
    pub init: Option<Url>,
    /// Media pieces in order. For a single-file track this holds one entry: the file itself.
    pub segments: Vec<Url>,
    pub duration: Option<f64>,
}

#[derive(Debug, PartialEq)]
pub struct Mpd {
    pub video: Vec<Track>,
    pub audio: Vec<Track>,
}

#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: HashMap<String, String>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn a(&self, k: &str) -> Option<&str> {
        self.attrs.get(k).map(String::as_str)
    }
    fn child(&self, n: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == n)
    }
    fn all<'a>(&'a self, n: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == n)
    }
}

fn bad(m: &str) -> Error {
    Error::Invalid(m.into())
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn node_of(e: &BytesStart) -> Node {
    Node {
        name: e.local_name().as_ref().to_string(),
        attrs: e.attributes().flatten().map(|a| (a.key.local_name().as_ref().to_string(), unescape(&a.value))).collect(),
        ..Node::default()
    }
}

fn tree(xml: &str) -> Result<Node> {
    let mut r = Reader::from_str(xml);
    let mut stack: Vec<Node> = vec![];
    let mut root = None;
    loop {
        match r.read_event().map_err(|_| bad("the manifest is not valid XML"))? {
            Event::Start(e) => stack.push(node_of(&e)),
            Event::Empty(e) => {
                let n = node_of(&e);
                match stack.last_mut() {
                    Some(p) => p.children.push(n),
                    None => root = Some(n),
                }
            }
            Event::Text(t) => {
                if let Some(p) = stack.last_mut() {
                    p.text.push_str(&unescape(t.as_ref()));
                }
            }
            Event::End(_) => {
                let n = stack.pop().ok_or_else(|| bad("the manifest is not valid XML"))?;
                match stack.last_mut() {
                    Some(p) => p.children.push(n),
                    None => root = Some(n),
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    root.ok_or_else(|| bad("the manifest is empty"))
}

/// ISO-8601 duration like PT1H2M3.5S -> seconds.
pub fn iso_duration(s: &str) -> Option<f64> {
    let rest = s.strip_prefix('P')?;
    let (date, time) = rest.split_once('T').unwrap_or((rest, ""));
    let mut total = 0.0;
    let mut num = String::new();
    for c in date.chars() {
        match c {
            '0'..='9' | '.' => num.push(c),
            'D' => total += num.parse::<f64>().ok()? * 86400.0,
            'Y' | 'M' | 'W' => return None, // calendar units: not meaningful for a video length
            _ => return None,
        }
        if c == 'D' {
            num.clear();
        }
    }
    num.clear();
    for c in time.chars() {
        match c {
            '0'..='9' | '.' => num.push(c),
            'H' => {
                total += num.parse::<f64>().ok()? * 3600.0;
                num.clear();
            }
            'M' => {
                total += num.parse::<f64>().ok()? * 60.0;
                num.clear();
            }
            'S' => {
                total += num.parse::<f64>().ok()?;
                num.clear();
            }
            _ => return None,
        }
    }
    Some(total)
}

/// Fill $RepresentationID$, $Bandwidth$, $Number$, $Time$ (with optional %0Nd) in a template.
pub fn fill(t: &str, id: &str, bandwidth: u64, number: u64, time: u64) -> String {
    let mut out = String::new();
    let parts: Vec<&str> = t.split('$').collect();
    for (i, p) in parts.iter().enumerate() {
        if i % 2 == 0 {
            out.push_str(p);
            continue;
        }
        if p.is_empty() {
            out.push('$'); // "$$" is an escaped dollar sign
            continue;
        }
        let (name, fmt) = p.split_once('%').map_or((*p, None), |(n, f)| (n, Some(f)));
        let width = fmt.and_then(|f| f.strip_prefix('0')).and_then(|f| f.strip_suffix('d')).and_then(|w| w.parse::<usize>().ok()).unwrap_or(0);
        match name {
            "RepresentationID" => out.push_str(id),
            "Bandwidth" => out.push_str(&format!("{bandwidth:0width$}")),
            "Number" => out.push_str(&format!("{number:0width$}")),
            "Time" => out.push_str(&format!("{time:0width$}")),
            _ => {
                out.push('$');
                out.push_str(p);
                out.push('$');
            }
        }
    }
    out
}

fn join(base: &Url, rel: &str) -> Result<Url> {
    base.join(rel.trim()).map_err(|_| bad("the manifest contains a broken link"))
}

fn base_of(n: &Node, parent: &Url) -> Result<Url> {
    match n.child("BaseURL") {
        Some(b) if !b.text.trim().is_empty() => join(parent, &b.text),
        _ => Ok(parent.clone()),
    }
}

fn segments_of(rep: &Node, set: &Node, base: &Url, id: &str, bw: u64, period_secs: Option<f64>) -> Result<(Option<Url>, Vec<Url>, Option<f64>)> {
    // SegmentTemplate may sit on the Representation or be inherited from the AdaptationSet
    if let Some(t) = rep.child("SegmentTemplate").or_else(|| set.child("SegmentTemplate")) {
        let timescale: f64 = t.a("timescale").and_then(|v| v.parse().ok()).unwrap_or(1.0);
        let init = t.a("initialization").map(|i| join(base, &fill(i, id, bw, 0, 0))).transpose()?;
        let media = t.a("media").ok_or_else(|| bad("the manifest has no media template"))?;
        let start: u64 = t.a("startNumber").and_then(|v| v.parse().ok()).unwrap_or(1);
        let mut urls = vec![];
        let mut total = 0.0;
        if let Some(tl) = t.child("SegmentTimeline") {
            let (mut time, mut number) = (0u64, start);
            for s in tl.all("S") {
                if let Some(tt) = s.a("t").and_then(|v| v.parse().ok()) {
                    time = tt;
                }
                let d: u64 = s.a("d").and_then(|v| v.parse().ok()).ok_or_else(|| bad("a timeline entry has no duration"))?;
                let r: i64 = s.a("r").and_then(|v| v.parse().ok()).unwrap_or(0);
                if r < 0 {
                    return Err(bad("this stream never ends (live) and cannot be downloaded yet"));
                }
                for _ in 0..=r {
                    urls.push(join(base, &fill(media, id, bw, number, time))?);
                    total += d as f64 / timescale;
                    time += d;
                    number += 1;
                }
            }
        } else {
            let d: f64 = t.a("duration").and_then(|v| v.parse().ok()).ok_or_else(|| bad("the manifest has no segment duration"))?;
            let secs = period_secs.ok_or_else(|| bad("the manifest has no total length"))?;
            let count = (secs * timescale / d).ceil() as u64;
            if count == 0 || count > 200_000 {
                return Err(bad("the manifest describes an impossible number of pieces"));
            }
            for n in start..start + count {
                urls.push(join(base, &fill(media, id, bw, n, (n - start) * d as u64))?);
            }
            total = secs;
        }
        return Ok((init, urls, Some(total)));
    }
    if let Some(l) = rep.child("SegmentList").or_else(|| set.child("SegmentList")) {
        let init = l.child("Initialization").and_then(|i| i.a("sourceURL")).map(|u| join(base, u)).transpose()?;
        let urls = l.all("SegmentURL").filter_map(|u| u.a("media")).map(|m| join(base, m)).collect::<Result<Vec<_>>>()?;
        return Ok((init, urls, period_secs));
    }
    // one file holding the whole track
    if rep.child("BaseURL").is_some() {
        return Ok((None, vec![base.clone()], period_secs));
    }
    Err(bad("the manifest does not say where the video pieces are"))
}

pub fn parse(xml: &str, manifest_url: &Url) -> Result<Mpd> {
    let root = tree(xml.trim_start_matches('\u{feff}'))?;
    if root.name != "MPD" {
        return Err(bad("this is not a DASH manifest"));
    }
    if root.a("type") == Some("dynamic") {
        return Err(bad("this is a live stream and cannot be downloaded yet"));
    }
    let periods: Vec<&Node> = root.all("Period").collect();
    let period = match periods.as_slice() {
        [p] => *p,
        [] => return Err(bad("the manifest has no content")),
        _ => return Err(bad("streams made of several parts (for example with ads) are not supported yet")),
    };
    let secs = period.a("duration").or_else(|| root.a("mediaPresentationDuration")).and_then(iso_duration);
    let root_base = base_of(&root, manifest_url)?;
    let period_base = base_of(period, &root_base)?;

    let (mut video, mut audio) = (vec![], vec![]);
    for set in period.all("AdaptationSet") {
        if set.child("ContentProtection").is_some() || set.all("Representation").any(|r| r.child("ContentProtection").is_some()) {
            return Err(bad("this stream is protected (DRM) and cannot be downloaded"));
        }
        let set_base = base_of(set, &period_base)?;
        for rep in set.all("Representation") {
            let mime = rep.a("mimeType").or_else(|| set.a("mimeType")).or_else(|| set.a("contentType")).unwrap_or("");
            let kind = if mime.starts_with("video") {
                Kind::Video
            } else if mime.starts_with("audio") {
                Kind::Audio
            } else {
                continue; // subtitles, thumbnails
            };
            let id = rep.a("id").unwrap_or("0");
            let bandwidth: u64 = rep.a("bandwidth").and_then(|v| v.parse().ok()).unwrap_or(0);
            let base = base_of(rep, &set_base)?;
            let (init, segments, duration) = segments_of(rep, set, &base, id, bandwidth, secs)?;
            let t = Track { kind, id: id.to_string(), bandwidth, height: rep.a("height").and_then(|v| v.parse().ok()), init, segments, duration };
            if kind == Kind::Video { video.push(t) } else { audio.push(t) }
        }
    }
    if video.is_empty() && audio.is_empty() {
        return Err(bad("the manifest has no video or audio tracks"));
    }
    Ok(Mpd { video, audio })
}

/// Best video at or below `max_height` (the smallest if nothing fits), and the best audio.
pub fn pick(m: &Mpd, max_height: Option<u32>) -> (Option<&Track>, Option<&Track>) {
    let fits: Vec<&Track> = m.video.iter().filter(|t| max_height.is_none_or(|h| t.height.is_none_or(|x| x <= h))).collect();
    let video = if fits.is_empty() { m.video.iter().min_by_key(|t| (t.height.unwrap_or(u32::MAX), t.bandwidth)) } else { fits.into_iter().max_by_key(|t| t.bandwidth) };
    (video, m.audio.iter().max_by_key(|t| t.bandwidth))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://cdn.test/vod/movie/manifest.mpd").unwrap()
    }

    const TIMELINE: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT20S">
 <BaseURL>media/</BaseURL>
 <Period>
  <AdaptationSet mimeType="video/mp4">
   <SegmentTemplate timescale="1000" initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/seg-$Number%03d$-$Time$.m4s" startNumber="1">
     <SegmentTimeline><S t="0" d="4000" r="2"/><S d="8000"/></SegmentTimeline>
   </SegmentTemplate>
   <Representation id="v360" bandwidth="500000" height="360"/>
   <Representation id="v720" bandwidth="2500000" height="720"/>
  </AdaptationSet>
  <AdaptationSet mimeType="audio/mp4" lang="en">
   <Representation id="a1" bandwidth="128000"><SegmentTemplate timescale="1" duration="10" initialization="a/init.mp4" media="a/$Number$.m4s"/></Representation>
  </AdaptationSet>
  <AdaptationSet mimeType="text/vtt"><Representation id="s" bandwidth="1"><BaseURL>subs.vtt</BaseURL></Representation></AdaptationSet>
 </Period>
</MPD>"#;

    #[test]
    fn timeline_and_number_templates() {
        let m = parse(TIMELINE, &base()).unwrap();
        assert_eq!(m.video.len(), 2);
        let v = &m.video[1];
        assert_eq!(v.init.as_ref().unwrap().as_str(), "https://cdn.test/vod/movie/media/v720/init.mp4");
        assert_eq!(v.segments.len(), 4, "3 repeats of the first entry plus the long one");
        assert_eq!(v.segments[0].as_str(), "https://cdn.test/vod/movie/media/v720/seg-001-0.m4s");
        assert_eq!(v.segments[3].as_str(), "https://cdn.test/vod/movie/media/v720/seg-004-12000.m4s");
        let a = &m.audio[0];
        assert_eq!(a.segments.len(), 2, "20 seconds in 10 second pieces");
        assert_eq!(a.segments[1].as_str(), "https://cdn.test/vod/movie/media/a/2.m4s");
        assert!(m.video.iter().chain(&m.audio).all(|t| t.id != "s"), "subtitles are skipped");
    }

    #[test]
    fn picking_quality() {
        let m = parse(TIMELINE, &base()).unwrap();
        let (v, a) = pick(&m, None);
        assert_eq!((v.unwrap().id.as_str(), a.unwrap().id.as_str()), ("v720", "a1"));
        assert_eq!(pick(&m, Some(480)).0.unwrap().id, "v360");
        assert_eq!(pick(&m, Some(100)).0.unwrap().id, "v360");
    }

    #[test]
    fn single_file_tracks_and_segment_lists() {
        let x = r#"<MPD type="static" mediaPresentationDuration="PT1M"><Period>
          <AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="1" height="480"><BaseURL>video.mp4</BaseURL><SegmentBase indexRange="0-100"/></Representation></AdaptationSet>
          <AdaptationSet mimeType="audio/mp4"><Representation id="a" bandwidth="1"><SegmentList><Initialization sourceURL="i.mp4"/><SegmentURL media="s1.m4s"/><SegmentURL media="s2.m4s"/></SegmentList></Representation></AdaptationSet>
        </Period></MPD>"#;
        let m = parse(x, &base()).unwrap();
        assert_eq!(m.video[0].segments[0].as_str(), "https://cdn.test/vod/movie/video.mp4");
        assert_eq!(m.audio[0].segments.len(), 2);
        assert_eq!(m.audio[0].init.as_ref().unwrap().as_str(), "https://cdn.test/vod/movie/i.mp4");
    }

    #[test]
    fn live_drm_and_multi_period_are_refused() {
        assert!(matches!(parse(r#"<MPD type="dynamic"><Period/></MPD>"#, &base()), Err(Error::Invalid(m)) if m.contains("live")));
        let drm = r#"<MPD type="static"><Period><AdaptationSet mimeType="video/mp4"><ContentProtection schemeIdUri="urn:uuid:edef8ba9"/><Representation id="v" bandwidth="1"><BaseURL>v.mp4</BaseURL></Representation></AdaptationSet></Period></MPD>"#;
        assert!(matches!(parse(drm, &base()), Err(Error::Invalid(m)) if m.contains("DRM")));
        assert!(matches!(parse(r#"<MPD type="static"><Period/><Period/></MPD>"#, &base()), Err(Error::Invalid(m)) if m.contains("several")));
        assert!(parse("not xml at all", &base()).is_err());
        assert!(parse("<html/>", &base()).is_err());
    }

    #[test]
    fn durations_and_templates() {
        assert_eq!(iso_duration("PT1H2M3.5S"), Some(3723.5));
        assert_eq!(iso_duration("PT20S"), Some(20.0));
        assert_eq!(iso_duration("P1DT1H"), Some(90000.0));
        assert_eq!(iso_duration("garbage"), None);
        assert_eq!(fill("$RepresentationID$/$Number%05d$-$Time$-$Bandwidth$-$$.m4s", "v1", 9, 42, 7), "v1/00042-7-9-$.m4s");
        assert_eq!(fill("plain", "x", 0, 0, 0), "plain");
    }

    #[test]
    fn timeline_with_open_repeat_means_live() {
        let x = r#"<MPD type="static"><Period><AdaptationSet mimeType="video/mp4"><SegmentTemplate media="$Number$.m4s" timescale="1"><SegmentTimeline><S t="0" d="2" r="-1"/></SegmentTimeline></SegmentTemplate><Representation id="v" bandwidth="1"/></AdaptationSet></Period></MPD>"#;
        assert!(matches!(parse(x, &base()), Err(Error::Invalid(m)) if m.contains("live")));
    }
}
