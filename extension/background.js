import { BRAND_NAME, HOST } from './brand.js';
import { classifyMedia, cookieHeader, fileName, isCapturableUrl, isYtdlpPage, mediaKey, safeForBatch } from './lib.js';

const VERSION = chrome.runtime.getManifest().version;
const MAX_MEDIA_PER_TAB = 50;

// ---- connection to the relay (one long-lived native port, replies matched by id) ----

let port = null;
const pending = new Map();
let nextId = 1;
let status = { ok: false, error: 'connecting' };

function connect() {
  port = chrome.runtime.connectNative(HOST);
  port.onMessage.addListener((m) => {
    const p = pending.get(m.id);
    if (p) {
      pending.delete(m.id);
      p(m);
    }
  });
  port.onDisconnect.addListener(() => {
    const detail = chrome.runtime.lastError?.message || '';
    port = null;
    pending.forEach((resolve) => resolve({ ok: false, error: 'host_disconnected', detail }));
    pending.clear();
  });
}

function call(msg, timeoutMs = 15000) {
  return new Promise((resolve) => {
    if (!port) connect();
    const id = nextId++;
    const timer = setTimeout(() => {
      pending.delete(id);
      resolve({ ok: false, error: 'timeout' });
    }, timeoutMs);
    pending.set(id, (m) => {
      clearTimeout(timer);
      resolve(m);
    });
    try {
      port.postMessage({ id, ...msg });
    } catch (e) {
      pending.delete(id);
      clearTimeout(timer);
      resolve({ ok: false, error: 'host_disconnected', detail: String(e) });
    }
  });
}

const api = (method, path, body) => call({ cmd: 'api', method, path, body });

async function refreshStatus() {
  const r = await call({ cmd: 'hello', version: VERSION }, 10000);
  status = r;
  await chrome.action.setBadgeBackgroundColor({ color: r.ok ? '#2563eb' : '#c0392b' });
  if (!r.ok) await chrome.action.setBadgeText({ text: '!' });
  else {
    await updateBadge();
    await getSettings();
  }
  return r;
}

/** The app's settings; its exclusion lists are kept locally so a new download can be judged without waiting on the app. */
async function getSettings() {
  const s = (await api('GET', '/api/settings')).body;
  if (s) await chrome.storage.local.set({ excl: s.exclusions || null });
  return s;
}

/** The app's own exclusion lists (sites and file endings the user wants the browser to keep). */
function isExcluded(ex, url) {
  try {
    const u = new URL(url);
    const ext = (u.pathname.split('.').pop() || '').toLowerCase();
    const host = u.hostname.toLowerCase();
    return (ex?.extensions || []).some((e) => e.toLowerCase() === ext) || (ex?.sites || []).some((h) => host === h.toLowerCase() || host.endsWith(`.${h.toLowerCase()}`));
  } catch {
    return false;
  }
}

const captureEnabled = async () => (await chrome.storage.local.get({ capture: true })).capture;

// ---- catching downloads ----

// Chrome waits for this listener before it names the file or shows its own Save As window,
// so a download taken over here never shows that window. True when the app took it.
async function takeOver(item) {
  const url = item.finalUrl || item.url;
  if (item.byExtensionId || !isCapturableUrl(url)) return false;
  const o = await chrome.storage.local.get({ capture: true, ask: true, excl: null });
  if (!o.capture) return false;
  const drop = async () => {
    await chrome.downloads.cancel(item.id).catch(() => {});
    await chrome.downloads.erase({ id: item.id }).catch(() => {});
    return true;
  };
  if (o.ask && !isExcluded(o.excl, url)) {
    // like IDM: show the small window at once; the app check runs while it opens
    const key = `pend:${Date.now()}`;
    const base = (item.filename || '').split(/[\\/]/).pop();
    await chrome.storage.session.set({ [key]: { url, referer: item.referrer || undefined, filename: base || fileName(url), size: item.totalBytes > 0 ? item.totalBytes : item.fileSize > 0 ? item.fileSize : null } });
    chrome.windows.create({ url: `confirm.html?k=${encodeURIComponent(key)}`, type: 'popup', width: 600, height: 296, focused: true });
    if (!status.ok && !(await refreshStatus()).ok) {
      // app unreachable: the browser keeps the download (the window closes itself when it gets no settings)
      await chrome.storage.session.remove(key);
      return false;
    }
    return drop();
  }
  if (!status.ok && !(await refreshStatus()).ok) return false; // app unreachable: the browser keeps the download
  const cookies = await chrome.cookies.getAll({ url }).catch(() => []);
  const r = await api('POST', '/api/capture', { url, referer: item.referrer || undefined, cookie: cookieHeader(cookies) || undefined, user_agent: navigator.userAgent });
  return r.ok && r.status === 200 && r.body?.captured ? drop() : false;
}

chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  takeOver(item)
    .catch(() => false)
    .then((taken) => taken || suggest());
  return true; // suggest() is called later
});

// ---- spotting media on pages ----

const mediaKeyFor = (tabId) => `media:${tabId}`;

async function recordMedia(tabId, entry) {
  const k = mediaKeyFor(tabId);
  const list = (await chrome.storage.session.get(k))[k] || [];
  const key = mediaKey(entry.url);
  if (list.some((e) => mediaKey(e.url) === key)) return;
  list.push(entry);
  await chrome.storage.session.set({ [k]: list.slice(-MAX_MEDIA_PER_TAB) });
  await updateBadge(tabId, list.length);
}

async function updateBadge(tabId, count) {
  if (!status.ok) return;
  if (tabId === undefined) {
    const [t] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!t) return;
    tabId = t.id;
    count = ((await chrome.storage.session.get(mediaKeyFor(tabId)))[mediaKeyFor(tabId)] || []).length;
  }
  await chrome.action.setBadgeText({ tabId, text: count ? String(count) : '' });
}

chrome.webRequest.onHeadersReceived.addListener(
  (d) => {
    if (d.tabId < 0 || d.statusCode >= 400) return;
    const h = {};
    (d.responseHeaders || []).forEach((x) => (h[x.name.toLowerCase()] = x.value || ''));
    const m = classifyMedia(d.url, h);
    if (m) recordMedia(d.tabId, { url: d.url, kind: m.kind, size: m.size, type: h['content-type'] || '', name: fileName(d.url), at: Date.now() });
  },
  { urls: ['<all_urls>'], types: ['media', 'xmlhttprequest', 'other'] },
  ['responseHeaders'],
);

chrome.tabs.onUpdated.addListener((tabId, change) => {
  if (change.status === 'loading' && change.url) {
    chrome.storage.session.remove(mediaKeyFor(tabId));
    chrome.action.setBadgeText({ tabId, text: '' });
  }
});
chrome.tabs.onRemoved.addListener((tabId) => chrome.storage.session.remove(mediaKeyFor(tabId)));

// ---- right-click menu ----

chrome.runtime.onInstalled.addListener(() => {
  const item = (id, title, contexts) => chrome.contextMenus.create({ id, title, contexts });
  item('dl-link', `Download link with ${BRAND_NAME}`, ['link']);
  item('dl-media', `Download with ${BRAND_NAME}`, ['video', 'audio', 'image']);
  item('grab-links', `Get all links on this page with ${BRAND_NAME}`, ['page']);
  item('grab-images', `Get all images on this page with ${BRAND_NAME}`, ['page']);
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === 'grab-links' || info.menuItemId === 'grab-images') {
    await chrome.storage.session.set({ grab: info.menuItemId === 'grab-links' ? 'links' : 'images' });
    chrome.action.openPopup().catch(() => chrome.action.setBadgeText({ tabId: tab?.id, text: '…' }));
    return;
  }
  const url = info.menuItemId === 'dl-link' ? info.linkUrl : info.srcUrl;
  if (url) await addUrls([url], info.pageUrl || tab?.url);
});

// ---- adding on request (explicit user actions skip the exclusion list) ----

async function addUrls(urls, referer, filename) {
  const first = urls.find(isCapturableUrl);
  const cookies = first ? await chrome.cookies.getAll({ url: first }).catch(() => []) : [];
  // shortcut: one cookie header for the whole batch (taken from the first link); fine for one site, per-link cookies if needed
  return api('POST', '/api/tasks', {
    urls: urls.filter(isCapturableUrl).map(safeForBatch).join('\n'),
    referer: referer || undefined,
    cookie: cookieHeader(cookies) || undefined,
    user_agent: navigator.userAgent,
    filename: urls.length === 1 ? filename || undefined : undefined, // a page title only makes sense for one link
  });
}

// ---- the helper tool for YouTube-style pages (yt-dlp), asked about at most every 30 s ----

let tools = { at: 0, ready: false };
async function toolsReady() {
  if (Date.now() - tools.at > 30000) {
    const r = await api('GET', '/api/tools');
    tools = { at: Date.now(), ready: !!(r.ok && r.status === 200 && r.body?.ytdlp?.ready) };
  }
  return tools.ready;
}

/** A whole page (YouTube, Vimeo...) handed to the helper tool instead of a direct file link. */
function addPage(msg) {
  return api('POST', '/api/tasks', { urls: safeForBatch(msg.url), ytdlp: true, max_height: msg.max_height, audio_only: msg.audio_only, filename: msg.filename, referer: msg.url });
}

// ---- messages from the popup and from the floating button ----

chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  (async () => {
    switch (msg.type) {
      case 'status':
        if (!status.ok) await refreshStatus();
        return reply({ status, version: VERSION, capture: await captureEnabled(), brand: BRAND_NAME });
      case 'set_capture':
        await chrome.storage.local.set({ capture: !!msg.on });
        return reply({ ok: true });
      case 'media': {
        const k = mediaKeyFor(msg.tabId);
        return reply({ media: (await chrome.storage.session.get(k))[k] || [] });
      }
      case 'media_here': {
        // asked by the floating button: what can be downloaded from the tab it sits in
        const k = mediaKeyFor(sender.tab?.id);
        const media = ((await chrome.storage.session.get(k))[k] || []).filter((m) => ['video', 'audio', 'hls', 'dash'].includes(m.kind));
        const page = msg.pageUrl || sender.tab?.url || '';
        return reply({ media, ytdlp: isYtdlpPage(page) ? { ready: await toolsReady() } : null });
      }
      case 'confirm_info': {
        const p = (await chrome.storage.session.get(msg.key))[msg.key];
        return reply({ pending: p || null, settings: await getSettings() });
      }
      case 'confirm_go': {
        const p = (await chrome.storage.session.get(msg.key))[msg.key];
        if (!p) return reply({ ok: false });
        await chrome.storage.session.remove(msg.key);
        const cookies = await chrome.cookies.getAll({ url: p.url }).catch(() => []);
        return reply(await api('POST', '/api/tasks', { urls: safeForBatch(p.url), referer: p.referer, cookie: cookieHeader(cookies) || undefined, user_agent: navigator.userAgent, filename: msg.filename || undefined, dir: msg.dir || undefined, paused: msg.later || undefined }));
      }
      case 'pick_path':
        // the Save As window waits for the user, so this call may take minutes
        return reply(await call({ cmd: 'pick_path', path: String(msg.path || '') }, 600000));
      case 'file_icon': {
        // Windows' own icon for a file type, asked once per type and kept
        const ext = String(msg.ext || '').toLowerCase();
        const { icons } = await chrome.storage.local.get({ icons: {} });
        if (!(ext in icons)) {
          const r = await call({ cmd: 'file_icon', ext });
          if (!r.ok) return reply({});
          icons[ext] = r.png ? `data:image/png;base64,${r.png}` : '';
          await chrome.storage.local.set({ icons });
        }
        return reply({ src: icons[ext] });
      }
      case 'task_get':
        return reply(await api('GET', `/api/tasks/${Number(msg.id)}`));
      case 'task_act':
        return reply(await api('POST', `/api/tasks/${Number(msg.id)}/${msg.act === 'pause' ? 'pause' : 'resume'}`, {}));
      case 'set_ask':
        await chrome.storage.local.set({ ask: !!msg.on });
        return reply({ ok: true });
      case 'add_page':
        return reply(await addPage(msg));
      case 'add':
        return reply(await addUrls(msg.urls, msg.referer, msg.filename));
      case 'open_ui':
        return reply(await call({ cmd: 'open_ui', view: msg.view }));
      default:
        return reply({ ok: false, error: 'unknown_message' });
    }
  })();
  return true; // reply asynchronously
});

refreshStatus();
