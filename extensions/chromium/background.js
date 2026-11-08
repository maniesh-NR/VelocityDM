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
  'shared/state.js'
);

var DETECTOR = self.VDMDetector;
var FILTERS = self.VDMFilters;
var MSG = self.VDMMessaging;

var NATIVE_HOST = 'com.velocitydm.nmh';
var VIDEOS_KEY = 'vdm_recent_videos';
var VIDEO_CAP = 50;

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

  chrome.runtime.sendNativeMessage(NATIVE_HOST, command, function (reply) {
    const normalized = MSG.normalize(reply);
    if (normalized.ok) {
      // VelocityDM owns it now: drop the browser download
      chrome.downloads.cancel(item.id).catch(function () {});
      chrome.downloads.erase({ id: item.id }).catch(function () {});
    } else {
      // host unreachable: let the browser finish the download
      chrome.downloads.resume(item.id).catch(function () {});
    }
  });
}

chrome.downloads.onCreated.addListener(function (item) {
  handOffDownload(item).catch(function () {});
});

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
    sendNative(MSG.add({ url: info.linkUrl, referer: pageUrl }));
  } else if (info.menuItemId === 'vdm-image' && info.srcUrl) {
    sendNative(MSG.add({ url: info.srcUrl, referer: pageUrl }));
  } else if (info.menuItemId === 'vdm-video' && (info.srcUrl || info.linkUrl)) {
    sendNative(MSG.add({ url: info.srcUrl || info.linkUrl, referer: pageUrl }));
  } else if (info.menuItemId === 'vdm-magnet') {
    const mag = selectedMagnet(info);
    if (mag) sendNative(MSG.addTorrent(mag));
  } else if (info.menuItemId === 'vdm-all' && tab && tab.id != null) {
    chrome.tabs.sendMessage(tab.id, { type: 'grabLinks' }, function (groups) {
      if (chrome.runtime.lastError || !groups) return;
      const urls = self.VDMLinks.flatten(groups);
      if (urls.length) sendNative(MSG.addBatch(urls));
    });
  }
});

function sendNative(command) {
  chrome.runtime.sendNativeMessage(NATIVE_HOST, command, function (reply) {
    const normalized = MSG.normalize(reply);
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
        chrome.runtime.sendNativeMessage(NATIVE_HOST, MSG.ping(), function (reply) {
          sendResponse(MSG.normalize(reply));
        });
        break;
      case 'addDownload':
        sendNative(MSG.add(msg.item || {}));
        sendResponse({ ok: true });
        break;
      case 'addBatch':
        sendNative(MSG.addBatch(msg.pattern || msg.urls || []));
        sendResponse({ ok: true });
        break;
      case 'addTorrent':
        sendNative(MSG.addTorrent(msg.magnet || msg.path || ''));
        sendResponse({ ok: true });
        break;
      case 'getDownloads':
        chrome.runtime.sendNativeMessage(NATIVE_HOST, MSG.status(), function (reply) {
          sendResponse(MSG.normalize(reply));
        });
        break;
      default:
        sendResponse({ ok: false, error: 'unknown message ' + (msg && msg.type) });
    }
  })();
  return true; // async sendResponse
});
