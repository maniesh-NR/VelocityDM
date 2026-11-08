// SPDX-License-Identifier: GPL-3.0-or-later
import assert from "node:assert/strict";
import {
  EXTENSION_CATEGORIES,
  isDownloadableExtension,
  filenameFromUrl,
  filenameFromDisposition,
  mimeToCategory,
  decideCapture,
} from "../src/lib/classify.js";

const MB = 1048576;

const baseSettings = {
  host: "127.0.0.1",
  port: 36990,
  minSizeMB: 1,
  blocklist: [],
  categories: {
    video: true,
    audio: true,
    archive: true,
    document: true,
    software: true,
    "image-large": false,
    other: false,
  },
};

export default function register(test) {
  // --- filenameFromUrl -----------------------------------------------------

  test("filenameFromUrl plain path", () => {
    assert.equal(filenameFromUrl("https://example.com/files/video.mp4"), "video.mp4");
  });

  test("filenameFromUrl ignores query and fragment", () => {
    assert.equal(filenameFromUrl("https://example.com/file.zip?dl=1&token=abc#frag"), "file.zip");
  });

  test("filenameFromUrl decodes percent-encoded names", () => {
    assert.equal(filenameFromUrl("https://example.com/My%20Video.mp4"), "My Video.mp4");
  });

  test("filenameFromUrl trailing slash is null", () => {
    assert.equal(filenameFromUrl("https://example.com/dir/"), null);
  });

  test("filenameFromUrl non-http scheme is null", () => {
    assert.equal(filenameFromUrl("ftp://example.com/file.zip"), null);
    assert.equal(filenameFromUrl("blob:https://example.com/1234-5678"), null);
  });

  test("filenameFromUrl sanitizes traversal segments", () => {
    assert.equal(filenameFromUrl("https://example.com/files/%2e%2e%2fsecret.zip"), ".._secret.zip");
  });

  test("filenameFromUrl replaces illegal characters", () => {
    assert.equal(filenameFromUrl("https://example.com/my*file|name.mp4"), "my_file_name.mp4");
    assert.equal(filenameFromUrl("https://example.com/down%3Aload%7Cname.mp4"), "down_load_name.mp4");
  });

  test("filenameFromUrl truncates long names but keeps the extension", () => {
    const name = filenameFromUrl("https://example.com/" + "a".repeat(200) + ".zip");
    assert.ok(name);
    assert.equal(name.length, 180);
    assert.ok(name.endsWith(".zip"));
  });

  test("filenameFromUrl rejects dotfiles without extension", () => {
    assert.equal(filenameFromUrl("https://example.com/.hidden"), null);
  });

  test("filenameFromUrl rejects extensionless and empty paths", () => {
    assert.equal(filenameFromUrl("https://example.com/file"), null);
    assert.equal(filenameFromUrl("https://example.com/"), null);
    assert.equal(filenameFromUrl("not a url"), null);
  });

  // --- filenameFromDisposition ----------------------------------------------

  test("filenameFromDisposition quoted string", () => {
    assert.equal(filenameFromDisposition('attachment; filename="my file.zip"'), "my file.zip");
  });

  test("filenameFromDisposition rfc5987 utf-8 value", () => {
    assert.equal(filenameFromDisposition("attachment; filename*=UTF-8''caf%C3%A9%20menu.pdf"), "caf\u00e9 menu.pdf");
  });

  test("filenameFromDisposition unescapes quoted-string and sanitizes quotes", () => {
    assert.equal(filenameFromDisposition('attachment; filename="a \\"quote\\".zip"'), "a _quote_.zip");
  });

  test("filenameFromDisposition filename* wins over filename", () => {
    assert.equal(
      filenameFromDisposition('attachment; filename="fallback.zip"; filename*=UTF-8\'\'win.zip'),
      "win.zip"
    );
  });

  test("filenameFromDisposition unquoted value", () => {
    assert.equal(filenameFromDisposition("attachment; filename=archive.tar.gz"), "archive.tar.gz");
  });

  test("filenameFromDisposition garbage is null", () => {
    assert.equal(filenameFromDisposition("inline"), null);
    assert.equal(filenameFromDisposition("attachment; filename="), null);
    assert.equal(filenameFromDisposition(null), null);
    assert.equal(filenameFromDisposition(""), null);
  });

  // --- mimeToCategory ---------------------------------------------------------

  test("mimeToCategory video", () => {
    assert.equal(mimeToCategory("video/mp4"), "video");
    assert.equal(mimeToCategory("Video/WEBM; codecs=vp9"), "video");
  });

  test("mimeToCategory audio", () => {
    assert.equal(mimeToCategory("audio/ogg"), "audio");
  });

  test("mimeToCategory hls playlists map to video", () => {
    assert.equal(mimeToCategory("application/vnd.apple.mpegurl"), "video");
    assert.equal(mimeToCategory("application/x-mpegurl"), "video");
  });

  test("mimeToCategory archive suffix map", () => {
    assert.equal(mimeToCategory("application/zip"), "archive");
    assert.equal(mimeToCategory("application/x-7z-compressed"), "archive");
    assert.equal(mimeToCategory("application/gzip"), "archive");
  });

  test("mimeToCategory pdf is document", () => {
    assert.equal(mimeToCategory("application/pdf"), "document");
  });

  test("mimeToCategory octet-stream is unknown", () => {
    assert.equal(mimeToCategory("application/octet-stream"), null);
  });

  test("mimeToCategory html is never a download", () => {
    assert.equal(mimeToCategory("text/html; charset=utf-8"), null);
  });

  test("mimeToCategory garbage is null", () => {
    assert.equal(mimeToCategory(""), null);
    assert.equal(mimeToCategory(null), null);
    assert.equal(mimeToCategory("gibberish"), null);
  });

  // --- isDownloadableExtension ------------------------------------------------

  test("isDownloadableExtension covers every category", () => {
    let checked = 0;
    for (const [category, extensions] of Object.entries(EXTENSION_CATEGORIES)) {
      assert.ok(extensions.length > 0, category);
      for (const ext of extensions) {
        assert.equal(isDownloadableExtension("file." + ext), true, ext);
        checked += 1;
      }
    }
    assert.ok(checked >= 40);
  });

  test("isDownloadableExtension is case-insensitive", () => {
    assert.equal(isDownloadableExtension("MOVIE.MP4"), true);
    assert.equal(isDownloadableExtension("Setup.EXE"), true);
  });

  test("isDownloadableExtension rejects non-downloadable names", () => {
    for (const ext of ["html", "htm", "js", "css", "txt", "json", "svg"]) {
      assert.equal(isDownloadableExtension("page." + ext), false, ext);
    }
    assert.equal(isDownloadableExtension("noextension"), false);
    assert.equal(isDownloadableExtension(""), false);
  });

  // --- decideCapture -----------------------------------------------------------

  test("decideCapture accepts a normal mp4", () => {
    const d = decideCapture(
      { url: "https://cdn.example.com/a/movie.mp4", mime: "video/mp4", contentLength: 10 * MB, method: "GET" },
      baseSettings
    );
    assert.deepEqual(
      { capture: d.capture, category: d.category, reason: d.reason },
      { capture: true, category: "video", reason: "ok" }
    );
    assert.equal(d.filename, "movie.mp4");
  });

  test("decideCapture captures hls playlists when size is known", () => {
    const d = decideCapture(
      {
        url: "https://stream.example.com/live/index.m3u8",
        mime: "application/vnd.apple.mpegurl",
        contentLength: 2 * MB,
        method: "GET",
      },
      baseSettings
    );
    assert.equal(d.capture, true);
    assert.equal(d.category, "video");
  });

  test("decideCapture enforces the minimum size when length is known", () => {
    const d = decideCapture(
      { url: "https://example.com/small.zip", mime: "application/zip", contentLength: 500 * 1024, method: "GET" },
      baseSettings
    );
    assert.equal(d.capture, false);
    assert.equal(d.reason, "size");
  });

  test("decideCapture unknown size falls back to the extension signal", () => {
    const ok = decideCapture(
      { url: "https://example.com/setup.exe", mime: "application/octet-stream", contentLength: null, method: "GET" },
      baseSettings
    );
    assert.equal(ok.capture, true);
    assert.equal(ok.category, "software");
    const bad = decideCapture(
      {
        url: "https://example.com/stream/playlist",
        mime: "application/vnd.apple.mpegurl",
        contentLength: null,
        method: "GET",
      },
      baseSettings
    );
    assert.equal(bad.capture, false);
    assert.equal(bad.reason, "unknown-size");
  });

  test("decideCapture rejects text/html without a downloadable extension", () => {
    const d = decideCapture(
      { url: "https://example.com/watch", mime: "text/html; charset=utf-8", contentLength: 4096, method: "GET" },
      baseSettings
    );
    assert.equal(d.capture, false);
    assert.equal(d.reason, "mime-html");
  });

  test("decideCapture allows text/html when the extension is downloadable", () => {
    const d = decideCapture(
      { url: "https://example.com/files/report.pdf", mime: "text/html", contentLength: 5 * MB, method: "GET" },
      baseSettings
    );
    assert.equal(d.capture, true);
    assert.equal(d.category, "document");
  });

  test("decideCapture rejects blob, javascript, and data urls", () => {
    for (const url of [
      "blob:https://example.com/abcd-1234",
      "javascript:alert(1)",
      "data:application/zip;base64,AAAA",
    ]) {
      const d = decideCapture({ url, mime: null, contentLength: null, method: "GET" }, baseSettings);
      assert.equal(d.capture, false, url);
      assert.equal(d.reason, "scheme", url);
    }
  });

  test("decideCapture honors the blocklist", () => {
    const settings = { ...baseSettings, blocklist: ["ads.example.com"] };
    const d = decideCapture(
      {
        url: "https://cdn.ads.example.com/file.zip",
        mime: "application/zip",
        contentLength: 5 * MB,
        method: "GET",
      },
      settings
    );
    assert.equal(d.capture, false);
    assert.equal(d.reason, "blocklist");
  });

  test("decideCapture honors disabled categories", () => {
    const settings = { ...baseSettings, categories: { ...baseSettings.categories, video: false } };
    const d = decideCapture(
      { url: "https://example.com/clip.mp4", mime: "video/mp4", contentLength: 5 * MB, method: "GET" },
      settings
    );
    assert.equal(d.capture, false);
    assert.equal(d.reason, "category-disabled");
  });

  test("decideCapture leaves image-large disabled by default", () => {
    const d = decideCapture(
      {
        url: "https://example.com/artwork.psd",
        mime: "application/octet-stream",
        contentLength: 50 * MB,
        method: "GET",
      },
      baseSettings
    );
    assert.equal(d.capture, false);
    assert.equal(d.reason, "category-disabled");
    assert.equal(d.category, "image-large");
  });

  test("decideCapture rejects non-GET methods", () => {
    const d = decideCapture(
      { url: "https://example.com/file.zip", mime: "application/zip", contentLength: 5 * MB, method: "POST" },
      baseSettings
    );
    assert.equal(d.capture, false);
    assert.equal(d.reason, "method");
  });

  test("decideCapture rejects invalid urls", () => {
    const d = decideCapture({ url: "not a url", mime: null, contentLength: null, method: "GET" }, baseSettings);
    assert.equal(d.capture, false);
    assert.equal(d.reason, "scheme");
  });
}
