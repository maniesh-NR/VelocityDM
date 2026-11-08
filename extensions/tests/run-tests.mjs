#!/usr/bin/env node
/**
 * VelocityDM extension test suite (shared modules).
 * Run: node tests/run-tests.mjs
 */
import { createRequire } from 'node:module';
import assert from 'node:assert/strict';

const require = createRequire(import.meta.url);
const DET = require('../shared/detector.js');
const PAT = require('../shared/patterns.js');
const LINKS = require('../shared/links.js');
const FILTERS = require('../shared/filters.js');
const MSG = require('../shared/messaging.js');
const STATE = require('../shared/state.js');

let passed = 0;
function ok(name, cond) {
  if (cond) { passed++; console.log('  ok -', name); }
  else { console.error('  FAIL -', name); process.exitCode = 1; }
}

console.log('detector');
ok('m3u8 by extension', DET.isHls('https://x/live/master.m3u8', ''));
ok('m3u8 by mime', DET.isHls('https://x/playlist', 'application/vnd.apple.mpegurl'));
ok('mpd detection', DET.isDash('https://x/stream.mpd', 'application/dash+xml'));
ok('video by mime', DET.isVideo('https://x/blob?id=1', 'video/mp4', 10 * 1024 * 1024));
ok('video by extension', DET.isVideo('https://x/movie.mkv', '', 0));
ok('small clips are not video', !DET.isVideo('https://x/clip.mp4', 'video/mp4', 1024));
ok('html is not video', !DET.isVideo('https://x/page.html', 'text/html', 0));
ok('audio detection', DET.isAudio('https://x/song.mp3', '') && DET.isAudio('https://x/t', 'audio/ogg'));
ok('category stream', DET.category('https://x/a.m3u8', '') === 'video-stream');
ok('category archive', DET.category('https://x/a.zip', '') === 'archive');
ok('category program', DET.category('https://x/a.exe', '') === 'program');
ok('category document', DET.category('https://x/a.pdf', '') === 'document');
ok('category page', DET.category('https://x/a.html', 'text/html') === 'page');
ok('quality resolution in url', DET.qualityLabel('https://x/1920x1080/v.mp4', '') === '1080p');
ok('quality height tag', DET.qualityLabel('https://x/video_720p.mp4', '') === '720p');
ok('quality hls label', DET.qualityLabel('https://x/playlist.m3u8', '') === 'HLS');
ok('title extraction', DET.titleFor('https://x/path/My%20Video.mp4?x=1') === 'My Video.mp4');
ok('extension extraction', DET.extOf('https://x/a/b/file.TAR.gz?x=1') === 'gz');

console.log('patterns');
{
  const r1 = PAT.expand('https://x/pic[1:3].jpg');
  ok('numeric range', r1.ok && JSON.stringify(r1.urls) === JSON.stringify(['https://x/pic1.jpg', 'https://x/pic2.jpg', 'https://x/pic3.jpg']));
  const r2 = PAT.expand('https://x/p[001:002].png');
  ok('padding preserved', r2.ok && r2.urls[0] === 'https://x/p001.png' && r2.urls[1] === 'https://x/p002.png');
  const r3 = PAT.expand('https://x/f[1,4,7].zip');
  ok('enum list', r3.ok && r3.urls.length === 3 && r3.urls[1] === 'https://x/f4.zip');
  const r4 = PAT.expand('https://x/[1:2]/a[1:2].bin');
  ok('two groups cartesian', r4.ok && r4.urls.length === 4);
  ok('plain passthrough', PAT.expand('https://x/file.zip').urls[0] === 'https://x/file.zip');
  ok('descending range rejected', !PAT.expand('x[3:1].y').ok);
  ok('unbalanced bracket rejected', !PAT.expand('x[1.y').ok);
  ok('non-numeric rejected', !PAT.expand('x[a:b].y').ok);
  ok('huge range rejected', !PAT.expand('x[1:500000].y').ok);
  ok('empty rejected', !PAT.expand('').ok);
  ok('list parse', JSON.stringify(PAT.parseList('# c\nhttps://a/1\n\n  https://a/2 ')) === JSON.stringify(['https://a/1', 'https://a/2']));
  ok('url validation', PAT.isHttpUrl('http://x/') && !PAT.isHttpUrl('ftp://x/'));
  ok('magnet validation', PAT.isMagnet('magnet:?xt=urn:btih:abc&tr=http://t/') && !PAT.isMagnet('http://x/'));
}

console.log('links');
{
  const anchors = [
    { href: 'https://x/a/file.zip', text: 'file one', download: null },
    { href: 'https://x/a/file.zip', text: 'dup', download: null },
    { href: 'https://x/b/movie.mp4', text: 'movie', download: null },
    { href: 'https://x/c/setup.exe', text: '', download: 'custom-setup.exe' },
    { href: 'https://x/page.html', text: 'page', download: null },
    { href: 'javascript:void(0)', text: 'js', download: null },
  ];
  const images = [{ src: 'https://x/img/pic.png', alt: 'pic' }];
  const groups = LINKS.extract(anchors, images);
  ok('dedupe', groups.archive.length === 1);
  ok('video grouping', groups.video.length === 1 && groups.video[0].name === 'movie.mp4');
  ok('download attr name', groups.program[0].name === 'custom-setup.exe');
  ok('pages dropped to other', groups.other.some(function (e) { return e.url.endsWith('.html'); }));
  ok('js urls ignored', !groups.other.some(function (e) { return e.url.startsWith('javascript'); }));
  ok('images included', groups.other.some(function (e) { return e.url.endsWith('.png'); }));
  const filtered = LINKS.filterGroups(groups, ['archive']);
  ok('filter keeps only wanted', filtered.archive && !filtered.video);
  ok('flatten', LINKS.flatten(groups).length === 5);
}

console.log('filters');
{
  const s = FILTERS.mergeSettings({ excludedHosts: ['mail.example.com'] });
  ok('defaults merged', s.minSizeBytes > 0 && s.interceptCategories.indexOf('archive') !== -1);
  ok('host matching', FILTERS.isExcluded('https://mail.example.com/x', s) && FILTERS.isExcluded('https://a.mail.example.com/x', s));
  ok('other hosts pass', !FILTERS.isExcluded('https://example.com/x', s));
  ok('zip always intercepted', FILTERS.shouldTakeOver({ url: 'https://x/f.zip', filename: 'f.zip', fileSize: 10 }, s));
  ok('small unknown file not taken', !FILTERS.shouldTakeOver({ url: 'https://x/f.bin', filename: 'f.bin', fileSize: 1024 }, s));
  ok('large file taken', FILTERS.shouldTakeOver({ url: 'https://x/f.bin', filename: 'f.bin', fileSize: 50 * 1024 * 1024 }, s));
  ok('video taken regardless of size', FILTERS.shouldTakeOver({ url: 'https://x/v.mp4', filename: 'v.mp4', fileSize: 0 }, s));
  ok('excluded host not taken', !FILTERS.shouldTakeOver({ url: 'https://mail.example.com/f.zip', filename: 'f.zip' }, s));
  ok('extension downloads not taken', !FILTERS.shouldTakeOver({ url: 'https://x/f.zip', filename: 'f.zip', byExtensionId: 'abc' }, s));
  ok('torrent taken', FILTERS.shouldTakeOver({ url: 'https://x/f.torrent', filename: 'f.torrent' }, s));
  ok('category via mime', FILTERS.categoryOf({ url: 'https://x/blob', mime: 'video/webm' }) === 'video');
}

console.log('messaging');
{
  const c1 = MSG.add({ url: 'https://x/f.zip', referer: 'https://x/', cookies: 'a=b' });
  ok('add command shape', c1.cmd === 'add' && c1.url === 'https://x/f.zip' && c1.referer === 'https://x/' && c1.cookies === 'a=b');
  ok('add omits empties', !('filename' in c1) && !('quality' in c1));
  const c2 = MSG.addBatch('https://x/p[1:2].jpg');
  ok('addBatch pattern', c2.cmd === 'addBatch' && c2.pattern === 'https://x/p[1:2].jpg');
  const c3 = MSG.addBatch(['u1', 'u2']);
  ok('addBatch urls', Array.isArray(c3.urls) && c3.urls.length === 2);
  const c4 = MSG.addTorrent('magnet:?xt=urn:btih:abc');
  ok('magnet routed', c4.cmd === 'addTorrent' && c4.magnet && !c4.torrentPath);
  const c5 = MSG.addTorrent('/tmp/x.torrent');
  ok('torrent path routed', c5.torrentPath === '/tmp/x.torrent' && !c5.magnet);
  ok('ping/status/open', MSG.ping().cmd === 'ping' && MSG.status().cmd === 'status' && MSG.open().cmd === 'open');
  ok('normalize ok reply', MSG.normalize({ ok: true, id: 3 }).ok === true);
  ok('normalize error reply', MSG.normalize({ ok: false, error: 'boom' }).error === 'boom');
  ok('normalize garbage', !MSG.normalize(null).ok && !MSG.normalize('x').ok);
}

console.log('state');
{
  const store = STATE.recentStore(3);
  store.push({ url: 'a' });
  store.push({ url: 'b' });
  store.push({ url: 'c' });
  store.push({ url: 'a' });
  ok('lru update moves to front', store.list()[0].url === 'a' && store.size() === 3);
  store.push({ url: 'd' });
  ok('cap enforced', store.size() === 3 && !store.list().some(function (e) { return e.url === 'b'; }));
  ok('clear', store.clear().length === 0);
  ok('seenAt set', typeof store.push({ url: 'z' })[0].seenAt === 'number');
}

console.log('\n' + passed + ' assertions passed');
if (process.exitCode) console.error('FAILURES PRESENT');
