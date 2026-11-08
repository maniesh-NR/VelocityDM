// SPDX-License-Identifier: GPL-3.0-or-later
import assert from "node:assert/strict";
import {
  encodeFrame,
  FrameDecoder,
  makeHello,
  makeRpc,
  parseIncoming,
  MAX_FRAME_BYTES,
  EXTENSION_VERSION,
} from "../src/lib/protocol.js";

export default function register(test) {
  test("encodeFrame and FrameDecoder roundtrip", () => {
    const msg = { type: "rpc", id: 7, method: "add", params: { url: "https://example.com/a.zip" } };
    const frame = encodeFrame(msg);
    assert.ok(frame.length > 4);
    const decoder = new FrameDecoder();
    const messages = decoder.push(frame);
    assert.deepEqual(messages, [msg]);
  });

  test("frame length prefix is little-endian u32", () => {
    const frame = encodeFrame({ type: "hello" });
    const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
    const length = view.getUint32(0, true);
    assert.equal(length, frame.length - 4);
    assert.equal(new TextDecoder().decode(frame.subarray(4)), '{"type":"hello"}');
  });

  test("decoder handles frames split mid-header", () => {
    const decoder = new FrameDecoder();
    const frame = encodeFrame({ type: "event", event: "completed", params: { id: 1 } });
    let out = [];
    out = out.concat(decoder.push(frame.subarray(0, 2)));
    assert.equal(out.length, 0);
    out = out.concat(decoder.push(frame.subarray(2, 3)));
    assert.equal(out.length, 0);
    out = out.concat(decoder.push(frame.subarray(3)));
    assert.equal(out.length, 1);
    assert.equal(out[0].event, "completed");
  });

  test("decoder handles frames split mid-payload", () => {
    const decoder = new FrameDecoder();
    const frame = encodeFrame({ type: "rpcResult", id: 3, ok: true, result: { n: 42 } });
    const mid = 4 + Math.floor((frame.length - 4) / 2);
    assert.equal(decoder.push(frame.subarray(0, mid)).length, 0);
    const out = decoder.push(frame.subarray(mid));
    assert.equal(out.length, 1);
    assert.deepEqual(out[0].result, { n: 42 });
  });

  test("decoder handles two frames in one push", () => {
    const decoder = new FrameDecoder();
    const a = encodeFrame({ type: "hello", version: "1.0.0" });
    const b = encodeFrame({ type: "event", event: "progress", params: {} });
    const merged = new Uint8Array(a.length + b.length);
    merged.set(a, 0);
    merged.set(b, a.length);
    const out = decoder.push(merged);
    assert.equal(out.length, 2);
    assert.equal(out[0].type, "hello");
    assert.equal(out[1].event, "progress");
  });

  test("decoder skips malformed JSON payloads", () => {
    const decoder = new FrameDecoder();
    const bad = new Uint8Array(4 + 7);
    new DataView(bad.buffer).setUint32(0, 7, true);
    bad.set(new TextEncoder().encode("{not js"), 4);
    const good = encodeFrame({ type: "hello", version: "9" });
    const merged = new Uint8Array(bad.length + good.length);
    merged.set(bad, 0);
    merged.set(good, bad.length);
    const out = decoder.push(merged);
    assert.equal(out.length, 1);
    assert.equal(out[0].version, "9");
  });

  test("encoder and decoder enforce the frame limit", () => {
    assert.throws(() => encodeFrame({ blob: "x".repeat(1024) }, 64));
    const decoder = new FrameDecoder(64);
    const hostile = new Uint8Array(4);
    new DataView(hostile.buffer).setUint32(0, 4096, true);
    assert.throws(() => decoder.push(hostile));
  });

  test("makeHello shape", () => {
    const hello = makeHello();
    assert.equal(hello.type, "hello");
    assert.equal(hello.version, EXTENSION_VERSION);
    assert.equal(hello.protocol, 1);
    assert.equal(hello.client, "velocitydm-companion");
  });

  test("makeRpc shape and validation", () => {
    const rpc = makeRpc(1, "add", { url: "https://example.com/x" });
    assert.deepEqual(rpc, { type: "rpc", id: 1, method: "add", params: { url: "https://example.com/x" } });
    assert.equal(makeRpc(2, "list").params, null);
    assert.throws(() => makeRpc(-1, "add"));
    assert.throws(() => makeRpc(1.5, "add"));
    assert.throws(() => makeRpc(1, ""));
    assert.throws(() => makeRpc(1, null));
  });

  test("parseIncoming normalizes hello frames", () => {
    const n = parseIncoming({ type: "hello", version: "1.0.0", daemon: "running" });
    assert.equal(n.kind, "hello");
    assert.equal(n.version, "1.0.0");
    assert.equal(n.daemon, "running");
  });

  test("parseIncoming normalizes rpcResult frames", () => {
    const ok = parseIncoming({ type: "rpcResult", id: 5, ok: true, result: { id: 1 } });
    assert.equal(ok.kind, "rpcResult");
    assert.equal(ok.ok, true);
    assert.deepEqual(ok.result, { id: 1 });
    const err = parseIncoming({ type: "rpcResult", id: 6, ok: false, error: { code: -32000, message: "boom" } });
    assert.equal(err.ok, false);
    assert.deepEqual(err.error, { code: -32000, message: "boom" });
  });

  test("parseIncoming normalizes event frames and aliases", () => {
    const a = parseIncoming({ type: "event", event: "completed", params: { id: 1 } });
    assert.equal(a.kind, "event");
    assert.equal(a.event, "completed");
    const b = parseIncoming({ type: "notification", name: "failed", data: { id: 2 } });
    assert.equal(b.kind, "event");
    assert.equal(b.event, "failed");
    assert.deepEqual(b.params, { id: 2 });
  });

  test("parseIncoming reports unknown frames instead of throwing", () => {
    assert.equal(parseIncoming(null).kind, "unknown");
    assert.equal(parseIncoming([1, 2]).kind, "unknown");
    assert.equal(parseIncoming({ type: "mystery" }).kind, "unknown");
  });

  test("MAX_FRAME_BYTES matches the Chromium limit", () => {
    assert.equal(MAX_FRAME_BYTES, 64 * 1024 * 1024);
  });
}
