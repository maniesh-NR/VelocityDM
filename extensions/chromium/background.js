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
  'shared/state.js'
);

var DETECTOR = self.VDMDetector;
var FILTERS = self.VDMFilters;
var MSG = self.VDMMessaging;
var NMHOSTLIB = self.VDMNMHost;
var COOKIES = self.VDMCookies;

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
    pushVideo({
      url: url,
      title: DETECTOR.titleFor(url),
      quality: DETECTOR.qualityLabel(url, mime, {}),
      kind: DETECTOR.isHls(url, mime) ? 'hls' : (DETECTOR.isDash(url, mime) ? 'dash' : 'file'),
      pageUrl: details.initiator || details.documentUrl || '',
      size: len,
    }).catch(function () {});
  },
  { urls: ['<all_urls>'] },
  ['responseHeaders']
);

// ---------------------------------------------------------------------------
// download takeover
// ---------------------------------------------------------------------------

async function handOffDownload(item) {
  const settings = await getSettings();
  const filename = item.filename && item.filename.indexOf('/') !== -1
    ? item.filename.substring(item.filename.lastIndexOf('/') + 1)
    : item.filename;
  const probe = {
    url: item.url,
    filename: filename,
    mime: item.mime,
    fileSize: item.fileSize || item.totalBytes || 0,
    byExtensionId: item.byExtensionId,
  };
  if (!FILTERS.shouldTakeOver(probe, settings)) return;

  // pause the browser download while we hand it off
  try { await chrome.downloads.pause(item.id); } catch (e) { /* already done */ }

  let cookies = '';
  try {
    const all = await chrome.cookies.getAll({ url: item.url });
    cookies = all.map(function (c) { return c.name + '=' + c.value; }).join('; ');
  } catch (e) { /* cookies permission may be restricted */ }

  const command = MSG.add({
    url: item.url,
    filename: filename,
    referer: item.referrer || '',
    cookies: cookies,
  });

  hostSend(command).then(function (normalized) {
    if (normalized.ok) {
      // VelocityDM owns it now: drop the browser download
      chrome.downloads.cancel(item.id).catch(function () {});
      chrome.downloads.erase({ id: item.id }).catch(function () {});
    } else {
      // host unreachable or declined: let the browser finish the download
      chrome.downloads.resume(item.id).catch(function () {});
    }
  });
}

chrome.downloads.onCreated.addListener(function (item) {
  handOffDownload(item).catch(function () {});
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
    addWithJar({ url: info.srcUrl || info.linkUrl, referer: pageUrl });
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
      case 'clearVideos':
        await chrome.storage.session.remove(VIDEOS_KEY);
        sendResponse({ ok: true, videos: [] });
        break;
      case 'getSettings':
        sendResponse({ ok: true, settings: await getSettings() });
        break;
      case 'saveSettings':
        await chrome.storage.local.set({ settings: msg.settings });
        sendResponse({ ok: true });
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
