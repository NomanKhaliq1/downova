import test from 'node:test';
import assert from 'node:assert/strict';
import { classifyMedia, collectFromPage, cookieHeader, fileName, formatSize, isCapturableUrl, makeFilter, mediaKey, rangeTotal, safeForBatch } from '../lib.js';

const MB = 1024 * 1024;

test('only http(s) links are captured', () => {
  assert.equal(isCapturableUrl('https://a.com/x.zip'), true);
  assert.equal(isCapturableUrl('blob:https://a.com/uuid'), false);
  assert.equal(isCapturableUrl('data:text/plain,hi'), false);
  assert.equal(isCapturableUrl('chrome-extension://x/y'), false);
  assert.equal(isCapturableUrl(''), false);
});

test('cookie header joins name=value pairs', () => {
  assert.equal(cookieHeader([{ name: 'a', value: '1' }, { name: 'b', value: 'x y' }, { name: '', value: 'skip' }]), 'a=1; b=x y');
  assert.equal(cookieHeader([]), '');
  assert.equal(cookieHeader(undefined), '');
});

test('video and audio by content type, with a size floor', () => {
  assert.deepEqual(classifyMedia('https://c.net/v/clip', { 'content-type': 'video/mp4', 'content-length': String(5 * MB) }), { kind: 'video', size: 5 * MB });
  assert.deepEqual(classifyMedia('https://c.net/a', { 'content-type': 'audio/mpeg; charset=x', 'content-length': String(3 * MB) }), { kind: 'audio', size: 3 * MB });
  assert.equal(classifyMedia('https://c.net/ad.mp4', { 'content-type': 'video/mp4', 'content-length': '5000' }), null, 'tiny clips are ignored');
  assert.equal(classifyMedia('https://c.net/x', { 'content-type': 'text/html' }), null);
});

test('size comes from Content-Range when the server sends a partial response', () => {
  const m = classifyMedia('https://c.net/movie.mp4', { 'content-type': 'video/mp4', 'content-length': '65536', 'content-range': `bytes 0-65535/${900 * MB}` });
  assert.equal(m.size, 900 * MB);
  assert.equal(rangeTotal('bytes 0-9/*'), null);
});

test('octet-stream falls back to the file extension', () => {
  assert.equal(classifyMedia('https://c.net/dl/film.mkv?t=1', { 'content-type': 'application/octet-stream', 'content-length': String(80 * MB) }).kind, 'video');
  assert.equal(classifyMedia('https://c.net/dl/song.flac', { 'content-length': String(30 * MB) }).kind, 'audio');
  assert.equal(classifyMedia('https://c.net/dl/data.bin', { 'content-type': 'application/octet-stream', 'content-length': String(80 * MB) }), null);
});

test('streams: manifests count, their pieces do not', () => {
  assert.equal(classifyMedia('https://c.net/live/master.m3u8', { 'content-type': 'application/vnd.apple.mpegurl' }).kind, 'hls');
  assert.equal(classifyMedia('https://c.net/x/index.m3u8?sig=1', {}).kind, 'hls');
  assert.equal(classifyMedia('https://c.net/m/manifest.mpd', { 'content-type': 'application/dash+xml' }).kind, 'dash');
  assert.equal(classifyMedia('https://c.net/live/seg-001.ts', { 'content-type': 'video/mp2t', 'content-length': String(5 * MB) }), null);
  assert.equal(classifyMedia('https://c.net/v/chunk-4.m4s', { 'content-type': 'video/iso.segment', 'content-length': String(5 * MB) }), null);
});

test('the same file with different tokens is one entry', () => {
  assert.equal(mediaKey('https://c.net/v/a.mp4?token=1'), mediaKey('https://c.net/v/a.mp4?token=2'));
  assert.notEqual(mediaKey('https://c.net/v/a.mp4'), mediaKey('https://c.net/v/b.mp4'));
});

test('batch safety keeps literal brackets from turning into ranges', () => {
  assert.equal(safeForBatch('https://x.com/a[1-3].jpg'), 'https://x.com/a%5B1-3%5D.jpg');
  assert.equal(safeForBatch('https://x.com/plain.jpg'), 'https://x.com/plain.jpg');
});

test('filter: words, regex and bad regex', () => {
  const words = makeFilter('holiday jpg');
  assert.equal(words('Holiday-2024.JPG'), true);
  assert.equal(words('holiday.png'), false);
  assert.equal(makeFilter('/\\.(png|gif)$/')('a.GIF'), true);
  assert.equal(makeFilter('/(/')('anything'), false);
  assert.equal(makeFilter('')('x'), true);
});

test('names and sizes read well', () => {
  assert.equal(fileName('https://x.com/a/b/My%20File.mp4?x=1'), 'My File.mp4');
  assert.equal(fileName('https://x.com/'), 'x.com');
  assert.equal(formatSize(null), '');
  assert.equal(formatSize(512), '512 B');
  assert.equal(formatSize(1536), '1.5 KB');
  assert.equal(formatSize(300 * MB), '300 MB');
});

test('collectFromPage de-duplicates and keeps only http(s)', () => {
  const links = [{ href: 'https://a.com/1', textContent: ' One ' }, { href: 'https://a.com/1', textContent: 'dup' }, { href: 'javascript:void(0)', textContent: 'x' }, { href: 'https://a.com/pic.png', textContent: 'p' }];
  const imgs = [{ currentSrc: 'https://a.com/i.jpg', src: '', alt: 'cat' }, { currentSrc: '', src: 'data:image/png;base64,AA', alt: '' }];
  globalThis.document = { querySelectorAll: (sel) => (sel === 'img' ? imgs : links) };
  assert.deepEqual(collectFromPage('links').map((x) => x.url), ['https://a.com/1', 'https://a.com/pic.png']);
  assert.equal(collectFromPage('links')[0].text, 'One');
  assert.deepEqual(collectFromPage('images').map((x) => x.url), ['https://a.com/i.jpg', 'https://a.com/pic.png']);
  delete globalThis.document;
});

test('YouTube player slices are not downloadable files, but the watch page is a video page', async () => {
  const { isYtdlpPage } = await import('../lib.js');
  assert.equal(classifyMedia('https://rr3---sn-x.googlevideo.com/videoplayback?expire=1&range=0-99', { 'content-type': 'video/mp4', 'content-length': String(5 * MB) }), null);
  assert.equal(isYtdlpPage('https://www.youtube.com/watch?v=abc123'), true);
  assert.equal(isYtdlpPage('https://www.youtube.com/'), false);
  assert.equal(isYtdlpPage('https://www.youtube.com/watch'), false);
  assert.equal(isYtdlpPage('https://youtu.be/abc123'), true);
  assert.equal(isYtdlpPage('https://www.youtube.com/shorts/xyz'), true);
  assert.equal(isYtdlpPage('https://vimeo.com/12345'), true);
  assert.equal(isYtdlpPage('https://vimeo.com/'), false);
  assert.equal(isYtdlpPage('https://example.com/watch?v=1'), false);
  assert.equal(isYtdlpPage('not a url'), false);
});
