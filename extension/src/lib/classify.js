// SPDX-License-Identifier: GPL-3.0-or-later
// Pure classification logic for VelocityDM Companion: filename extraction from
// URLs and Content-Disposition headers, extension and mime category mapping,
// and the capture decision used by the service worker and the test suite.
// No browser or Node APIs are required by this module.

export const EXTENSION_CATEGORIES = Object.freeze({
  video: Object.freeze(["mp4", "mkv", "webm", "avi", "mov", "flv", "ts", "m4v", "3gp", "m3u8"]),
  audio: Object.freeze(["mp3", "m4a", "flac", "wav", "ogg", "opus", "aac"]),
  archive: Object.freeze(["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso"]),
  document: Object.freeze(["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "epub", "cbz"]),
  software: Object.freeze(["exe", "msi", "dmg", "pkg", "deb", "rpm", "apk", "appimage", "flatpak"]),
  "image-large": Object.freeze(["psd", "ai", "raw", "tiff"]),
});

const EXTENSION_LOOKUP = (() => {
  const map = new Map();
  for (const [category, extensions] of Object.entries(EXTENSION_CATEGORIES)) {
    for (const ext of extensions) map.set(ext, category);
  }
  return map;
})();

const MAX_NAME_LENGTH = 180;
const ILLEGAL_CHARACTERS = /[\\/:*?"<>|\u0000-\u001f\u007f]/g;
const BYTES_PER_MB = 1048576;

const HLS_MIME_TYPES = new Set(["application/vnd.apple.mpegurl", "application/x-mpegurl"]);

const MIME_ARCHIVE_MAP = new Map([
  ["application/zip", "archive"],
  ["application/x-zip-compressed", "archive"],
  ["application/x-7z-compressed", "archive"],
  ["application/x-rar-compressed", "archive"],
  ["application/vnd.rar", "archive"],
  ["application/x-tar", "archive"],
  ["application/gzip", "archive"],
  ["application/x-gzip", "archive"],
  ["application/x-bzip", "archive"],
  ["application/x-bzip2", "archive"],
  ["application/x-xz", "archive"],
  ["application/x-apple-diskimage", "archive"],
  ["application/x-iso9660-image", "archive"],
]);

const MIME_DOCUMENT_MAP = new Map([
  ["application/pdf", "document"],
  ["application/epub+zip", "document"],
  ["application/msword", "document"],
  ["application/vnd.ms-excel", "document"],
  ["application/vnd.ms-powerpoint", "document"],
  ["application/vnd.openxmlformats-officedocument.wordprocessingml.document", "document"],
  ["application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", "document"],
  ["application/vnd.openxmlformats-officedocument.presentationml.presentation", "document"],
]);

function hasExtension(name) {
  const dot = name.lastIndexOf(".");
  return dot > 0 && dot < name.length - 1;
}

// Maps a filename (or bare extension) to its category, or null when unknown.
export function extensionCategory(filename) {
  if (typeof filename !== "string") return null;
  const dot = filename.lastIndexOf(".");
  if (dot <= 0 || dot === filename.length - 1) return null;
  return EXTENSION_LOOKUP.get(filename.slice(dot + 1).toLowerCase()) || null;
}

export function isDownloadableExtension(filename) {
  return extensionCategory(filename) !== null;
}

// decodeURIComponent wrapper that returns the input unchanged on malformed
// sequences instead of throwing.
export function decodePercent(value) {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

// Shared filename sanitizer: replaces filesystem-illegal characters and control
// characters with "_", forbids "." and "..", and caps the length at 180
// characters while preserving the extension. Returns null for empty names.
export function sanitizeFilename(rawName, options = {}) {
  if (typeof rawName !== "string") return null;
  let name = rawName.replace(ILLEGAL_CHARACTERS, "_").trim();
  if (!name || name === "." || name === "..") return null;
  if (name.length > MAX_NAME_LENGTH) {
    const dot = name.lastIndexOf(".");
    const ext = dot > 0 ? name.slice(dot) : "";
    if (ext.length > 0 && ext.length < MAX_NAME_LENGTH) {
      name = name.slice(0, MAX_NAME_LENGTH - ext.length) + ext;
    } else {
      name = name.slice(0, MAX_NAME_LENGTH);
    }
  }
  if (!name || name === "." || name === "..") return null;
  if (options.requireExtension && !hasExtension(name)) return null;
  return name;
}

// Extracts a candidate filename from the last path segment of an http(s) URL.
// Returns null for non-http(s) schemes, empty or trailing-slash paths,
// extensionless names, and names that sanitize to nothing usable.
export function filenameFromUrl(url) {
  if (typeof url !== "string" || url.length === 0) return null;
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
  const segments = parsed.pathname.split("/");
  const last = segments[segments.length - 1] || "";
  if (!last) return null;
  return sanitizeFilename(decodePercent(last), { requireExtension: true });
}

// Splits a Content-Disposition header on semicolons while respecting
// quoted-string sections so that ";" inside quoted filenames is preserved.
function splitHeaderParts(header) {
  const parts = [];
  let current = "";
  let inQuotes = false;
  for (let i = 0; i < header.length; i += 1) {
    const ch = header[i];
    if (ch === '"') {
      inQuotes = !inQuotes;
      current += ch;
      continue;
    }
    if (ch === "\\" && inQuotes && i + 1 < header.length) {
      current += ch + header[i + 1];
      i += 1;
      continue;
    }
    if (ch === ";" && !inQuotes) {
      parts.push(current);
      current = "";
      continue;
    }
    current += ch;
  }
  parts.push(current);
  return parts;
}

// Parses a plain (possibly quoted) parameter value with escaped-character
// handling per RFC 2616 quoted-string rules.
function parsePlainValue(value) {
  if (value.startsWith('"')) {
    let out = "";
    for (let i = 1; i < value.length; i += 1) {
      const ch = value[i];
      if (ch === "\\" && i + 1 < value.length) {
        out += value[i + 1];
        i += 1;
        continue;
      }
      if (ch === '"') break;
      out += ch;
    }
    return out.trim();
  }
  return value.trim();
}

function percentToBytes(value) {
  const bytes = [];
  for (let i = 0; i < value.length; i += 1) {
    const ch = value[i];
    if (ch === "%") {
      const hex = value.slice(i + 1, i + 3);
      if (/^[0-9a-fA-F]{2}$/.test(hex)) {
        bytes.push(parseInt(hex, 16));
        i += 2;
        continue;
      }
      return null;
    }
    const code = ch.charCodeAt(0);
    if (code < 0x80) {
      bytes.push(code);
      continue;
    }
    for (const byte of new TextEncoder().encode(ch)) bytes.push(byte);
  }
  return Uint8Array.from(bytes);
}

// Parses an RFC 5987 ext-value: charset''percent-encoded-value.
function parseExtendedValue(value) {
  const firstApostrophe = value.indexOf("'");
  if (firstApostrophe === -1) return null;
  const secondApostrophe = value.indexOf("'", firstApostrophe + 1);
  if (secondApostrophe === -1) return null;
  const charset = value.slice(0, firstApostrophe).trim().toLowerCase() || "utf-8";
  const encoded = value.slice(secondApostrophe + 1);
  if (!encoded) return null;
  const bytes = percentToBytes(encoded);
  if (bytes === null) return null;
  try {
    if (charset === "iso-8859-1" || charset === "latin1") {
      let out = "";
      for (const byte of bytes) out += String.fromCharCode(byte);
      return out;
    }
    return new TextDecoder(charset).decode(bytes);
  } catch {
    return null;
  }
}

// Extracts a filename from a Content-Disposition header. filename* (RFC 5987)
// takes precedence over the plain filename parameter. Returns the sanitized
// name or null.
export function filenameFromDisposition(header) {
  if (typeof header !== "string" || header.length === 0) return null;
  let starValue = null;
  let plainValue = null;
  for (const part of splitHeaderParts(header)) {
    const eq = part.indexOf("=");
    if (eq <= 0) continue;
    const key = part.slice(0, eq).trim().toLowerCase();
    const value = part.slice(eq + 1).trim();
    if (key === "filename*") starValue = starValue || value;
    else if (key === "filename") plainValue = plainValue || value;
  }
  let name = null;
  if (starValue) name = parseExtendedValue(starValue);
  if (!name && plainValue) name = parsePlainValue(plainValue);
  if (!name) return null;
  return sanitizeFilename(name);
}

// Maps a mime type to a capture category. application/octet-stream is treated
// as unknown (null) and text/html is never a download (null).
export function mimeToCategory(mime) {
  if (typeof mime !== "string") return null;
  const base = mime.split(";", 1)[0].trim().toLowerCase();
  if (!base) return null;
  if (base.startsWith("video/")) return "video";
  if (base.startsWith("audio/")) return "audio";
  if (HLS_MIME_TYPES.has(base)) return "video";
  if (MIME_ARCHIVE_MAP.has(base)) return "archive";
  if (MIME_DOCUMENT_MAP.has(base)) return "document";
  return null;
}

function normalizeMinSizeMB(value) {
  const size = typeof value === "number" && Number.isFinite(value) ? value : 1;
  return size >= 0 ? size : 1;
}

// Capture decision. Input candidate: { url, mime, contentLength, referrer,
// method } (referrer is accepted for pipeline parity; the daemon receives it
// separately). Rules:
// - only http(s) GET requests are eligible (blob:, data:, javascript: rejected)
// - blocklist entries are substring-matched against the hostname
// - text/html is rejected unless the URL extension is downloadable
// - when content-length is known it must clear minSizeMB; when unknown the
//   decision falls back to the URL extension signal
// - the resolved category must be enabled in settings (defaults to "other")
export function decideCapture(candidate, settings) {
  const reject = (reason, category = null, filename = null) => ({
    capture: false,
    reason,
    category,
    filename,
  });
  const input = candidate && typeof candidate === "object" ? candidate : {};
  const config = settings && typeof settings === "object" ? settings : {};

  let parsed;
  try {
    parsed = new URL(input.url);
  } catch {
    return reject("scheme");
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return reject("scheme");

  const method = typeof input.method === "string" && input.method ? input.method.toUpperCase() : "GET";
  if (method !== "GET") return reject("method");

  const hostname = parsed.hostname.toLowerCase();
  if (Array.isArray(config.blocklist)) {
    for (const entry of config.blocklist) {
      if (typeof entry === "string" && entry && hostname.includes(entry.toLowerCase())) {
        return reject("blocklist");
      }
    }
  }

  const filename = filenameFromUrl(input.url);
  const extCategory = filename ? extensionCategory(filename) : null;
  const mimeBase = typeof input.mime === "string" ? input.mime.split(";", 1)[0].trim().toLowerCase() : "";
  const category = extCategory || mimeToCategory(input.mime);
  const resolvedCategory = category || "other";

  if (mimeBase === "text/html" && !extCategory) return reject("mime-html", extCategory, filename);

  const minBytes = normalizeMinSizeMB(config.minSizeMB) * BYTES_PER_MB;
  const length =
    typeof input.contentLength === "number" && Number.isFinite(input.contentLength) && input.contentLength >= 0
      ? input.contentLength
      : null;
  if (length !== null) {
    if (length < minBytes) return reject("size", extCategory, filename);
  } else if (!extCategory) {
    return reject("unknown-size", extCategory, filename);
  }

  const toggles = config.categories && typeof config.categories === "object" ? config.categories : {};
  if (toggles[resolvedCategory] !== true) return reject("category-disabled", extCategory, filename);

  return { capture: true, reason: "ok", category: resolvedCategory, filename };
}
