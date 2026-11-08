// SPDX-License-Identifier: GPL-3.0-or-later
// Native messaging frame and message helpers for VelocityDM Companion.
// Chromium native messaging uses a little-endian u32 length prefix followed by
// UTF-8 JSON. These helpers are pure: no browser APIs are required, so they
// are unit-testable under bun and reusable inside the daemon test rig.

export const MAX_FRAME_BYTES = 64 * 1024 * 1024; // 64 MiB, matches Chromium
export const EXTENSION_VERSION = "1.0.0";

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

function limitError(maxBytes) {
  return new Error("velocitydm: frame exceeds the " + maxBytes + " byte frame limit");
}

// Serializes an object into one length-prefixed frame. Throws when the encoded
// payload would exceed the frame limit.
export function encodeFrame(obj, maxBytes = MAX_FRAME_BYTES) {
  const json = JSON.stringify(obj);
  const limit = maxBytes - 4;
  if (limit < 0 || json.length > limit) throw limitError(maxBytes);
  const payload = textEncoder.encode(json);
  if (payload.length > limit) throw limitError(maxBytes);
  const frame = new Uint8Array(4 + payload.length);
  new DataView(frame.buffer).setUint32(0, payload.length, true);
  frame.set(payload, 4);
  return frame;
}

function toUint8Array(bytes) {
  if (bytes instanceof Uint8Array) return bytes;
  if (bytes instanceof ArrayBuffer) return new Uint8Array(bytes);
  if (ArrayBuffer.isView(bytes)) {
    return new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }
  if (Array.isArray(bytes)) return Uint8Array.from(bytes);
  throw new TypeError("velocitydm: expected bytes (Uint8Array, ArrayBuffer, DataView, or Array)");
}

// Incremental frame decoder: accepts arbitrary byte chunks (including frames
// split mid-header or mid-payload) and returns every complete message.
// Malformed JSON payloads are skipped without corrupting the stream.
export class FrameDecoder {
  constructor(maxBytes = MAX_FRAME_BYTES) {
    this.maxBytes = maxBytes;
    this.buffer = new Uint8Array(0);
  }

  push(bytes) {
    const incoming = toUint8Array(bytes);
    if (this.buffer.length === 0) {
      this.buffer = incoming;
    } else {
      const merged = new Uint8Array(this.buffer.length + incoming.length);
      merged.set(this.buffer, 0);
      merged.set(incoming, this.buffer.length);
      this.buffer = merged;
    }
    const messages = [];
    while (this.buffer.length >= 4) {
      const view = new DataView(this.buffer.buffer, this.buffer.byteOffset, this.buffer.byteLength);
      const frameLength = view.getUint32(0, true);
      if (frameLength > this.maxBytes) {
        throw limitError(this.maxBytes);
      }
      if (this.buffer.length < 4 + frameLength) break;
      const payload = this.buffer.subarray(4, 4 + frameLength);
      let parsed = null;
      try {
        parsed = JSON.parse(textDecoder.decode(payload));
      } catch {
        parsed = null;
      }
      this.buffer = this.buffer.slice(4 + frameLength);
      if (parsed !== null) messages.push(parsed);
    }
    return messages;
  }
}

// First frame the extension sends after connecting to the daemon.
export function makeHello() {
  return { type: "hello", version: EXTENSION_VERSION, client: "velocitydm-companion", protocol: 1 };
}

// Request frame for the extension-to-daemon RPC channel.
export function makeRpc(id, method, params) {
  if (!Number.isInteger(id) || id < 0) {
    throw new TypeError("velocitydm: rpc id must be a non-negative integer");
  }
  if (typeof method !== "string" || method.length === 0) {
    throw new TypeError("velocitydm: rpc method must be a non-empty string");
  }
  return { type: "rpc", id, method, params: params === undefined ? null : params };
}

// Normalizes daemon frames into the shapes the extension consumes:
//   { kind: "hello", version, daemon }
//   { kind: "rpcResult", id, ok, result | error }
//   { kind: "event", event, params }
// Anything unrecognized is reported as kind "unknown" instead of throwing.
export function parseIncoming(obj) {
  if (!obj || typeof obj !== "object" || Array.isArray(obj)) {
    return { kind: "unknown", raw: obj };
  }
  switch (obj.type) {
    case "hello":
      return {
        kind: "hello",
        version: typeof obj.version === "string" ? obj.version : null,
        daemon: typeof obj.daemon === "string" ? obj.daemon : null,
        raw: obj,
      };
    case "rpcResult":
    case "rpc-result":
    case "response":
      return {
        kind: "rpcResult",
        id: obj.id,
        ok: Boolean(obj.ok),
        result: obj.ok ? obj.result ?? null : null,
        error: obj.ok ? null : obj.error ?? "daemon error",
        raw: obj,
      };
    case "event":
    case "notification":
      return {
        kind: "event",
        event: typeof obj.event === "string" ? obj.event : typeof obj.name === "string" ? obj.name : null,
        params: obj.params ?? obj.data ?? {},
        raw: obj,
      };
    default:
      return { kind: "unknown", raw: obj };
  }
}
