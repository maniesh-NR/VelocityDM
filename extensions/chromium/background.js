/**
 * VelocityDM background service worker (Chromium MV3).
 * - sniffs media requests (m3u8/mpd/video) into a recent-videos list
 * - takes browser downloads over via the native messaging host
 * - context menu: download link/image/video/all links with VelocityDM
 */
/* global chrome, importScripts, self */

importScripts(
  'shared/detector.js',
  'shared/patterns.js',
  'shared/links.js',
  'shared/filters.js',
  'shared/messaging.js',
  'shared/nmhost.js',
  'shared/cookies.js',
  'shared/takeover.js',
  'shared/mime-table.js',
  'shared/state.js'
);

var DETECTOR = self.VDMDetector;
var FILTERS = self.VDMFilters;
var MSG = self.VDMMessaging;
var NMHOSTLIB = self.VDMNMHost;
var COOKIES = self.VDMCookies;
var VDMTAKEOVER = self.VDMTakeover;
// S11-i B8-js: the generated MIME->extension table (Rust is the source of
// truth; scripts/gen-mime-table.py emits this UMD copy)
var MIMETABLE = self.VDMMimeTable;
var EXT_FOR_MIME = MIMETABLE ? MIMETABLE.extensionForMime : null;

var NATIVE_HOST = 'com.velocitydm.nmh';
var VIDEOS_KEY = 'vdm_recent_videos';
var VIDEO_CAP = 50;

// The native-host link (S11-i B4): persistent port preferred, one-shot
// fallback; bounded backoff reconnect; a bounded command queue. The link
// is created once per service-worker lifetime; a connected port keeps the
// worker alive on current Chrome.
var HOST = NMHOSTLIB.createLink({
  hostName: NATIVE_HOST,
  connectNative: (typeof chrome !== 'undefined' && chrome.runtime && chrome.runtime.connectNative)
    ? function (name) { return chrome.runtime.connectNative(name); }
    : undefined,
  sendNativeMessage: function (name, command, cb) {
    chrome.runtime.sendNativeMessage(name, command, cb);
  },
  log: function (m) { console.warn('VelocityDM:', m); },
});

/** Send through the link; always resolves to a normalized reply object. */
function hostSend(command) {
  return HOST.send(command).then(function (r) { return r; }, function (e) {
    return (e && typeof e === 'object' && e.error) ? e : { ok: false, error: 'engine unreachable' };
  });
}

// ---------------------------------------------------------------------------
// settings
// ---------------------------------------------------------------------------

async function getSettings() {
  const stored = await chrome.storage.local.get('settings');
  return FILTERS.mergeSettings(stored.settings);
}

// ---------------------------------------------------------------------------
// sniffed videos (storage.session survives service worker restarts)
// ---------------------------------------------------------------------------

async function pushVideo(entry) {
  const data = await chrome.storage.session.get(VIDEOS_KEY);
  let items = data[VIDEOS_KEY] || [];
  items = items.filter(function (e) { return e.url !== entry.url; });
  entry.seenAt = Date.now();
  items.unshift(entry);
  if (items.length > VIDEO_CAP) items.length = VIDEO_CAP;
  await chrome.storage.session.set({ [VIDEOS_KEY]: items });
}

async function getVideos() {
  const data = await chrome.storage.session.get(VIDEOS_KEY);
  return data[VIDEOS_KEY] || [];
}

// ---------------------------------------------------------------------------
// network sniffing
// ---------------------------------------------------------------------------

chrome.webRequest.onResponseHeaders.addListener(
  function (details) {
    // media elements, XHR playlists, and direct video requests
    if (details.type === 'main_frame' || details.type === 'sub_frame') return;
    const headers = {};
    (details.responseHeaders || []).forEach(function (h) {
      headers[h.name.toLowerCase()] = h.value;
    });
    const url = details.url;
    const mime = headers['content-type'] || '';
    const len = parseInt(headers['content-length'] || '0', 10) || 0;
    if (!url || url.startsWith('blob:') || url.startsWith('data:')) return;
    if (!DETECTOR.isVideo(url, mime, len)) return;
    // S11-i B8-js: a filename hint from the shared MIME table when the URL
    // yields no extension — never overrides an existing one
    const hint = filenameHint(url, mime);
    pushVideo({
      url: url,
      title: DETECTOR.titleFor(url),
      quality: DETECTOR.qualityLabel(url, mime, {}),
      kind: DETECTOR.isHls(url, mime) ? 'hls' : (DETECTOR.isDash(url, mime) ? 'dash' : 'file'),
      pageUrl: details.initiator || details.documentUrl || '',
      size: len,
      filename: hint,
    }).catch(function () {});
  },
  { urls: ['<all_urls>'] },
  ['responseHeaders']
);

/** The sniffed-list filename hint: only when the URL path has no
 *  extension AND the table resolves the content type. */
function filenameHint(url, mime) {
  if (DETECTOR.extOf(url)) return '';
  if (!EXT_FOR_MIME || !mime) return '';
  const base = DETECTOR.titleFor(url);
  if (!base || base.indexOf('.') !== -1) return '';
  let ext = null;
  try { ext = EXT_FOR_MIME(mime); } catch (e) { ext = null; }
  return ext ? base + '.' + ext : '';
}

// ---------------------------------------------------------------------------
// download takeover (S11-i B6: shelf, filename participation, decline note)
// ---------------------------------------------------------------------------

// download ids we are actively taking over (filename participation)
var takeoverPendingIds = new Set();
var TAKEOVER_NOTE_KEY = 'vdm_takeover_note';

function storeTakeoverNote(text, url) {
  chrome.storage.session.set({
    [TAKEOVER_NOTE_KEY]: { text: text, url: url || '', at: Date.now() },
  }).catch(function () {});
}

async function handOffDownload(item) {
  if (item && typeof item.id === 'number') {
    // marked before any await so the filename listener sees it
    takeoverPendingIds.add(item.id);
  }
  var result;
  try {
    result = await VDMTAKEOVER.handOff(item, {
      getSettings: getSettings,
      shouldTakeOver: FILTERS.shouldTakeOver,
      withJar: COOKIES.withJar,
      jarApi: JAR_API,
      send: hostSend,
      buildAdd: function (ready) { return MSG.add(ready); },
      downloads: {
        pause: function (id) { return chrome.downloads.pause(id); },
        cancel: function (id) { return chrome.downloads.cancel(id); },
        erase: function (q) { return chrome.downloads.erase(q); },
        resume: function (id) { return chrome.downloads.resume(id); },
      },
      note: storeTakeoverNote,
      extForMime: EXT_FOR_MIME,
    });
  } catch (e) { /* never break the pipeline on a takeover error */ }
  if (item && typeof item.id === 'number') takeoverPendingIds.delete(item.id);
  return result;
}

chrome.downloads.onCreated.addListener(function (item) {
  handOffDownload(item).catch(function () {});
});

// Shelf suppression (Chromium-only, feature-detected; Firefox no-ops):
// while takeover is enabled in settings the shelf is off for ALL
// downloads (the documented trade-off); disabling any intercept category
// turns it back on.
function applyShelf() {
  var api = (typeof chrome !== 'undefined' && chrome.downloads &&
    typeof chrome.downloads.setShelfEnabled === 'function')
    ? chrome.downloads.setShelfEnabled.bind(chrome.downloads)
    : null;
  if (!api) return; // Firefox has no shelf API — no-op by design
  getSettings().then(function (settings) {
    api(VDMTAKEOVER.isEnabled(settings)).catch(function () {});
  }).catch(function () {});
}

// Filename participation: suggest our filename ONCE, on our takeover
// (the browser item matches the engine's naming); downloads we are not
// taking over are untouched (never fighting a user rename).
chrome.downloads.onDeterminingFilename.addListener(function (item, suggest) {
  if (!item || !takeoverPendingIds.has(item.id)) return;
  takeoverPendingIds.delete(item.id);
  var name = VDMTAKEOVER.filenameFor(item, EXT_FOR_MIME);
  if (name) suggest({ filename: name, conflictAction: 'uniquify' });
});

// ---------------------------------------------------------------------------
// cookie jar (S11-i B5): every add path with an http(s) URL forwards the
// jar — context-menu link/image/video adds, popup manual adds, batch adds
// (per-URL, capped at the first 32 URLs) and the takeover path (unchanged).
// The jar itself is never logged.
// ---------------------------------------------------------------------------

var JAR_API = (typeof chrome !== 'undefined' && chrome.cookies) ? chrome.cookies : null;

function addWithJar(item) {
  return COOKIES.withJar(item, JAR_API).then(function (ready) {
    return sendNative(MSG.add(ready));
  });
}

// Content-script video takeover (S11-i B6): the add rides the jar like
// every other path, and on SUCCESS the current tab navigates to the
// static confirmation page (taken.html — hash read once, textContent
// only, immediate history.back). The downloads-pipeline path never
// navigates; a decline surfaces the static note to the popup.
function addVideoWithConfirm(url, referer, tab) {
  COOKIES.withJar({ url: url, referer: referer }, JAR_API).then(function (ready) {
    return hostSend(MSG.add(ready));
  }).then(function (normalized) {
    if (normalized.ok) {
      if (tab && tab.id != null) {
        var target = VDMTAKEOVER.confirmTarget(
          url, chrome.runtime.getURL('taken.html'));
        chrome.tabs.update(tab.id, { url: target }).catch(function () {});
      }
    } else {
      storeTakeoverNote(VDMTAKEOVER.noteFor(normalized), url);
    }
  }).catch(function () {});
}

// ---------------------------------------------------------------------------
// context menus
// ---------------------------------------------------------------------------

chrome.runtime.onInstalled.addListener(function () {
  chrome.contextMenus.removeAll(function () {
    chrome.contextMenus.create({
      id: 'vdm-link',
      title: 'Download with VelocityDM',
      contexts: ['link'],
    });
    chrome.contextMenus.create({
      id: 'vdm-image',
      title: 'Download image with VelocityDM',
      contexts: ['image'],
    });
    chrome.contextMenus.create({
      id: 'vdm-video',
      title: 'Download video with VelocityDM',
      contexts: ['video', 'audio'],
    });
    chrome.contextMenus.create({
      id: 'vdm-all',
      title: 'Download all links on this page with VelocityDM',
      contexts: ['page', 'selection'],
    });
    chrome.contextMenus.create({
      id: 'vdm-magnet',
      title: 'Download with VelocityDM',
      contexts: ['selection'],
    });
  });
});

function selectedMagnet(info) {
  const text = (info.selectionText || '').trim();
  return text && text.toLowerCase().startsWith('magnet:') ? text : null;
}

chrome.contextMenus.onClicked.addListener(function (info, tab) {
  const pageUrl = tab && tab.url ? tab.url : '';
  if (info.menuItemId === 'vdm-link' && info.linkUrl) {
    addWithJar({ url: info.linkUrl, referer: pageUrl });
  } else if (info.menuItemId === 'vdm-image' && info.srcUrl) {
    addWithJar({ url: info.srcUrl, referer: pageUrl });
  } else if (info.menuItemId === 'vdm-video' && (info.srcUrl || info.linkUrl)) {
    // content-script video takeover: confirm-in-tab on success (B6)
    addVideoWithConfirm(info.srcUrl || info.linkUrl, pageUrl, tab);
  } else if (info.menuItemId === 'vdm-magnet') {
    const mag = selectedMagnet(info);
    if (mag) sendNative(MSG.addTorrent(mag));
  } else if (info.menuItemId === 'vdm-all' && tab && tab.id != null) {
    chrome.tabs.sendMessage(tab.id, { type: 'grabLinks' }, function (groups) {
      if (chrome.runtime.lastError || !groups) return;
      const urls = self.VDMLinks.flatten(groups);
      if (urls.length) sendBatchWithJars(urls);
    });
  }
});

function sendBatchWithJars(urls) {
  COOKIES.jarsForBatch(urls, JAR_API).then(function (r) {
    sendNative(MSG.addBatch(urls, r.map));
  });
}

function sendNative(command) {
  hostSend(command).then(function (normalized) {
    if (!normalized.ok) {
      console.warn('VelocityDM host:', normalized.error);
    }
  });
}

// ---------------------------------------------------------------------------
// messages from content script / popup / options
// ---------------------------------------------------------------------------

chrome.runtime.onMessage.addListener(function (msg, sender, sendResponse) {
  (async function () {
    switch (msg && msg.type) {
      case 'getVideos':
        sendResponse({ ok: true, videos: await getVideos() });
        break;
      case 'domVideos':
        // S11-i B7: the content script's bounded detection window pushes
        // DOM candidates; they merge into the same list the popup reads
        if (sender && Array.isArray(msg.videos)) {
          for (const v of msg.videos.slice(0, VIDEO_CAP)) {
            if (v && v.url && /^https?:/i.test(v.url)) {
              await pushVideo({
                url: v.url,
                title: v.title || DETECTOR.titleFor(v.url),
                quality: v.quality || '',
                kind: v.kind || 'dom',
                pageUrl: v.pageUrl || '',
                size: 0,
              });
            }
          }
        }
        sendResponse({ ok: true });
        break;
      case 'clearVideos':
        await chrome.storage.session.remove(VIDEOS_KEY);
        sendResponse({ ok: true, videos: [] });
        break;
      case 'getSettings':
        sendResponse({ ok: true, settings: await getSettings() });
        break;
      case 'saveSettings':
        await chrome.storage.local.set({ settings: msg.settings });
        applyShelf(); // takeover enablement may have changed (B6)
        sendResponse({ ok: true });
        break;
      case 'hostState':
        // the popup surfaces 'engine unreachable' while the link is down
        sendResponse({ ok: true, state: HOST.state() });
        break;
      case 'getTakeoverNote':
        chrome.storage.session.get(TAKEOVER_NOTE_KEY).then(function (data) {
          sendResponse({ ok: true, note: data[TAKEOVER_NOTE_KEY] || null });
        });
        break;
      case 'clearTakeoverNote':
        chrome.storage.session.remove(TAKEOVER_NOTE_KEY).then(function () {
          sendResponse({ ok: true, note: null });
        });
        break;
      case 'nativePing':
        hostSend(MSG.ping()).then(function (normalized) {
          sendResponse(normalized);
        });
        break;
      case 'addDownload':
        // popup manual add: forward the jar for http(s) URLs (B5)
        addWithJar(msg.item || {});
        sendResponse({ ok: true });
        break;
      case 'addBatch':
        if (Array.isArray(msg.urls)) {
          sendBatchWithJars(msg.urls);
        } else {
          sendNative(MSG.addBatch(msg.pattern || []));
        }
        sendResponse({ ok: true });
        break;
      case 'addTorrent':
        sendNative(MSG.addTorrent(msg.magnet || msg.path || ''));
        sendResponse({ ok: true });
        break;
      case 'getDownloads':
        hostSend(MSG.status()).then(function (normalized) {
          sendResponse(normalized);
        });
        break;
      default:
        sendResponse({ ok: false, error: 'unknown message ' + (msg && msg.type) });
    }
  })();
  return true; // async sendResponse
});

// startup: align the shelf with the current takeover settings (B6)
applyShelf();
