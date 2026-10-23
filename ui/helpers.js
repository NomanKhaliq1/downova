// Submission always uses the original text; this preview is bounded to protect the UI.
function expandPreview(text, cap = 500) {
  const out = [];
  const expand = (line) => {
    if (out.length >= cap) return;
    const m = /\[([0-9]+)-([0-9]+)(?::([0-9]+))?\]|\[([a-zA-Z])-([a-zA-Z])(?::([0-9]+))?\]/.exec(line);
    if (!m) { out.push(line); return; }
    const numeric = m[1] !== undefined;
    const from = numeric ? +m[1] : m[4].charCodeAt(0), to = numeric ? +m[2] : m[5].charCodeAt(0), step = +(m[3] || m[6] || 1);
    if (!step || from > to || !Number.isSafeInteger(to) || (!numeric && (m[4] === m[4].toLowerCase()) !== (m[5] === m[5].toLowerCase()))) { out.push(line); return; }
    const width = numeric && m[1].startsWith('0') ? m[1].length : 0;
    for (let n = from; n <= to && out.length < cap; n += step) expand(line.slice(0,m.index) + (numeric ? String(n).padStart(width,'0') : String.fromCharCode(n)) + line.slice(m.index+m[0].length));
  };
  for (const line of text.split(/\r?\n/).map(x=>x.trim()).filter(x=>x && !x.startsWith('#'))) expand(line);
  return out;
}
function filterLinks(text) {
  const m = /^\/(.*)\/([a-z]*)$/.exec(text.trim());
  if (m) { const re = new RegExp(m[1], m[2].replace(/[gy]/g,'') || 'i'); return value=>re.test(value); }
  const words = text.toLowerCase().trim().split(/\s+/);
  return value=>words.every(word=>value.toLowerCase().includes(word));
}
const percent = t => t.total ? Math.min(100,Math.max(0,Math.round(t.downloaded/t.total*100))) : 0;
