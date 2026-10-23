import { BRAND_NAME, HOST } from './brand.js';
import { classifyMedia, cookieHeader, fileName, isCapturableUrl, mediaKey, safeForBatch } from './lib.js';

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
  else await updateBadge();
  return r;
}

const captureEnabled = async () => (await chrome.storage.local.get({ capture: true })).capture;

// ---- catching downloads ----

chrome.downloads.onCreated.addListener(async (item) => {
  const url = item.finalUrl || item.url;
  if (item.byExtensionId || !isCapturableUrl(url) || !(await captureEnabled())) return;
  if (!status.ok && !(await refreshStatus()).ok) return; // app unreachable: the browser keeps the download
  const cookies = await chrome.cookies.getAll({ url }).catch(() => []);
  const r = await api('POST', '/api/capture', { url, referer: item.referrer || undefined, cookie: cookieHeader(cookies) || undefined, user_agent: navigator.userAgent });
  if (r.ok && r.status === 200 && r.body?.captured) {
    await chrome.downloads.cancel(item.id).catch(() => {});
    await chrome.downloads.erase({ id: item.id }).catch(() => {});
  }
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

async function addUrls(urls, referer) {
  const first = urls.find(isCapturableUrl);
  const cookies = first ? await chrome.cookies.getAll({ url: first }).catch(() => []) : [];
  // shortcut: one cookie header for the whole batch (taken from the first link); fine for one site, per-link cookies if needed
  return api('POST', '/api/tasks', {
    urls: urls.filter(isCapturableUrl).map(safeForBatch).join('\n'),
    referer: referer || undefined,
    cookie: cookieHeader(cookies) || undefined,
    user_agent: navigator.userAgent,
  });
}

// ---- messages from the popup ----

chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
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
      case 'add':
        return reply(await addUrls(msg.urls, msg.referer));
      case 'open_ui':
        return reply(await call({ cmd: 'open_ui' }));
      default:
        return reply({ ok: false, error: 'unknown_message' });
    }
  })();
  return true; // reply asynchronously
});

refreshStatus();
