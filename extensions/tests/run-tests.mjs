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
const MD3 = require('../shared/md3.js');
const NMHOST = require('../shared/nmhost.js');

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

console.log('md3 (Material You)');
{
  const light = MD3.schemeFromSeed('#4156A4', false);
  const dark = MD3.schemeFromSeed('#4156A4', true);
  const hexOf = (v) => /^#[0-9a-fA-F]{6}$/.test(v);
  ok('all roles are hex colors', Object.values(light).every(hexOf) && Object.values(dark).every(hexOf));
  ok('light and dark differ', light.primary !== dark.primary && light.surface !== dark.surface);
  ok('light surface is light, dark surface is dark',
    parseInt(light.surface.slice(1, 3), 16) > 200 && parseInt(dark.surface.slice(1, 3), 16) < 40);
  ok('role set complete', ['primary', 'onPrimary', 'primaryContainer', 'onPrimaryContainer',
    'secondary', 'tertiary', 'error', 'success', 'surface', 'onSurface', 'surfaceVariant',
    'onSurfaceVariant', 'surfaceContainer', 'outline', 'outlineVariant', 'background']
    .every((k) => k in light));
  const other = MD3.schemeFromSeed('#006A6A', true);
  ok('scheme is seed-sensitive', other.primary !== dark.primary);
  const again = MD3.schemeFromSeed('#4156A4', true);
  ok('scheme is deterministic', again.primary === dark.primary);
  ok('parseHex accepts #RRGGBB and RRGGBB',
    JSON.stringify(MD3.parseHex('#4156a4')) === '[65,86,164]' && JSON.stringify(MD3.parseHex('4156A4')) === '[65,86,164]');
  ok('parseHex rejects junk', MD3.parseHex('#4156A') === null && MD3.parseHex('zzzzzz') === null && MD3.parseHex('') === null);
  ok('presets all parse', MD3.PRESETS.every(([n, h]) => MD3.parseHex(h) !== null));
  ok('tonal palette: tone 100 is white-ish, tone 0 is black-ish',
    MD3.tonal(258, 48, 100).slice(1, 3) === 'ff' && MD3.tonal(258, 48, 0).slice(1, 3) === '00');
  ok('cssVarName mapping', MD3.cssVarName('primaryContainer') === '--md-primary-container');
  ok('tonal palette stays in gamut at high chroma',
    /^#[0-9a-fA-F]{6}$/.test(MD3.tonal(258, 120, 40)) && /^#[0-9a-fA-F]{6}$/.test(MD3.tonal(60, 120, 80)));
}


console.log('nmhost (B4 — NM host resilience)');
{
  // pure: backoff math (500 ms doubling to the 8 s cap; 30 s reset rule)
  ok('backoff doubles', NMHOST.nextBackoffMs(500) === 1000 &&
    NMHOST.nextBackoffMs(1000) === 2000 &&
    NMHOST.nextBackoffMs(4000) === 8000);
  ok('backoff caps at 8 s', NMHOST.nextBackoffMs(8000) === 8000);
  ok('backoff from scratch starts at 500 ms', NMHOST.nextBackoffMs(0) === 500 && NMHOST.nextBackoffMs(null) === 500);
  ok('stable reset after 30 s only', NMHOST.shouldResetBackoff(30000) && !NMHOST.shouldResetBackoff(29999) && !NMHOST.shouldResetBackoff(null));

  // pure: host-absence matcher
  ok('absence: not found', NMHOST.isHostAbsence('Native host not found.'));
  ok('absence: not installed', NMHOST.isHostAbsence('The specified native host is not installed.'));
  ok('absence: access denied', NMHOST.isHostAbsence('Access is denied.'));
  ok('absence: host exited', NMHOST.isHostAbsence('Native host has exited.'));
  ok('transient errors are not absence', !NMHOST.isHostAbsence('connection reset by peer') &&
    !NMHOST.isHostAbsence('') && !NMHOST.isHostAbsence(null) && !NMHOST.isHostAbsence(undefined));
  ok('absence match is case-insensitive', NMHOST.isHostAbsence('NATIVE HOST HAS EXITED'));

  // pure: bounded queue (cap 32, drop-oldest)
  {
    const q0 = [];
    for (let i = 0; i < NMHOST.QUEUE_CAP; i++) q0.push({ command: { n: i }, requeued: false });
    const res = NMHOST.queuePush(q0, { command: { n: 99 }, requeued: false });
    ok('queue cap is 32', NMHOST.QUEUE_CAP === 32 && res.queue.length === 32);
    ok('drop-oldest keeps the newest', res.dropped.command.n === 0 && res.queue[31].command.n === 99);
  }

  // pure: episode budget window (10 minutes)
  ok('budget: inside / outside / unset', NMHOST.withinBudget(1000, 1000 + 599999) &&
    !NMHOST.withinBudget(1000, 1000 + 600000) && NMHOST.withinBudget(null, 999999));
}

// integration-style with a mock port + fake clock -------------------------
function fakeClock() {
  let ms = 0;
  const pending = [];
  return {
    now: () => ms,
    setTimeout(fn, d) { pending.push({ fn, at: ms + d }); return pending.length - 1; },
    clearTimeout(h) { if (pending[h]) pending[h].fn = null; },
    advance(d) {
      ms += d;
      for (;;) {
        let idx = -1;
        for (let i = 0; i < pending.length; i++) {
          if (pending[i].fn && pending[i].at <= ms) { idx = i; break; }
        }
        if (idx === -1) break;
        const fn = pending[idx].fn;
        pending[idx].fn = null;
        fn();
      }
    },
  };
}

function mockPort() {
  const posted = [];
  const msgL = [];
  const discL = [];
  return {
    posted,
    postMessage(cmd) { posted.push(cmd); },
    disconnect() {},
    onMessage: { addListener(fn) { msgL.push(fn); } },
    onDisconnect: { addListener(fn) { discL.push(fn); } },
    _reply(r) { msgL.slice().forEach((fn) => fn(r)); },
    _drop(err) { discL.slice().forEach((fn) => fn(err)); },
  };
}

function makeLink(over) {
  const clk = fakeClock();
  const env = {
    hostName: 'com.velocitydm.nmh',
    now: clk.now,
    setTimeout: clk.setTimeout,
    clearTimeout: clk.clearTimeout,
  };
  Object.assign(env, over);
  const link = NMHOST.createLink(env);
  return { link, clk };
}

{
  // first send fails (transient drop) -> re-queued -> delivered after reconnect
  const P1 = mockPort();
  let factory = () => P1;
  let connects = 0;
  const { link, clk } = makeLink({
    connectNative: () => { connects++; return factory(); },
  });
  const p = link.send({ cmd: 'ping' });
  ok('connected after the first send', link.state() === 'connected' && connects === 1 && P1.posted.length === 1);
  P1._drop('connection reset by peer'); // in-flight failure, transient
  ok('backoff state after a transient drop', link.state() === 'backoff');
  ok('first retry waits 500 ms', link._nextDelayMs() === 500);
  const P2 = mockPort();
  factory = () => P2;
  clk.advance(500);
  ok('reconnect attempted after the backoff', connects === 2 && link.state() === 'connected');
  ok('command re-queued and delivered after reconnect', P2.posted.length === 1 && P2.posted[0].cmd === 'ping');
  P2._reply({ ok: true, pong: true });
  const r = await p;
  ok('reply resolved after the reconnect', r.ok === true && r.pong === true);
}

{
  // re-queue exactly once: a second transient failure rejects the command
  const P1 = mockPort();
  let factory = () => P1;
  const { link, clk } = makeLink({
    connectNative: () => factory(),
  });
  const p = link.send({ cmd: 'status' });
  P1._drop('connection reset'); // failure #1: re-queued once
  const P2 = mockPort();
  factory = () => P2;
  clk.advance(500);
  P2._drop('connection reset'); // failure #2 while in flight again: rejected
  const r = await p.catch((e) => e);
  ok('second failure rejects the command', r && r.ok === false && r.error === 'engine unreachable');
}

{
  // stable >= 30 s closes the episode: the backoff restarts at 500 ms
  const P1 = mockPort();
  let factory = () => P1;
  const { link, clk } = makeLink({ connectNative: () => factory() } );
  link.send({ cmd: 'ping' }).catch(() => {});
  P1._drop('connection reset');
  const P2 = mockPort();
  factory = () => P2;
  clk.advance(500); // reconnect to P2
  P2._reply({ ok: true }); // reply resolves the first command
  ok('connected and idle after the reconnect', link.state() === 'connected' && link._nextDelayMs() === null);
  // destabilize once to raise the backoff (short-lived stability keeps it)
  P2._drop('connection reset');
  ok('short-lived stability keeps the backoff growing', link.state() === 'backoff' && link._nextDelayMs() === 1000);
  const P3 = mockPort();
  factory = () => P3;
  clk.advance(1000); // the retry tick reconnects to P3
  clk.advance(31000); // 31 s of stability on P3
  P3._drop('connection reset');
  ok('30 s of stability resets the backoff to 500 ms', link.state() === 'backoff' && link._nextDelayMs() === 500);
}

{
  // host absence: graceful degradation — fail fast, no retry churn
  const P1 = mockPort();
  const { link } = makeLink({ connectNative: () => P1 });
  const p = link.send({ cmd: 'add', url: 'https://x/f.zip' });
  P1._drop('Native host not found.');
  ok('absence state entered', link.state() === 'absent');
  ok('no retry churn while absent', link._nextDelayMs() === null);
  const r = await p.catch((e) => e);
  ok('in-flight command failed fast (decline path preserved)', r && r.ok === false && r.error === 'engine unreachable');
  // a fresh link with the host present delivers (the re-probe recovers)
  let lastPort = null;
  const { link: link2 } = makeLink({
    connectNative: () => { lastPort = mockPort(); return lastPort; },
  });
  const q = link2.send({ cmd: 'ping' });
  ok('re-probe connects when the host appears', link2.state() === 'connected' && lastPort && lastPort.posted.length === 1);
  lastPort._reply({ ok: true, pong: true });
  const r2 = await q;
  ok('recovered link delivers the command', r2 && r2.ok === true && r2.pong === true);
}

{
  // episode budget: repeated refusals stop after 10 minutes (dead state)
  const P1 = mockPort();
  let factory = () => P1;
  let refused = false;
  const { link, clk } = makeLink({
    connectNative: () => (refused ? null : P1),
  });
  const p0 = link.send({ cmd: 'ping' }).catch((e) => e);
  P1._drop('connection reset');
  refused = true; // every reconnect attempt refuses from now on
  let guard = 0;
  while (link.state() !== 'dead' && guard < 500) {
    clk.advance(9000); // longer than any single backoff wait
    guard++;
  }
  ok('reconnect attempts stop after the 10-minute budget', link.state() === 'dead');
  const r0 = await p0;
  ok('the queued command is rejected when the budget expires', r0 && r0.ok === false && r0.error === 'engine unreachable');
  const r = await link.send({ cmd: 'ping' }).catch((e) => e);
  ok('dead link rejects new commands immediately', r && r.ok === false && r.error === 'engine unreachable');
}

{
  // one-shot fallback: delivery without a port capability
  const sent = [];
  const { link } = makeLink({
    sendNativeMessage: (name, command, cb) => { sent.push({ name, command }); cb({ ok: true, pong: true }); },
  });
  const r = await link.send({ cmd: 'ping' });
  ok('one-shot fallback delivers', r.ok === true && r.pong === true && sent.length === 1 &&
    sent[0].name === 'com.velocitydm.nmh');
  ok('one-shot link reports connected', link.state() === 'connected');
}

{
  // one-shot fallback with host absence: fail fast + decline preserved
  const sent = [];
  const { link } = makeLink({
    sendNativeMessage: (name, command, cb) => { sent.push(command); cb(undefined); },
  });
  globalThis.chrome = { runtime: { lastError: { message: 'Native host not found.' } } };
  const p = link.send({ cmd: 'add', url: 'https://x/f.zip' });
  const r = await p.catch((e) => e);
  ok('one-shot absence rejects the command', r && r.ok === false && r.error === 'engine unreachable');
  ok('one-shot absence degrades to absent state', link.state() === 'absent');
  delete globalThis.chrome;
  ok('command payload was sent to the one-shot API', sent.length === 1 && sent[0].cmd === 'add');
}

{
  // secrecy precondition for B5: the log channel never carries commands
  const notes = [];
  const P1 = mockPort();
  const { link } = makeLink({
    connectNative: () => P1,
    log: (m) => notes.push(m),
  });
  // fill the pipeline: one in flight + a full queue -> the 34th drops oldest
  for (let i = 0; i < NMHOST.QUEUE_CAP + 2; i++) {
    link.send({ cmd: 'add', url: 'https://x/f.zip', cookies: 'SESSIONCOOKIE' + i + '=sekrit' + i }).catch(() => {});
  }
  ok('queue-full note fired once', notes.length === 1);
  ok('log channel carries static strings only',
    notes.every((n) => n.indexOf('sekrit') === -1 && n.indexOf('SESSIONCOOKIE') === -1));
}

console.log('\n' + passed + ' assertions passed');
if (process.exitCode) console.error('FAILURES PRESENT');
