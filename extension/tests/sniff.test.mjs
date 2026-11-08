// SPDX-License-Identifier: GPL-3.0-or-later
// Integration-style sniffing scenarios: header sets as delivered by
// webRequest.onHeadersReceived flow through the same pure pipeline the
// service worker uses (classify + settings + dedupe).
import assert from "node:assert/strict";
import { decideCapture, filenameFromDisposition } from "../src/lib/classify.js";
import { mergeSettings } from "../src/lib/settings.js";
import { DedupeRing } from "../src/lib/dedupe.js";

const MB = 1048576;

function headersToMap(pairs) {
  const find = (name) => {
    const hit = pairs.find((p) => p.name.toLowerCase() === name);
    return hit ? hit.value : null;
  };
  return find;
}

function observe(find, url, method = "GET", settings = null) {
  const mime = find("content-type");
  const rawLength = find("content-length");
  const contentLength = rawLength && /^\d+$/.test(rawLength.trim()) ? parseInt(rawLength.trim(), 10) : null;
  const disposition = find("content-disposition");
  const decision = decideCapture({ url, mime, contentLength, referrer: "https://page.example.com/", method }, settings || mergeSettings({}));
  let filename = decision.filename;
  const fromDisposition = filenameFromDisposition(disposition);
  if (fromDisposition) filename = fromDisposition;
  return { decision, filename };
}

export default function register(test) {
  test("video response with content-length is captured", () => {
    const find = headersToMap([
      { name: "Content-Type", value: "video/mp4" },
      { name: "Content-Length", value: String(64 * MB) },
    ]);
    const { decision, filename } = observe(find, "https://cdn.example.com/movie.mp4");
    assert.equal(decision.capture, true);
    assert.equal(decision.category, "video");
    assert.equal(filename, "movie.mp4");
  });

  test("disposition overrides the URL filename", () => {
    const find = headersToMap([
      { name: "Content-Type", value: "application/zip" },
      { name: "Content-Length", value: String(5 * MB) },
      { name: "Content-Disposition", value: 'attachment; filename="release bundle.zip"' },
    ]);
    const { decision, filename } = observe(find, "https://example.com/d/12345");
    assert.equal(decision.capture, true);
    assert.equal(filename, "release bundle.zip");
  });

  test("m3u8 playlist without content-length but with extension is captured", () => {
    const find = headersToMap([{ name: "Content-Type", value: "application/vnd.apple.mpegurl" }]);
    const { decision } = observe(find, "https://stream.example.com/vod/index.m3u8");
    assert.equal(decision.capture, true);
    assert.equal(decision.category, "video");
  });

  test("small archive below the size gate is skipped", () => {
    const find = headersToMap([
      { name: "Content-Type", value: "application/zip" },
      { name: "Content-Length", value: String(400 * 1024) },
    ]);
    const { decision } = observe(find, "https://example.com/tiny.zip");
    assert.equal(decision.capture, false);
    assert.equal(decision.reason, "size");
  });

  test("html document responses are ignored", () => {
    const find = headersToMap([
      { name: "Content-Type", value: "text/html; charset=utf-8" },
      { name: "Content-Length", value: String(120 * 1024) },
    ]);
    const { decision } = observe(find, "https://example.com/watch/stream");
    assert.equal(decision.capture, false);
    assert.equal(decision.reason, "mime-html");
  });

  test("page navigation (document type requests) would be ignored by the worker filter", () => {
    // The background registers webRequest filters for media/xmlhttprequest/object
    // types only; document requests never reach handleObservation. This test
    // pins the decision for a document-like GET in case the filter changes.
    const find = headersToMap([
      { name: "Content-Type", value: "text/html" },
      { name: "Content-Length", value: String(2 * MB) },
    ]);
    const { decision } = observe(find, "https://example.com/page.html", "GET");
    assert.equal(decision.capture, false);
  });

  test("dedupe prevents duplicate captures across observations", () => {
    const ring = new DedupeRing(500);
    const url = "https://cdn.example.com/video.webm";
    assert.equal(ring.see(url), true);
    assert.equal(ring.see(url), false);
  });

  test("settings merge feeds decideCapture end to end", () => {
    const settings = mergeSettings({ minSizeMB: "10", blocklist: ["slowcdn"] });
    const big = observe(
      headersToMap([{ name: "Content-Type", value: "video/webm" }, { name: "Content-Length", value: String(50 * MB) }]),
      "https://fast.example.com/a.webm",
      "GET",
      settings
    );
    assert.equal(big.decision.capture, true);
    const small = observe(
      headersToMap([{ name: "Content-Type", value: "video/webm" }, { name: "Content-Length", value: String(5 * MB) }]),
      "https://fast.example.com/b.webm",
      "GET",
      settings
    );
    assert.equal(small.decision.capture, false);
    const blocked = observe(
      headersToMap([{ name: "Content-Type", value: "video/webm" }, { name: "Content-Length", value: String(50 * MB) }]),
      "https://slowcdn.example.net/c.webm",
      "GET",
      settings
    );
    assert.equal(blocked.decision.capture, false);
    assert.equal(blocked.decision.reason, "blocklist");
  });

  test("worker observation rejects the daemon endpoint itself", () => {
    // Mirrors isDaemonUrl logic in background.js: requests to the daemon's own
    // host:port must never be captured even when they look like downloads.
    const settings = mergeSettings({});
    assert.equal(settings.port, 36990);
    const parsed = new URL("http://127.0.0.1:36990/add");
    const isDaemon = parsed.hostname === settings.host && parsed.port === String(settings.port);
    assert.equal(isDaemon, true);
    const other = new URL("http://127.0.0.1:36991/add");
    const isOther = other.hostname === settings.host && other.port === String(settings.port);
    assert.equal(isOther, false, "the SSE port is not the NMH endpoint");
  });
}
