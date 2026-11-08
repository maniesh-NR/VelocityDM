// SPDX-License-Identifier: GPL-3.0-or-later
// Settings core (pure) plus a chrome.storage.local wrapper for VelocityDM
// Companion. mergeSettings doubles as the migration step: stored values are
// type-checked, coerced where safe, and unknown keys are pruned, so settings
// written by older or newer builds always converge on the current shape.

export const DEFAULTS = Object.freeze({
  host: "127.0.0.1",
  port: 36990,
  minSizeMB: 1,
  sniffEnabled: true,
  contextMenu: true,
  notify: true,
  blocklist: [],
  categories: Object.freeze({
    video: true,
    audio: true,
    archive: true,
    document: true,
    software: true,
    "image-large": false,
    other: false,
  }),
  manualAlwaysAsk: false,
});

const BOOLEAN_KEYS = ["sniffEnabled", "contextMenu", "notify", "manualAlwaysAsk"];
const BLOCKLIST_LIMIT = 200;

function toNumber(value) {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() !== "") {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return null;
}

function cloneDefaults() {
  return JSON.parse(JSON.stringify(DEFAULTS));
}

// Deep-merges stored settings over the defaults with strict type checks,
// safe numeric coercion, unknown-key pruning, and blocklist sanitization
// (strings only, trimmed, lowercased, deduplicated, capped at 200 entries).
export function mergeSettings(stored) {
  const out = cloneDefaults();
  if (!stored || typeof stored !== "object" || Array.isArray(stored)) return out;

  if (typeof stored.host === "string") {
    const host = stored.host.trim();
    if (host && !/\s/.test(host)) out.host = host;
  }
  if (stored.port !== undefined) {
    const port = toNumber(stored.port);
    if (port !== null && Number.isInteger(port) && port >= 1 && port <= 65535) out.port = port;
  }
  if (stored.minSizeMB !== undefined) {
    const minSize = toNumber(stored.minSizeMB);
    if (minSize !== null && minSize >= 0 && minSize <= 102400) out.minSizeMB = minSize;
  }
  for (const key of BOOLEAN_KEYS) {
    if (typeof stored[key] === "boolean") out[key] = stored[key];
  }
  if (Array.isArray(stored.blocklist)) {
    const cleaned = [];
    const seen = new Set();
    for (const entry of stored.blocklist) {
      if (typeof entry !== "string") continue;
      const host = entry.trim().toLowerCase();
      if (!host || seen.has(host)) continue;
      seen.add(host);
      cleaned.push(host);
      if (cleaned.length >= BLOCKLIST_LIMIT) break;
    }
    out.blocklist = cleaned;
  }
  if (stored.categories && typeof stored.categories === "object" && !Array.isArray(stored.categories)) {
    for (const key of Object.keys(out.categories)) {
      if (typeof stored.categories[key] === "boolean") out.categories[key] = stored.categories[key];
    }
  }
  return out;
}

function defaultStorage() {
  if (typeof chrome !== "undefined" && chrome.storage && chrome.storage.local) {
    return chrome.storage.local;
  }
  return null;
}

// Thin wrapper over chrome.storage.local with a cached read. The storage area
// is injectable so tests can supply a plain object with get/set callbacks.
export class SettingsStore {
  constructor(storageArea = defaultStorage()) {
    this.key = "settings";
    this.storage = storageArea;
    this.cache = null;
  }

  async get(force = false) {
    if (!force && this.cache) return this.cache;
    let stored = {};
    if (this.storage) {
      const raw = await new Promise((resolve) => {
        try {
          this.storage.get(this.key, (result) => resolve(result));
        } catch {
          resolve(undefined);
        }
      });
      const value = raw && typeof raw === "object" ? raw[this.key] : undefined;
      if (value && typeof value === "object" && !Array.isArray(value)) stored = value;
    }
    this.cache = mergeSettings(stored);
    return this.cache;
  }

  async set(patch) {
    const current = await this.get();
    const next = mergeSettings({ ...current, ...(patch && typeof patch === "object" ? patch : {}) });
    this.cache = next;
    if (this.storage) {
      await new Promise((resolve) => {
        try {
          this.storage.set({ [this.key]: next }, () => resolve());
        } catch {
          resolve();
        }
      });
    }
    return next;
  }

  invalidate() {
    this.cache = null;
  }
}
