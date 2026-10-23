// Floating "Download" button on top of videos (like IDM's). One small shadow-DOM overlay per frame; the page cannot
// style or read it, and it does nothing until the user clicks. The brand name and colour are filled in at packaging time.
(() => {
  if (window.__dmFloat || window.top !== window && !document.body) return;
  window.__dmFloat = true;
  const NAME = '__BRAND_NAME__'.startsWith('__') ? 'Downloader' : '__BRAND_NAME__';
  const COLOR = '__BRAND_COLOR__'.startsWith('__') ? '#2563eb' : '__BRAND_COLOR__';
  const MIN_W = 200, MIN_H = 112;

  let enabled = true;
  try { chrome.storage.local.get({ floating: true }, (v) => { enabled = v.floating; refresh(); }); } catch { /* extension reloaded */ }
  try { chrome.storage.onChanged.addListener((c) => { if (c.floating) { enabled = c.floating.newValue !== false; refresh(); } }); } catch { /* ignore */ }

  const host = document.createElement('div');
  host.style.cssText = 'all:initial;position:fixed;top:0;left:0;width:0;height:0;z-index:2147483647';
  const root = host.attachShadow({ mode: 'closed' });
  root.innerHTML = `<style>
    :host{all:initial}
    *{box-sizing:border-box;font:13px/1.35 "Segoe UI",system-ui,sans-serif}
    .pill{position:fixed;display:none;align-items:center;gap:6px;padding:6px 10px 6px 8px;border:0;border-radius:8px;background:${COLOR};color:#fff;cursor:pointer;box-shadow:0 2px 10px #0006;font-weight:600;opacity:.95}
    .pill:hover{opacity:1;filter:brightness(1.1)} .pill:focus-visible,.item button:focus-visible,.close:focus-visible{outline:2px solid #fff;outline-offset:2px}
    .pill svg{width:16px;height:16px;flex:none} .count{background:#fff;color:${COLOR};border-radius:9px;padding:0 6px;font-size:11px;min-width:18px;text-align:center}
    .panel{position:fixed;display:none;width:min(340px,calc(100vw - 20px));max-height:min(60vh,380px);overflow:auto;padding:10px;background:#16181d;color:#f2f4f8;border:1px solid #3a3f4b;border-radius:10px;box-shadow:0 8px 30px #000a}
    .head{display:flex;align-items:center;justify-content:space-between;margin-bottom:6px;font-weight:700}
    .close{border:0;background:none;color:inherit;font-size:18px;cursor:pointer;padding:0 4px;border-radius:4px}
    .item{display:flex;align-items:center;gap:8px;padding:6px 0;border-top:1px solid #2b2f38}
    .kind{flex:none;font-size:11px;text-transform:uppercase;background:#2b2f38;border-radius:4px;padding:1px 6px}
    .name{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap} .size{flex:none;opacity:.7;font-size:12px}
    .item button,.act{flex:none;border:0;border-radius:6px;background:${COLOR};color:#fff;padding:5px 10px;font-weight:600;cursor:pointer}
    .act.alt{background:#2b2f38} .note{margin:6px 0 0;opacity:.8;font-size:12px} .ok{color:#7ee2a8} .bad{color:#ff9a9a}
    .row{display:flex;gap:6px;flex-wrap:wrap;margin-top:6px}
    @media (prefers-reduced-motion:no-preference){.pill{transition:opacity .15s}}
  </style>
  <button class="pill" type="button" aria-haspopup="dialog"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4v11M7 11l5 5 5-5M5 20h14"/></svg><span class="label"></span><span class="count" hidden></span></button>
  <div class="panel" role="dialog" aria-label="${NAME}"></div>`;
  const pill = root.querySelector('.pill'), label = root.querySelector('.label'), count = root.querySelector('.count'), panel = root.querySelector('.panel');
  label.textContent = 'Download';

  let target = null;        // the video the pill is attached to
  let hovering = false, lastActive = 0, open = false, info = { media: [], ytdlp: null }, infoAt = 0;
  const videos = new Set();

  const visibleRect = (v) => {
    const r = v.getBoundingClientRect();
    const w = Math.min(r.right, innerWidth) - Math.max(r.left, 0), h = Math.min(r.bottom, innerHeight) - Math.max(r.top, 0);
    return r.width >= MIN_W && r.height >= MIN_H && w > MIN_W / 2 && h > MIN_H / 2 && getComputedStyle(v).visibility !== 'hidden' ? r : null;
  };
  const playing = (v) => !v.paused && !v.ended && v.readyState > 2;

  function pick() {
    let best = null, bestArea = 0;
    for (const v of videos) {
      if (!v.isConnected) { videos.delete(v); continue; }
      const r = visibleRect(v);
      if (r && (playing(v) || v === target || (hover.x >= r.left && hover.x <= r.right && hover.y >= r.top && hover.y <= r.bottom))) {
        const area = r.width * r.height;
        if (area > bestArea) { best = v; bestArea = area; }
      }
    }
    return best;
  }
  const hover = { x: -1, y: -1 };

  function place() {
    if (!enabled || !document.documentElement.contains(host) && !attach()) return;
    const v = pick();
    if (v) target = v;
    const r = target && visibleRect(target);
    const overVideo = r && hover.x >= r.left && hover.x <= r.right && hover.y >= r.top && hover.y <= r.bottom;
    const show = r && (overVideo || hovering || open || Date.now() - lastActive < 3000);
    if (!r || !show) { pill.style.display = 'none'; if (!open) panel.style.display = 'none'; if (!r) target = null; return; }
    pill.style.display = 'inline-flex';
    const pw = pill.offsetWidth, x = Math.max(4, Math.min(r.right - pw - 10, innerWidth - pw - 4)), y = Math.max(4, Math.min(r.top + 10, innerHeight - 40));
    pill.style.left = x + 'px'; pill.style.top = y + 'px';
    if (open) { panel.style.display = 'block'; panel.style.left = Math.max(10, Math.min(x + pw - panel.offsetWidth, innerWidth - panel.offsetWidth - 10)) + 'px'; panel.style.top = Math.min(y + 38, innerHeight - panel.offsetHeight - 10) + 'px'; }
  }

  function attach() {
    const parent = document.fullscreenElement || document.documentElement;
    if (host.parentNode !== parent) parent.appendChild(host);
    return host.isConnected;
  }

  const ask = (msg) => new Promise((res) => { try { chrome.runtime.sendMessage(msg, (r) => res(chrome.runtime.lastError ? null : r)); } catch { res(null); } });
  async function loadInfo(force) {
    if (!force && Date.now() - infoAt < 1500) return;
    infoAt = Date.now();
    const r = await ask({ type: 'media_here', pageUrl: location.href });
    if (r) info = r;
    const n = info.media.length;
    count.hidden = !n; count.textContent = n;
    label.textContent = n || !info.ytdlp ? 'Download' : 'Download video';
  }

  const fmt = (n) => (n == null ? '' : n >= 1 << 30 ? (n / (1 << 30)).toFixed(1) + ' GB' : n >= 1 << 20 ? Math.round(n / (1 << 20)) + ' MB' : Math.round(n / 1024) + ' KB');
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };

  async function send(msg, note) {
    note.className = 'note'; note.textContent = '…';
    const r = await ask(msg);
    const ok = r && (r.ok !== false) && (r.status == null || r.status < 300) && !(r.body && r.body.invalid && r.body.invalid.length && !r.body.added.length && !r.body.duplicates.length);
    note.className = 'note ' + (ok ? 'ok' : 'bad');
    note.textContent = ok ? `Added to ${NAME}` : (r && r.body && r.body.error) || (r && r.error === 'app_not_running' ? `${NAME} is not running` : 'Could not add the download');
  }

  async function render() {
    await loadInfo(true);
    panel.textContent = '';
    const head = el('div', 'head'); head.append(el('span', '', NAME));
    const close = el('button', 'close', '×'); close.setAttribute('aria-label', 'Close'); close.onclick = () => { open = false; place(); };
    head.append(close); panel.append(head);
    const note = el('p', 'note');
    for (const m of info.media) {
      const row = el('div', 'item'); row.append(el('span', 'kind', m.kind));
      const name = el('span', 'name', m.name); name.title = m.url; row.append(name, el('span', 'size', fmt(m.size)));
      const b = el('button', '', 'Download');
      b.onclick = () => send({ type: 'add', urls: [m.url], referer: location.href, filename: document.title }, note);
      row.append(b); panel.append(row);
    }
    if (info.ytdlp) {
      const box = el('div', 'item'); box.style.flexDirection = 'column'; box.style.alignItems = 'stretch';
      if (info.ytdlp.ready) {
        box.append(el('span', 'name', 'Whole video from this page'));
        const row = el('div', 'row');
        for (const [text, opts] of [['Best', {}], ['1080p', { max_height: 1080 }], ['720p', { max_height: 720 }], ['480p', { max_height: 480 }], ['Audio only', { audio_only: true }]]) {
          const b = el('button', 'act' + (text === 'Best' ? '' : ' alt'), text);
          b.onclick = () => send({ type: 'add_page', url: location.href, filename: document.title, ...opts }, note);
          row.append(b);
        }
        box.append(row);
      } else {
        box.append(el('span', 'name', 'This site needs a helper tool (yt-dlp).'));
        const b = el('button', 'act', `Set it up in ${NAME}`); b.onclick = () => ask({ type: 'open_ui', view: 'streams' });
        const row = el('div', 'row'); row.append(b); box.append(row);
      }
      panel.append(box);
    } else if (!info.media.length) {
      panel.append(el('p', 'note', 'No downloadable file found for this video yet. Play it for a few seconds and open this again.'));
    }
    panel.append(note);
    place();
  }

  pill.addEventListener('click', (e) => { e.stopPropagation(); open = !open; lastActive = Date.now(); if (open) render(); else place(); });
  pill.addEventListener('mouseenter', () => { hovering = true; });
  pill.addEventListener('mouseleave', () => { hovering = false; });
  panel.addEventListener('mouseenter', () => { hovering = true; });
  panel.addEventListener('mouseleave', () => { hovering = false; });
  addEventListener('keydown', (e) => { if (e.key === 'Escape' && open) { open = false; place(); } }, true);
  addEventListener('click', (e) => { if (open && !e.composedPath().includes(host)) { open = false; place(); } }, true);
  addEventListener('pointermove', (e) => { hover.x = e.clientX; hover.y = e.clientY; }, { passive: true, capture: true });
  addEventListener('scroll', () => place(), { passive: true, capture: true });
  addEventListener('resize', () => place(), { passive: true });
  document.addEventListener('fullscreenchange', () => { attach(); place(); });
  document.addEventListener('play', (e) => { if (e.target instanceof HTMLVideoElement) { videos.add(e.target); lastActive = Date.now(); loadInfo(false); place(); } }, true);

  function scan() { document.querySelectorAll('video').forEach((v) => videos.add(v)); }
  function refresh() { scan(); if (!enabled) { pill.style.display = 'none'; panel.style.display = 'none'; open = false; } else place(); if (enabled && target) loadInfo(false); }
  new MutationObserver(() => scan()).observe(document.documentElement, { childList: true, subtree: true });
  setInterval(refresh, 500);
  scan();
})();
