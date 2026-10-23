// Pure helpers shared by the service worker and the popup. No browser APIs here, so Node can test them.

export const isCapturableUrl = (url) => /^https?:\/\//i.test(url || '');

/** chrome.cookies.getAll() result -> Cookie request header value. */
export function cookieHeader(cookies) {
  return (cookies || []).filter((c) => c.name).map((c) => `${c.name}=${c.value}`).join('; ');
}

const MEDIA_EXT = /\.(mp4|m4v|webm|mkv|mov|avi|flv|mp3|m4a|aac|ogg|opus|flac|wav)$/i;
const SEGMENT_EXT = /\.(ts|m4s|cmfv|cmfa|aac\.ts)$/i;
const MIN_MEDIA_BYTES = 200 * 1024; // smaller responses are previews, ads or sound effects

/** Content-Range "bytes 0-99/1234" -> 1234 (total size), else null. */
export function rangeTotal(contentRange) {
  const m = /\/(\d+)\s*$/.exec(contentRange || '');
  return m ? Number(m[1]) : null;
}

/**
 * Decide whether a response is something the user would want to save.
 * `h` holds lower-case header values. Returns {kind, size} or null.
 */
export function classifyMedia(url, h) {
  let path = '', host = '';
  try {
    ({ pathname: path, hostname: host } = new URL(url));
  } catch {
    return null;
  }
  if (/(^|\.)googlevideo\.com$/.test(host) && path === '/videoplayback') return null; // one slice of a player stream, not a file
  if (SEGMENT_EXT.test(path)) return null; // pieces of a stream, not a file
  const type = (h['content-type'] || '').split(';')[0].trim().toLowerCase();
  const size = rangeTotal(h['content-range']) ?? (h['content-length'] ? Number(h['content-length']) : null);

  if (/mpegurl/.test(type) || /\.m3u8$/i.test(path)) return { kind: 'hls', size };
  if (type === 'application/dash+xml' || /\.mpd$/i.test(path)) return { kind: 'dash', size };

  const kind = type.startsWith('video/') ? 'video' : type.startsWith('audio/') ? 'audio' : MEDIA_EXT.test(path) && (!type || type === 'application/octet-stream') ? (/\.(mp3|m4a|aac|ogg|opus|flac|wav)$/i.test(path) ? 'audio' : 'video') : null;
  if (!kind) return null;
  if (size !== null && size < MIN_MEDIA_BYTES) return null;
  return { kind, size };
}

/** Same file served with different tokens in the query string counts once. */
export function mediaKey(url) {
  try {
    const u = new URL(url);
    return u.origin + u.pathname;
  } catch {
    return url;
  }
}

export function fileName(url) {
  try {
    const last = decodeURIComponent(new URL(url).pathname.split('/').filter(Boolean).pop() || '');
    return last || new URL(url).hostname;
  } catch {
    return url;
  }
}

export function formatSize(n) {
  if (n == null) return '';
  const u = ['B', 'KB', 'MB', 'GB'];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n >= 100 || i === 0 ? Math.round(n) : n.toFixed(1)} ${u[i]}`;
}

/** The manager treats [1-5] in a link as a range; keep real brackets literal. */
export const safeForBatch = (url) => url.replace(/\[/g, '%5B').replace(/\]/g, '%5D');

/** Filter text: "/regex/" or plain words (all must appear, case-insensitive). */
export function makeFilter(text) {
  const t = (text || '').trim();
  if (!t) return () => true;
  const re = /^\/(.+)\/([a-z]*)$/.exec(t);
  if (re) {
    try {
      const rx = new RegExp(re[1], re[2].includes('i') ? re[2] : re[2] + 'i');
      return (s) => rx.test(s);
    } catch {
      return () => false;
    }
  }
  const words = t.toLowerCase().split(/\s+/);
  return (s) => words.every((w) => s.toLowerCase().includes(w));
}

/** Run inside the page: collect links or images. Must stay self-contained (it is serialised). */
export function collectFromPage(mode) {
  const seen = new Set();
  const out = [];
  const add = (url, text) => {
    if (!/^https?:\/\//i.test(url) || seen.has(url)) return;
    seen.add(url);
    out.push({ url, text: (text || '').trim().slice(0, 120) });
  };
  if (mode === 'images') {
    document.querySelectorAll('img').forEach((i) => add(i.currentSrc || i.src, i.alt));
    document.querySelectorAll('a[href]').forEach((a) => /\.(jpe?g|png|gif|webp|bmp|svg)(\?|#|$)/i.test(a.href) && add(a.href, a.textContent));
  } else {
    document.querySelectorAll('a[href]').forEach((a) => add(a.href, a.textContent));
  }
  return out;
}

const YTDLP_HOSTS = ['youtube.com', 'youtu.be', 'youtube-nocookie.com', 'vimeo.com', 'dailymotion.com', 'facebook.com', 'fb.watch', 'instagram.com', 'twitter.com', 'x.com', 'tiktok.com', 'twitch.tv', 'reddit.com', 'soundcloud.com', 'bilibili.com'];

/** A page of a site whose videos can only be fetched with a helper tool (yt-dlp), and that page is a video page. */
export function isYtdlpPage(url) {
  let u;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  const host = u.hostname.replace(/^www\.|^m\./, '');
  if (!YTDLP_HOSTS.some((h) => host === h || host.endsWith('.' + h))) return false;
  if (host === 'youtu.be') return u.pathname.length > 1;
  if (host.endsWith('youtube.com') || host.endsWith('youtube-nocookie.com')) return /^\/(watch|shorts\/|live\/|embed\/)/.test(u.pathname) && (u.pathname !== '/watch' || u.searchParams.has('v'));
  return u.pathname.length > 1; // other sites: anything below the home page
}
