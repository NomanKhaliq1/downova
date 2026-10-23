import { collectFromPage, fileName, formatSize, makeFilter } from './lib.js';

const $ = (id) => document.getElementById(id);
const ask = (msg) => chrome.runtime.sendMessage(msg);
let tab = null;
let found = [];

function el(tag, props = {}, ...kids) {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...kids);
  return e;
}

async function showStatus() {
  const s = await ask({ type: 'status' });
  $('brand').textContent = s.brand;
  $('capture').checked = s.capture;
  const st = s.status;
  const bad = !st.ok;
  $('status').className = bad ? 'bad' : 'muted';
  $('status').textContent = bad
    ? st.error === 'version_mismatch' ? `Update needed: extension ${st.extension_version}, app ${st.host_version}`
    : st.error === 'app_not_running' ? 'The app is not running'
    : /not found/i.test(st.detail || '') ? 'Browser integration is not installed'
    : `Not connected (${st.error})`
    : `Connected · v${s.version}`;
}

async function showMedia() {
  const { media } = await ask({ type: 'media', tabId: tab.id });
  if (!media.length) return;
  $('media').className = '';
  $('media').replaceChildren(
    ...media.map((m) =>
      el('div', { className: 'row' }, el('span', { className: 'muted', textContent: m.kind }), el('span', { className: 'name', title: m.url, textContent: m.name }), el('span', { className: 'muted', textContent: formatSize(m.size) }),
        el('button', { textContent: 'Download', onclick: async (e) => { e.target.disabled = true; await ask({ type: 'add', urls: [m.url], referer: tab.url }); e.target.textContent = 'Added'; } })),
    ),
  );
}

function renderGrab() {
  const ok = makeFilter($('filter').value);
  const rows = found.map((f, i) => ({ f, i })).filter(({ f }) => ok(`${f.url} ${f.text}`));
  $('list').replaceChildren(
    ...rows.map(({ f, i }) => el('label', { className: 'row' }, el('input', { type: 'checkbox', dataset: { i }, checked: false, onchange: count }), el('span', { className: 'name', title: f.url, textContent: f.text || fileName(f.url) }))),
  );
  count();
}

const boxes = () => [...$('list').querySelectorAll('input')];
function count() {
  const n = boxes().filter((b) => b.checked).length;
  $('count').textContent = `${n} of ${boxes().length}`;
  $('go').disabled = !n;
}

async function grab(mode) {
  $('grab').hidden = false;
  const [res] = await chrome.scripting.executeScript({ target: { tabId: tab.id }, func: collectFromPage, args: [mode] });
  found = res?.result || [];
  renderGrab();
}

$('capture').onchange = (e) => ask({ type: 'set_capture', on: e.target.checked });
$('links').onclick = () => grab('links');
$('images').onclick = () => grab('images');
$('filter').oninput = renderGrab;
$('all').onclick = () => { boxes().forEach((b) => (b.checked = true)); count(); };
$('none').onclick = () => { boxes().forEach((b) => (b.checked = false)); count(); };
$('openUi').onclick = () => ask({ type: 'open_ui' });
$('go').onclick = async () => {
  const urls = boxes().filter((b) => b.checked).map((b) => found[b.dataset.i].url);
  $('go').disabled = true;
  await ask({ type: 'add', urls, referer: tab.url });
  $('count').textContent = `${urls.length} added`;
};

(async () => {
  [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  await showStatus();
  if (tab) await showMedia();
  const { grab: want } = await chrome.storage.session.get('grab'); // set by the right-click menu
  if (want) { await chrome.storage.session.remove('grab'); grab(want); }
})();
