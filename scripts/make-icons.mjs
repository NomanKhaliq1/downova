// Draws the placeholder extension icons from the brand colour. Replace with brand/assets artwork later.
// usage: node scripts/make-icons.mjs [outDir]
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { deflateSync, crc32 } from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const outDir = process.argv[2] || join(root, 'extension', 'icons');
const hex = /primary_color\s*=\s*"#?([0-9a-fA-F]{6})"/.exec(readFileSync(join(root, 'brand', 'brand.toml'), 'utf8'))?.[1] || '2563eb';
const [r, g, b] = [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16));

function chunk(type, data) {
  const t = Buffer.from(type);
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([t, data])));
  return Buffer.concat([len, t, data, crc]);
}

function png(size) {
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0; // filter: none
    for (let x = 0; x < size; x++) {
      const fx = ((x + 0.5) / size) * 32;
      const fy = ((y + 0.5) / size) * 32;
      const dx = Math.abs(fx - 16) - 10;
      const dy = Math.abs(fy - 16) - 10;
      const inside = Math.hypot(Math.max(dx, 0), Math.max(dy, 0)) <= 6 && Math.max(dx, dy) <= 6;
      const shaft = fx >= 14 && fx < 18 && fy >= 7 && fy < 18;
      const head = fy >= 15 && fy <= 25 && Math.abs(fx - 16) <= (25 - fy) * 0.9 + 0.5;
      const base = fx >= 8 && fx < 24 && fy >= 26 && fy < 28.5;
      const white = shaft || head || base;
      const i = y * (size * 4 + 1) + 1 + x * 4;
      const px = !inside ? [0, 0, 0, 0] : white ? [255, 255, 255, 255] : [r, g, b, 255];
      raw.set(px, i);
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr.set([8, 6, 0, 0, 0], 8); // 8-bit RGBA
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}

mkdirSync(outDir, { recursive: true });
for (const s of [16, 32, 48, 128]) writeFileSync(join(outDir, `${s}.png`), png(s));
console.log(`icons written to ${outDir} (#${hex})`);
