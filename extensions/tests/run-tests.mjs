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
const COOKIES = require('../shared/cookies.js');
const TK = require('../shared/takeover.js');

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

console.log('cookies (B5 — jar forwarding on every add path)');
{
  // serializer shape (matches the takeover path's output)
  ok('serializer matches the takeover shape',
    COOKIES.serializeJar([{ name: 'a', value: '1' }, { name: 'b', value: '2' }]) === 'a=1; b=2');
  ok('empty jar forwards an empty string', COOKIES.serializeJar([]) === '');
  ok('malformed entries are skipped', COOKIES.serializeJar([{ name: 'a', value: '1' }, null, { name: '', value: 'x' }]) === 'a=1');
  ok('non-array tolerated', COOKIES.serializeJar(null) === '');

  // jarFor with a fake cookies API (session AND persistent cookies alike)
  const api = { getAll: (q) => Promise.resolve([{ name: 'sid', value: 'S1' }, { name: 'pref', value: 'P' }]) };
  const jar = await COOKIES.jarFor('https://x/f.zip', api);
  ok('jar read serializes name=value pairs', jar === 'sid=S1; pref=P');
  ok('non-http(s) URLs get no jar', (await COOKIES.jarFor('magnet:?xt=1', api)) === '' &&
    (await COOKIES.jarFor('ftp://x/', api)) === '');
  ok('absent API tolerated (best-effort)', (await COOKIES.jarFor('https://x/f.zip', null)) === '');
  const throwing = { getAll: () => Promise.reject(new Error('restricted')) };
  ok('restricted permission tolerated (empty jar)', (await COOKIES.jarFor('https://x/f.zip', throwing)) === '');

  // withJar attaches the jar to add items (the shared add-path helper)
  const withJar = await COOKIES.withJar({ url: 'https://x/f.zip', referer: 'https://x/' }, api);
  ok('withJar attaches cookies to the add item', withJar.cookies === 'sid=S1; pref=P' && withJar.referer === 'https://x/');
  const magnet = await COOKIES.withJar({ url: 'magnet:?xt=urn:btih:abc' }, api);
  ok('magnet adds carry no cookies', !('cookies' in magnet));
  const emptyJarApi = { getAll: () => Promise.resolve([]) };
  const empty = await COOKIES.withJar({ url: 'https://x/f.zip' }, emptyJarApi);
  ok('empty jar forwards an empty string (today\'s shape)', empty.cookies === '');

  // batch plan: per-URL, capped at the first 32 http(s) URLs
  const urls = [];
  for (let i = 0; i < 40; i++) urls.push('https://x/' + i);
  urls.splice(5, 0, 'magnet:?xt=1', 'javascript:void(0)');
  const plan = COOKIES.batchCookiePlan(urls);
  ok('batch cookie cap is 32', plan.withCookies.length === 32 && COOKIES.BATCH_COOKIE_CAP === 32);
  ok('only http(s) URLs get jars', plan.withCookies.every((u) => /^https?:/.test(u)));
  ok('non-http urls and overflow are skipped', plan.skipped === urls.length - 32);
  ok('empty batch plans empty', COOKIES.batchCookiePlan([]).withCookies.length === 0);

  // jarsForBatch: the per-URL map only carries NON-EMPTY jars
  const perUrl = {};
  for (let i = 0; i < 40; i++) perUrl['https://x/' + i] = (i === 0) ? [] : [{ name: 't', value: String(i) }];
  const batchApi = { getAll: (q) => Promise.resolve(perUrl[q.url] || []) };
  const r = await COOKIES.jarsForBatch(urls, batchApi);
  ok('batch map covers the first 32 http(s) URLs', r.covered === 32);
  ok('empty jars are omitted from the map', !('https://x/0' in r.map));
  ok('map values are serialized jars', r.map['https://x/1'] === 't=1');

  // every add path builds a command carrying cookies (builder level):
  // context-menu/popup/takeover single adds ride MSG.add's cookies field
  const addCmd = MSG.add({ url: 'https://x/f.zip', referer: 'https://x/', cookies: 'sid=S1' });
  ok('single add carries cookies', addCmd.cmd === 'add' && addCmd.cookies === 'sid=S1');
  // batch adds ride MSG.addBatch's cookiesBy (cap 32)
  const batchCmd = MSG.addBatch(urls, { 'https://x/1': 't=1' });
  ok('batch add carries the per-URL cookies map', batchCmd.cmd === 'addBatch' && batchCmd.cookiesBy['https://x/1'] === 't=1');
  ok('batch add without jars keeps today\'s shape', !('cookiesBy' in MSG.addBatch(['u1'])));
  ok('pattern batch keeps today\'s shape', !('cookiesBy' in MSG.addBatch('https://x/p[1:2].jpg')));

  // no-cookie-leak (the secrecy condition): no jar substring reaches a
  // captured log channel across the module surface
  const logs = [];
  const origLog = console.log;
  const origWarn = console.warn;
  const origErr = console.error;
  console.log = (...a) => logs.push(a.join(' '));
  console.warn = (...a) => logs.push(a.join(' '));
  console.error = (...a) => logs.push(a.join(' '));
  const secretApi = { getAll: () => Promise.resolve([{ name: 'SID', value: 'TOPSECRET42' }]) };
  await COOKIES.withJar({ url: 'https://sec.example/f.zip' }, secretApi);
  await COOKIES.jarsForBatch(['https://sec.example/f.zip'], secretApi);
  const batchLeak = MSG.addBatch(['https://sec.example/f.zip'], { 'https://sec.example/f.zip': 'SID=TOPSECRET42' });
  const leakStrings = JSON.stringify(batchLeak);
  console.log = origLog;
  console.warn = origWarn;
  console.error = origErr;
  ok('no cookie value reaches any captured log channel',
    logs.every((l) => l.indexOf('TOPSECRET42') === -1));
  ok('commands do carry the jar (the leak test would be vacuous otherwise)', leakStrings.indexOf('TOPSECRET42') !== -1);
}

console.log('takeover (B6 — takeover UX)');
{
  // shelf key: takeover enabled while any intercept category is on
  ok('shelf key: default settings enable takeover', TK.isEnabled(FILTERS.mergeSettings({})) === true);
  ok('shelf key: empty categories disable takeover', TK.isEnabled(FILTERS.mergeSettings({ interceptCategories: [] })) === false);
  ok('shelf key: absent settings disabled', TK.isEnabled(null) === false && TK.isEnabled(undefined) === false);

  // filename mirror (the engine-side naming rule, ours)
  ok('filename mirror: basename of the browser filename',
    TK.filenameFor({ filename: '/home/u/Video.mkv', url: 'https://x/v' }) === 'Video.mkv');
  ok('filename mirror: URL basename fallback', TK.filenameFor({ url: 'https://x/files/clip.mp4' }) === 'clip.mp4');
  ok('filename mirror: mime ext appended when no ext',
    TK.filenameFor({ filename: 'video', mime: 'video/mp2t' }, (m) => (m === 'video/mp2t' ? 'ts' : null)) === 'video.ts');
  ok('filename mirror: existing ext never overridden',
    TK.filenameFor({ filename: 'clip.bin', mime: 'video/mp4' }, () => 'mp4') === 'clip.bin');
  ok('filename mirror: unknown mime leaves the name alone',
    TK.filenameFor({ filename: 'video', mime: 'application/x-unknown' }, () => null) === 'video');
  ok('filename mirror: no mime table wired keeps today\'s name',
    TK.filenameFor({ filename: 'video', mime: 'video/mp4' }) === 'video');
  ok('filename mirror: nothing usable yields empty',
    TK.filenameFor({}) === '' && TK.filenameFor(null) === '');

  // decline notes: our static words
  ok('unreachable note', TK.noteFor({ ok: false, error: 'engine unreachable' }) === TK.NOTE_UNREACHABLE);
  ok('malformed-reply note', TK.noteFor({ ok: false, error: 'malformed host reply' }) === TK.NOTE_UNREACHABLE);
  ok('other decline note', TK.noteFor({ ok: false, error: 'missing url' }) === TK.NOTE_DECLINED);
  ok('garbage reply note', TK.noteFor(null) === TK.NOTE_DECLINED);
  ok('accept has no note', TK.noteFor({ ok: true }) === '');

  // confirm target: encodeURIComponent in the hash
  ok('confirm target encodes the url',
    TK.confirmTarget('https://x/a b.mp4', 'chrome-extension://abc/taken.html') ===
    'chrome-extension://abc/taken.html#https%3A%2F%2Fx%2Fa%20b.mp4');

  // taken.html render rule: XSS-attempt fixture renders inert
  {
    const el = { textContent: '' };
    const xss = '#<img src=x onerror=alert(1)>><script>evil()</script>';
    const out = TK.renderConfirmInto(el, xss);
    ok('xss fixture renders as text only', out === xss.substring(1) &&
      String(el.textContent) === xss.substring(1));
    ok('empty hash renders nothing', TK.renderConfirmInto(el, '') === null &&
      TK.renderConfirmInto(el, '#') === null);
    ok('percent-encoded hash decodes', TK.renderConfirmInto(el, '#https%3A%2F%2Fx%2Fv.mp4') === 'https://x/v.mp4');
    ok('raw percent sequence tolerated', TK.renderConfirmInto(el, '#100%') === '100%');
    ok('null element tolerated', TK.renderConfirmInto(null, '#x') === null);
  }

  // handOff orchestration with injectable deps
  function makeDeps(reply) {
    const calls = { paused: [], canceled: [], erased: [], resumed: [], notes: [], sent: [] };
    return {
      calls,
      deps: {
        getSettings: () => Promise.resolve(FILTERS.mergeSettings({})),
        shouldTakeOver: FILTERS.shouldTakeOver,
        withJar: (item) => Promise.resolve(Object.assign({ cookies: 'a=b' }, item)),
        jarApi: null,
        send: (cmd) => { calls.sent.push(cmd); return Promise.resolve(reply); },
        downloads: {
          pause: (id) => { calls.paused.push(id); return Promise.resolve(); },
          cancel: (id) => { calls.canceled.push(id); return Promise.resolve(); },
          erase: (q) => { calls.erased.push(q); return Promise.resolve(); },
          resume: (id) => { calls.resumed.push(id); return Promise.resolve(); },
        },
        note: (text, url) => calls.notes.push({ text, url }),
        buildAdd: (ready) => ({ cmd: 'add', url: ready.url, filename: ready.filename, cookies: ready.cookies }),
      },
    };
  }
  const item = { id: 7, url: 'https://x/f.zip', filename: 'f.zip', referrer: 'https://x/', mime: '', fileSize: 5 * 1024 * 1024 };

  // decline path pinned: the browser download RESUMES
  const d1 = makeDeps({ ok: false, error: 'engine unreachable' });
  const r1 = await TK.handOff(item, d1.deps);
  ok('decline: the browser download resumes (pinned)',
    r1 === 'declined' && d1.calls.resumed.join() === '7' && d1.calls.canceled.length === 0);
  ok('decline: pause happened before the send', d1.calls.paused.join() === '7');
  ok('decline: the static note surfaced',
    d1.calls.notes.length === 1 && d1.calls.notes[0].text === TK.NOTE_UNREACHABLE && d1.calls.notes[0].url === item.url);
  ok('decline: the add command carried the jar', d1.calls.sent[0].cookies === 'a=b');

  // accept path: cancel + erase, no resume, no note
  const d2 = makeDeps({ ok: true, id: 3 });
  const r2 = await TK.handOff(item, d2.deps);
  ok('accept: browser download canceled and erased',
    r2 === 'taken' && d2.calls.canceled.join() === '7' && d2.calls.erased.length === 1);
  ok('accept: no resume, no note', d2.calls.resumed.length === 0 && d2.calls.notes.length === 0);

  // skipped path: nothing paused, sent, resumed or noted
  const d3 = makeDeps({ ok: true });
  const r3 = await TK.handOff({ id: 9, url: 'https://x/f.bin', filename: 'f.bin', fileSize: 1024 }, d3.deps);
  ok('non-takeover items are skipped untouched',
    r3 === 'skipped' && d3.calls.paused.length === 0 && d3.calls.sent.length === 0 && d3.calls.resumed.length === 0);

  // pause failure tolerated (the already-paused case)
  const d4 = makeDeps({ ok: false, error: 'boom' });
  d4.deps.downloads.pause = () => Promise.reject(new Error('already done'));
  const r4 = await TK.handOff(item, d4.deps);
  ok('pause failure tolerated (decline still resumes)', r4 === 'declined' && d4.calls.resumed.join() === '7');
}

console.log('detector B7 (DOM detection hardening)');
{
  function fakeEl(over) {
    return Object.assign({
      isConnected: true,
      readyState: 2,
      videoWidth: 0,
      networkState: 0,
      currentSrc: 'https://x/v.mp4',
      src: '',
      title: '',
      getBoundingClientRect() { return { width: 320, height: 180 }; },
      parentElement: null,
    }, over);
  }

  // geometry gating table (thresholds ours: 200x120 / 640x360)
  ok('geometry: 320x180 passes', DET.passesGeometry(fakeEl({})) === true);
  ok('geometry: 200x120 boundary passes', DET.passesGeometry(fakeEl({ getBoundingClientRect: () => ({ width: 200, height: 120 }) })) === true);
  ok('geometry: 199x120 fails (width gate)', DET.passesGeometry(fakeEl({ getBoundingClientRect: () => ({ width: 199, height: 120 }) })) === false);
  ok('geometry: 150x90 fails', DET.passesGeometry(fakeEl({ getBoundingClientRect: () => ({ width: 150, height: 90 }) })) === false);
  ok('geometry: clipped to zero fails', DET.passesGeometry(fakeEl({ getBoundingClientRect: () => ({ width: 0, height: 0 }) })) === false);
  ok('geometry: detached element fails', DET.passesGeometry(fakeEl({ isConnected: false })) === false);
  ok('prominence: 640x360 yes, 639x360 no',
    DET.isProminent(fakeEl({ getBoundingClientRect: () => ({ width: 640, height: 360 }) })) === true &&
    DET.isProminent(fakeEl({ getBoundingClientRect: () => ({ width: 639, height: 360 }) })) === false);

  // state gating table
  ok('state: readyState>=2 passes', DET.passesState(fakeEl({ readyState: 2 })) === true);
  ok('state: readyState 1 without dimensions fails',
    DET.passesState(fakeEl({ readyState: 1, videoWidth: 0, networkState: 2 })) === false);
  ok('state: loading with known dimensions passes',
    DET.passesState(fakeEl({ readyState: 0, videoWidth: 1280, networkState: 2 })) === true);
  ok('state: readyState dominates the loading rule',
    DET.passesState(fakeEl({ readyState: 4, videoWidth: 0, networkState: 0 })) === true);

  // overlay exclusion (display/visibility on the element chain)
  ok('overlay: display:none on the element excludes',
    DET.chainVisible(fakeEl({}), () => ({ display: 'none' })) === false);
  ok('overlay: visibility:hidden on an ancestor excludes',
    DET.chainVisible(fakeEl({ parentElement: fakeEl({}) }), (n) =>
      (n.parentElement ? { visibility: 'hidden' } : { display: 'block' })) === false);
  ok('overlay: clean chain passes',
    DET.chainVisible(fakeEl({ parentElement: fakeEl({}) }), () => ({ display: 'block' })) === true);

  // combined candidate
  ok('candidate: gates combine and carry the source',
    (() => { const c = DET.domCandidate(fakeEl({}), () => ({ display: 'block' })); return c.ok && c.url === 'https://x/v.mp4'; })());
  ok('candidate: hidden element rejected',
    DET.domCandidate(fakeEl({}), () => ({ visibility: 'hidden' })).ok === false);
  ok('candidate: prominence flag carries',
    DET.domCandidate(fakeEl({ getBoundingClientRect: () => ({ width: 640, height: 360 }) }), () => ({})).prominent === true);
  ok('candidate: <source> child fallback',
    (() => { const c = DET.domCandidate(fakeEl({ currentSrc: '', src: '', querySelector: () => ({ src: 'https://x/s.m3u8' }) }), () => ({})); return c.ok && c.url === 'https://x/s.m3u8'; })());

  // inline-script scan: first 128 KiB only
  {
    const beyond = 'x'.repeat(DET.INLINE_SCAN_CAP) + ' https://x/late.mp4 ';
    ok('inline scan: URL beyond the 128 KiB cap is not matched',
      DET.scanInlineScript(beyond).indexOf('https://x/late.mp4') === -1);
    const within = ' https://x/early.mp4 ' + 'y'.repeat(DET.INLINE_SCAN_CAP);
    ok('inline scan: URL inside the cap is matched',
      DET.scanInlineScript(within).indexOf('https://x/early.mp4') !== -1);
    ok('inline scan: non-media URLs are not matched',
      DET.scanInlineScript('https://x/page.html https://x/a.mp4').join() === 'https://x/a.mp4' &&
      DET.scanInlineScript('https://x/page.html').length === 0);
    ok('inline scan: .mp4x is not a media URL (letter boundary)',
      DET.scanInlineScript('https://x/a.mp4x').length === 0);
    ok('inline scan: same URL deduped',
      DET.scanInlineScript('https://x/a.mp4 https://x/a.mp4').length === 1);
    ok('inline scan: query strings tolerated',
      DET.scanInlineScript('https://x/a.mp4?sig=1').join() === 'https://x/a.mp4');
    ok('inline scan: empty/null tolerated',
      DET.scanInlineScript('').length === 0 && DET.scanInlineScript(null).length === 0);
  }

  // site packs: data-driven, OFF by default
  ok('packs: default settings load none', DET.loadPacks(FILTERS.mergeSettings({})).length === 0);
  ok('packs: absent settings load none', DET.loadPacks(null).length === 0);
  ok('packs: a real pack loads', DET.loadPacks({ sitePacks: [{ selectors: ['.x'], urlPatterns: ['cdn.example'] }] }).length === 1);
  ok('packs: junk entries filtered', DET.loadPacks({ sitePacks: [null, {}, 7, 'x'] }).length === 0);
  ok('pack match: url pattern substring', DET.packMatches({ urlPatterns: ['cdn.example'] }, 'https://cdn.example/v.mp4') === true);
  ok('pack match: non-matching url', DET.packMatches({ urlPatterns: ['cdn.example'] }, 'https://other/v.mp4') === false);
  ok('pack match: pattern-less pack never matches', DET.packMatches({}, 'https://x/') === false && DET.packMatches(null, 'https://x/') === false);

  // bounded detection window: observers, debounce, teardown across two windows
  function fakeObsCtor(instances) {
    return function (cb) {
      const inst = { cb, observed: [], disconnected: false };
      inst.observe = function (t) { inst.observed.push(t); };
      inst.disconnect = function () { inst.disconnected = true; };
      instances.push(inst);
      return inst;
    };
  }
  {
    const moInst = [];
    const roInst = [];
    const changes = [];
    const clk2 = fakeClock();
    const mk = () => DET.detectionWindow({
      MutationObserver: fakeObsCtor(moInst),
      ResizeObserver: fakeObsCtor(roInst),
      setTimeout: clk2.setTimeout,
      clearTimeout: clk2.clearTimeout,
      gather: () => [{ url: 'https://x/a.mp4' }],
      onChange: (vs) => changes.push(vs),
    });
    const w = mk();
    w.open();
    ok('window: both observers armed', moInst.length === 1 && roInst.length === 1 && w.isOpen());
    const videoRec = { addedNodes: [{ tagName: 'VIDEO' }], removedNodes: [] };
    const divRec = { addedNodes: [{ tagName: 'DIV' }], removedNodes: [] };
    moInst[0].cb([videoRec]);
    moInst[0].cb([videoRec]); // rapid double mutation
    clk2.advance(DET.OBSERVER_DEBOUNCE_MS);
    ok('window: media mutation refreshes (debounced to one)', changes.length === 1);
    moInst[0].cb([divRec]);
    clk2.advance(DET.OBSERVER_DEBOUNCE_MS);
    ok('window: non-media mutation ignored', changes.length === 1);
    w.close();
    ok('window: full teardown (both disconnected)', moInst[0].disconnected && roInst[0].disconnected && !w.isOpen());
    // a pending debounce is cancelled by close (no post-close refresh)
    const w2 = mk();
    w2.open();
    moInst[1].cb([videoRec]);
    w2.close();
    clk2.advance(DET.OBSERVER_DEBOUNCE_MS * 4);
    ok('window: close cancels the pending refresh', changes.length === 1);
    ok('window: second window arms fresh observers (no leak)',
      moInst.length === 2 && moInst[1] !== moInst[0] && moInst[0].disconnected === true);
    moInst[1].cb([videoRec]);
    clk2.advance(DET.OBSERVER_DEBOUNCE_MS);
    ok('window: the fresh window still works', changes.length === 2);
    w2.close();
    ok('window: second window torn down', moInst[1].disconnected && roInst[1].disconnected);
  }

  // overhead demonstration: a synthetic heavy DOM (5k candidates) pass
  {
    const els = [];
    for (let i = 0; i < 5000; i++) {
      els.push(fakeEl({
        currentSrc: 'https://x/v' + i + '.mp4',
        getBoundingClientRect() { return { width: 200 + (i % 60), height: 120 }; },
      }));
    }
    const t0 = process.hrtime.bigint();
    let pass = 0;
    for (const el of els) {
      if (DET.domCandidate(el, () => ({ display: 'block', visibility: 'visible' })).ok) pass++;
    }
    const ms = Number(process.hrtime.bigint() - t0) / 1e6;
    console.log('  overhead: 5000-candidate gated pass measured at ' + ms.toFixed(2) + ' ms');
    ok('overhead: 5000-candidate pass under the 250 ms budget', ms < 250 && pass === 5000);
  }
}

console.log('\n' + passed + ' assertions passed');
if (process.exitCode) console.error('FAILURES PRESENT');
