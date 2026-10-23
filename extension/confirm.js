import { formatSize } from './lib.js';

const $ = (id) => document.getElementById(id);
const send = (m) => chrome.runtime.sendMessage(m);
const key = new URLSearchParams(location.search).get('k');
const join = (a, b) => (/^[a-z]:[\\/]|^[\\/]{2}/i.test(b) ? b : `${a.replace(/[\\/]+$/, '')}\\${b}`);
const split = (p) => {
  const i = Math.max(p.lastIndexOf('\\'), p.lastIndexOf('/'));
  return [p.slice(0, i), p.slice(i + 1)];
};

let info, settings, paths;

/** Folder for a category: the one the user remembered here, else the app's own. */
const folderOf = (name) => {
  const c = settings.categories.find((x) => x.name === name);
  return paths[name] || (c ? join(settings.download_root, c.folder) : settings.download_root);
};

function onCategory() {
  const name = $('cat').value;
  $('path').value = join(folderOf(name), split($('path').value)[1]);
  $('remember').checked = false;
  $('remember').parentElement.hidden = !name;
  $('rememberText').textContent = `Remember this path for "${name}"`;
}

async function init() {
  ({ pending: info, settings } = await send({ type: 'confirm_info', key }));
  if (!info || !settings) return window.close();
  ({ paths } = await chrome.storage.local.get({ paths: {} }));
  const ext = (info.filename.split('.').pop() || '').toLowerCase();
  const auto = settings.categories.find((c) => c.extensions.includes(ext));
  $('url').value = info.url;
  $('ext').textContent = info.filename.includes('.') ? ext.slice(0, 5).toUpperCase() : 'FILE';
  $('size').textContent = info.size ? formatSize(info.size) : 'Size unknown';
  $('cat').append(new Option('General', ''), ...settings.categories.map((c) => new Option(c.name, c.name)));
  $('cat').value = auto?.name || '';
  $('path').value = info.filename;
  $('cat').onchange = onCategory;
  onCategory();
  $('start').focus();
  if (ext && info.filename.includes('.')) {
    const { src } = (await send({ type: 'file_icon', ext })) || {};
    if (src) {
      $('icon').src = src;
      $('icon').hidden = false;
    }
  }
}

async function go(later) {
  const [dir, filename] = split($('path').value.trim());
  if (!dir || !filename) return ($('msg').textContent = 'Enter a full path, for example C:\\Downloads\\file.zip');
  $('start').disabled = $('later').disabled = true;
  if ($('remember').checked) await chrome.storage.local.set({ paths: { ...paths, [$('cat').value]: dir } });
  const r = await send({ type: 'confirm_go', key, filename, dir, later });
  const id = r?.body?.added?.[0] ?? r?.body?.duplicates?.[0];
  if (!r?.ok || id === undefined) {
    $('msg').textContent = 'Could not add the download. Is the app running?';
    $('start').disabled = $('later').disabled = false;
    return;
  }
  if (later) return window.close();
  watch(id);
}

async function browse() {
  $('browse').disabled = true;
  const r = await send({ type: 'pick_path', path: $('path').value.trim() });
  $('browse').disabled = false;
  if (r?.path) $('path').value = r.path;
  else if (r?.error) $('msg').textContent = 'Could not open the folder window.';
}

function watch(id) {
  $('ask').hidden = true;
  $('prog').hidden = false;
  let state = '';
  $('pause').onclick = () => send({ type: 'task_act', id, act: state === 'Paused' || state === 'Failed' ? 'resume' : 'pause' });
  const tick = async () => {
    const t = (await send({ type: 'task_get', id }))?.body;
    if (!t) return;
    state = t.status;
    const pct = t.total ? Math.min(100, (t.downloaded / t.total) * 100) : 0;
    $('pname').textContent = t.name || info.filename;
    $('fill').style.width = `${state === 'Done' ? 100 : pct}%`;
    const eta = t.eta_secs != null && state === 'Downloading' ? ` · ${t.eta_secs}s left` : '';
    $('pinfo').textContent = state === 'Done' ? `Finished · ${formatSize(t.total || t.downloaded)}` : state === 'Failed' ? `Failed: ${t.error || ''}` : `${state} · ${formatSize(t.downloaded)}${t.total ? ` of ${formatSize(t.total)}` : ''}${state === 'Downloading' ? ` · ${formatSize(t.speed)}/s${eta}` : ''}`;
    $('pause').textContent = state === 'Paused' ? 'Resume' : state === 'Failed' ? 'Retry' : 'Pause';
    $('pause').hidden = state === 'Done';
    document.title = state === 'Done' ? 'Finished' : `${Math.round(pct)}%`;
  };
  tick();
  setInterval(tick, 700);
}

$('start').onclick = () => go(false);
$('later').onclick = () => go(true);
$('browse').onclick = browse;
$('cancel').onclick = () => window.close();
$('close').onclick = () => window.close();
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') window.close();
  else if (e.key === 'Enter' && !$('ask').hidden && e.target.tagName !== 'BUTTON') go(false);
});
init();
