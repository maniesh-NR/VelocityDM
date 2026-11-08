// SPDX-License-Identifier: GPL-3.0-or-later
// VelocityDM Companion service worker (Manifest V3, ES module).
//
// Responsibilities:
// - context menu entry ("Download with VelocityDM") and the manual add pipeline
// - traffic sniffing via non-blocking webRequest observation
// - a single long-lived native messaging port to the VelocityDM desktop daemon
// - badge counter plus an in-memory capture list persisted to
//   chrome.storage.session (in-memory fallback when session storage is absent)
// - a small message bus shared by the popup and the options page

import { decideCapture, filenameFromDisposition } from "./lib/classify.js";
import { FrameDecoder, makeHello, makeRpc, parseIncoming } from "./lib/protocol.js";
import { SettingsStore } from "./lib/settings.js";
import { DedupeRing } from "./lib/dedupe.js";

const NMH_NAME = "com.velocitydm.nmh";
const MENU_ID = "vdm-download";
const KEEPALIVE_ALARM = "vdm-keepalive";
const CAPTURE_LIMIT = 100;
const CANDIDATE_LIMIT = 1000;
const BADGE_COLOR = "#4285F4";
const BADGE_MAX = 99;
const NOTIFY_ICON = "src/icons/icon-128.png";
const RPC_TIMEOUT_MS = 10000;
const RECONNECT_MS = 5000;

const settingsStore = new SettingsStore();
let dedupe = new DedupeRing(500);
const frameDecoder = new FrameDecoder();

let captures = [];
let capturesLoaded = false;
let captureSeq = 0;

let nativePort = null;
let nativeConnected = false;
let daemonVersion = null;
let reconnectTimer = null;
let rpcSeq = 1;
const pendingRpc = new Map();
const recentCandidates = new Map();

// --- context menu ---------------------------------------------------------

async function ensureContextMenu() {
  const settings = await settingsStore.get();
  await new Promise((resolve) => chrome.contextMenus.removeAll(() => resolve()));
  if (!settings.contextMenu) return;
  chrome.contextMenus.create(
    {
      id: MENU_ID,
      title: "Download with VelocityDM",
      contexts: ["link", "video", "audio", "selection"],
    },
    () => void chrome.runtime.lastError
  );
}

function pickMenuUrl(info) {
  const candidates = [info.mediaURL, info.linkURL, info.srcUrl];
  for (const candidate of candidates) {
    if (typeof candidate === "string" && candidate.length > 0) return candidate;
  }
  const selection = info.selectionText;
  if (typeof selection === "string" && /^https?:\/\/\S+$/i.test(selection.trim())) {
    return selection.trim();
  }
  return null;
}

async function handleMenuClick(info, tab) {
  const url = pickMenuUrl(info);
  if (!url) {
    await notify("VelocityDM", "Select a link, video, or audio element with a downloadable URL.", true);
    return;
  }
  const referrer = (tab && tab.url) || info.pageUrl || null;
  const result = await addUrl(url, { referrer });
  if (!result.ok) await notify("VelocityDM", result.error, true);
}

// --- notifications --------------------------------------------------------

// force bypasses the user-facing notify toggle; used for connect failures so
// that a broken setup is always reported to the user.
async function notify(title, message, force = false) {
  try {
    if (!force) {
      const settings = await settingsStore.get();
      if (!settings.notify) return;
    }
    chrome.notifications.create({
      type: "basic",
      iconUrl: NOTIFY_ICON,
      title,
      message: message || "",
    });
  } catch {
    // Notification failures must never break the capture pipeline.
  }
}

// --- native messaging port manager ---------------------------------------

function sendNative(message) {
  if (!nativePort || !nativeConnected) return false;
  try {
    nativePort.postMessage(message);
    return true;
  } catch {
    return false;
  }
}

function connectNative() {
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  if (nativePort && nativeConnected) return nativePort;
  if (nativePort) {
    try {
      nativePort.disconnect();
    } catch {
      // Already disconnected.
    }
    nativePort = null;
  }
  nativeConnected = false;
  try {
    nativePort = chrome.runtime.connectNative(NMH_NAME);
  } catch {
    nativePort = null;
    scheduleReconnect();
    return null;
  }
  nativePort.onMessage.addListener(onNativeMessage);
  nativePort.onDisconnect.addListener(() => {
    void chrome.runtime.lastError; // consume so the console stays clean
    nativePort = null;
    nativeConnected = false;
    flushPendingRpc("VelocityDM daemon is not running");
    scheduleReconnect();
  });
  nativeConnected = true;
  sendNative(makeHello());
  return nativePort;
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    settingsStore
      .get()
      .then((settings) => {
        if (settings.sniffEnabled || settings.contextMenu || settings.notify) connectNative();
      })
      .catch(() => {});
  }, RECONNECT_MS);
}

function flushPendingRpc(message) {
  for (const entry of pendingRpc.values()) {
    clearTimeout(entry.timer);
    entry.reject(new Error(message));
  }
  pendingRpc.clear();
}

function callDaemon(method, params, timeoutMs = RPC_TIMEOUT_MS) {
  const id = rpcSeq++;
  const message = makeRpc(id, method, params);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pendingRpc.delete(id);
      reject(new Error("VelocityDM daemon did not respond in time"));
    }, timeoutMs);
    pendingRpc.set(id, { resolve, reject, timer });
    if (!sendNative(message)) {
      clearTimeout(timer);
      pendingRpc.delete(id);
      reject(new Error("VelocityDM daemon is not running"));
    }
  });
}

function onNativeMessage(message) {
  let frames;
  if (message instanceof ArrayBuffer) {
    frames = frameDecoder.push(new Uint8Array(message));
  } else if (ArrayBuffer.isView(message)) {
    frames = frameDecoder.push(new Uint8Array(message.buffer, message.byteOffset, message.byteLength));
  } else {
    frames = [message];
  }
  for (const frame of frames) handleDaemonFrame(frame);
}

function handleDaemonFrame(frame) {
  const normalized = parseIncoming(frame);
  if (normalized.kind === "hello") {
    if (normalized.version) daemonVersion = normalized.version;
    return;
  }
  if (normalized.kind === "rpcResult") {
    const entry = pendingRpc.get(normalized.id);
    if (!entry) return;
    pendingRpc.delete(normalized.id);
    clearTimeout(entry.timer);
    if (normalized.ok) entry.resolve(normalized.result);
    else entry.reject(new Error(normalized.error));
    return;
  }
  if (normalized.kind === "event") handleDaemonEvent(normalized.event, normalized.params);
  // Unknown frames are ignored.
}

function handleDaemonEvent(event, params) {
  const detail = params && typeof params === "object" ? params : {};
  const label = String(detail.filename || detail.url || "Download");
  if (event === "completed") {
    notify("Download completed", label);
  } else if (event === "failed") {
    notify("Download failed", String(detail.error || label));
  }
  // "progress" events are intentionally ignored for the badge; the badge is
  // driven by locally captured downloads only.
}

// --- download pipeline ----------------------------------------------------

function isHttpUrl(url) {
  return typeof url === "string" && /^https?:\/\//i.test(url.trim());
}

function isDaemonUrl(url, settings) {
  try {
    const parsed = new URL(url);
    return parsed.hostname === settings.host && parsed.port === String(settings.port);
  } catch {
    return false;
  }
}

// Manual adds are explicit user intent, so they bypass the capture gates and
// the dedupe ring; only the scheme is validated here.
async function addUrl(url, opts = {}) {
  if (!isHttpUrl(url)) {
    return { ok: false, error: "Only http and https URLs can be queued." };
  }
  const cleanUrl = url.trim();
  const port = connectNative();
  if (!port) {
    return { ok: false, error: "VelocityDM daemon is not running", connected: false };
  }
  const params = {
    url: cleanUrl,
    referrer: typeof opts.referrer === "string" ? opts.referrer : null,
    filename: typeof opts.filename === "string" ? opts.filename : null,
  };
  try {
    const result = await callDaemon("add", params);
    return { ok: true, result };
  } catch (error) {
    const message = error && error.message ? error.message : "VelocityDM daemon is not running";
    return { ok: false, error: message, connected: Boolean(nativeConnected) };
  }
}

async function testConnection() {
  connectNative();
  if (!nativePort) return { ok: false, error: "VelocityDM daemon is not running" };
  try {
    const result = await callDaemon("hello", {}, RPC_TIMEOUT_MS);
    const version = daemonVersion || (result && result.version) || null;
    return { ok: true, version };
  } catch (error) {
    if (!nativeConnected) return { ok: false, error: "VelocityDM daemon is not running" };
    return { ok: false, error: error && error.message ? error.message : "unknown error" };
  }
}

// --- sniffing -------------------------------------------------------------

const WEB_FILTER = { urls: ["<all_urls>"], types: ["media", "xmlhttprequest", "object"] };

function recordCandidate(details) {
  if (!details.url || !/^https?:/i.test(details.url)) return;
  settingsStore
    .get()
    .then((settings) => {
      if (!settings.sniffEnabled) return;
      if (isDaemonUrl(details.url, settings)) return;
      recentCandidates.set(details.url, { method: details.method || "GET", time: Date.now() });
      if (recentCandidates.size > CANDIDATE_LIMIT) {
        const oldest = recentCandidates.keys().next().value;
        recentCandidates.delete(oldest);
      }
    })
    .catch(() => {});
}

function observeHeaders(details) {
  handleObservation(details).catch(() => {});
}

async function handleObservation(details) {
  const settings = await settingsStore.get();
  if (!settings.sniffEnabled) return;
  const url = details.url || "";
  if (!/^https?:/i.test(url)) return;
  if (isDaemonUrl(url, settings)) return;

  const headers = Array.isArray(details.responseHeaders) ? details.responseHeaders : [];
  const findHeader = (name) => {
    const hit = headers.find((h) => h && typeof h.name === "string" && h.name.toLowerCase() === name);
    return hit && hit.value != null ? String(hit.value) : null;
  };
  const mime = findHeader("content-type");
  const rawLength = findHeader("content-length");
  const contentLength = rawLength && /^\d+$/.test(rawLength.trim()) ? parseInt(rawLength.trim(), 10) : null;
  const disposition = findHeader("content-disposition");

  const decision = decideCapture(
    {
      url,
      mime,
      contentLength,
      referrer: details.initiator || details.originUrl || null,
      method: details.method || "GET",
    },
    settings
  );
  if (!decision.capture) return;

  let filename = decision.filename;
  const fromDisposition = filenameFromDisposition(disposition);
  if (fromDisposition) filename = fromDisposition;
  if (!dedupe.see(url)) return;

  captureSeq += 1;
  captures.unshift({
    id: "cap-" + Date.now() + "-" + captureSeq,
    url,
    filename: filename || url,
    category: decision.category,
    size: contentLength,
    mime,
    time: Date.now(),
  });
  if (captures.length > CAPTURE_LIMIT) captures.length = CAPTURE_LIMIT;
  await saveCaptures();
  updateBadge();
}

// --- capture persistence and badge ---------------------------------------

async function loadCaptures() {
  capturesLoaded = true;
  try {
    if (chrome.storage && chrome.storage.session) {
      const stored = await chrome.storage.session.get("vdmCaptures");
      const list = stored && stored.vdmCaptures;
      if (Array.isArray(list)) {
        captures = list.filter((item) => item && typeof item.url === "string").slice(0, CAPTURE_LIMIT);
      }
      return;
    }
  } catch {
    // Session storage unavailable or failed; fall back to in-memory only.
  }
}

async function saveCaptures() {
  try {
    if (chrome.storage && chrome.storage.session) {
      await chrome.storage.session.set({ vdmCaptures: captures });
      return true;
    }
  } catch {
    // Fall back to in-memory only.
  }
  return false;
}

async function ensureCapturesLoaded() {
  if (capturesLoaded) return;
  await loadCaptures();
}

function updateBadge() {
  const count = captures.length;
  const text = count === 0 ? "" : count > BADGE_MAX ? BADGE_MAX + "+" : String(count);
  try {
    chrome.action.setBadgeText({ text });
  } catch {
    // Badge updates are best-effort.
  }
}

// --- keepalive ------------------------------------------------------------

function ensureKeepaliveAlarm() {
  try {
    chrome.alarms.get(KEEPALIVE_ALARM, (existing) => {
      if (existing || chrome.runtime.lastError) return;
      chrome.alarms.create(KEEPALIVE_ALARM, { delayInMinutes: 4, periodInMinutes: 4 });
    });
  } catch {
    // Alarms are unavailable in some embedded environments.
  }
}

// --- message bus ----------------------------------------------------------

async function handleMessage(message) {
  const type = message && message.type;
  switch (type) {
    case "getState": {
      const settings = await settingsStore.get();
      return { ok: true, connected: nativeConnected, version: daemonVersion, settings };
    }
    case "getCaptures":
      await ensureCapturesLoaded();
      return { ok: true, captures };
    case "clearCaptures":
      captures = [];
      dedupe = new DedupeRing(500);
      await saveCaptures();
      updateBadge();
      return { ok: true, captures };
    case "addUrl":
      return addUrl(message.url, message.opts && typeof message.opts === "object" ? message.opts : {});
    case "testConnection":
      return testConnection();
    default:
      return { ok: false, error: "Unknown message type: " + String(type) };
  }
}

// --- event wiring (registered synchronously at the top level) -------------

chrome.runtime.onInstalled.addListener(() => {
  (async () => {
    chrome.action.setBadgeBackgroundColor({ color: BADGE_COLOR });
    await ensureContextMenu();
    ensureKeepaliveAlarm();
    await loadCaptures();
    updateBadge();
  })().catch(() => {});
});

chrome.runtime.onStartup.addListener(() => {
  (async () => {
    chrome.action.setBadgeBackgroundColor({ color: BADGE_COLOR });
    ensureKeepaliveAlarm();
    await loadCaptures();
    updateBadge();
  })().catch(() => {});
});

chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local" || !changes.settings) return;
  settingsStore.invalidate();
  ensureContextMenu().catch(() => {});
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  handleMenuClick(info, tab).catch(() => {});
});

chrome.webRequest.onBeforeRequest.addListener(recordCandidate, WEB_FILTER, []);
chrome.webRequest.onHeadersReceived.addListener(observeHeaders, WEB_FILTER, ["responseHeaders"]);

chrome.alarms.onAlarm.addListener((alarm) => {
  if (!alarm || alarm.name !== KEEPALIVE_ALARM) return;
  settingsStore
    .get()
    .then((settings) => {
      if (!settings.sniffEnabled) return;
      if (!nativeConnected) connectNative();
    })
    .catch(() => {});
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  handleMessage(message)
    .then((response) => sendResponse(response))
    .catch((error) => sendResponse({ ok: false, error: String((error && error.message) || error) }));
  return true; // keep the channel open for the async response
});
