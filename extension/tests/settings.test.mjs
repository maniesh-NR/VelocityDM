// SPDX-License-Identifier: GPL-3.0-or-later
import assert from "node:assert/strict";
import { DEFAULTS, mergeSettings, SettingsStore } from "../src/lib/settings.js";

export default function register(test) {
  test("mergeSettings returns defaults for empty input", () => {
    for (const input of [null, undefined, "text", 42, [], {}]) {
      const out = mergeSettings(input);
      assert.deepEqual(out, JSON.parse(JSON.stringify(DEFAULTS)));
    }
  });

  test("mergeSettings prunes unknown keys", () => {
    const out = mergeSettings({ host: "0.0.0.0", mystery: true, nested: { a: 1 } });
    assert.equal(out.host, "0.0.0.0");
    assert.equal("mystery" in out, false);
    assert.equal("nested" in out, false);
  });

  test("mergeSettings validates host", () => {
    assert.equal(mergeSettings({ host: "  localhost  " }).host, "localhost");
    assert.equal(mergeSettings({ host: "bad host" }).host, DEFAULTS.host);
    assert.equal(mergeSettings({ host: "" }).host, DEFAULTS.host);
    assert.equal(mergeSettings({ host: 123 }).host, DEFAULTS.host);
  });

  test("mergeSettings coerces and validates port", () => {
    assert.equal(mergeSettings({ port: "36991" }).port, 36991);
    assert.equal(mergeSettings({ port: 8080 }).port, 8080);
    assert.equal(mergeSettings({ port: 0 }).port, DEFAULTS.port);
    assert.equal(mergeSettings({ port: 70000 }).port, DEFAULTS.port);
    assert.equal(mergeSettings({ port: "abc" }).port, DEFAULTS.port);
    assert.equal(mergeSettings({ port: 8080.5 }).port, DEFAULTS.port);
  });

  test("mergeSettings coerces and bounds minSizeMB", () => {
    assert.equal(mergeSettings({ minSizeMB: "2.5" }).minSizeMB, 2.5);
    assert.equal(mergeSettings({ minSizeMB: 0 }).minSizeMB, 0);
    assert.equal(mergeSettings({ minSizeMB: -3 }).minSizeMB, DEFAULTS.minSizeMB);
    assert.equal(mergeSettings({ minSizeMB: 999999 }).minSizeMB, DEFAULTS.minSizeMB);
  });

  test("mergeSettings accepts booleans only for toggles", () => {
    assert.equal(mergeSettings({ sniffEnabled: false }).sniffEnabled, false);
    assert.equal(mergeSettings({ sniffEnabled: "false" }).sniffEnabled, true);
    assert.equal(mergeSettings({ notify: 0 }).notify, true);
    assert.equal(mergeSettings({ manualAlwaysAsk: true }).manualAlwaysAsk, true);
  });

  test("mergeSettings sanitizes the blocklist", () => {
    const out = mergeSettings({ blocklist: [" Ads.Example.com ", "", 42, "ads.example.com", null, "x"] });
    assert.deepEqual(out.blocklist, ["ads.example.com", "x"]);
    const big = mergeSettings({ blocklist: Array.from({ length: 300 }, (_, i) => "h" + i + ".com") });
    assert.equal(big.blocklist.length, 200);
  });

  test("mergeSettings merges known category toggles only", () => {
    const out = mergeSettings({ categories: { video: false, mystery: true, audio: "yes" } });
    assert.equal(out.categories.video, false);
    assert.equal(out.categories.audio, true);
    assert.equal("mystery" in out.categories, false);
    assert.equal(out.categories.archive, true);
  });

  test("SettingsStore caches reads and persists patches", async () => {
    const backing = { data: {}, get(key, cb) { cb({ [key]: this.data[key] }); }, set(obj, cb) { Object.assign(this.data, obj); cb(); } };
    const store = new SettingsStore(backing);
    const first = await store.get();
    assert.equal(first.port, DEFAULTS.port);
    backing.data.settings = { port: 1234 };
    const cached = await store.get();
    assert.equal(cached.port, DEFAULTS.port, "cached read must not hit storage");
    const next = await store.set({ port: 1234 });
    assert.equal(next.port, 1234);
    assert.equal(backing.data.settings.port, 1234);
    store.invalidate();
    const fresh = await store.get();
    assert.equal(fresh.port, 1234);
  });

  test("SettingsStore tolerates a missing storage area", async () => {
    const store = new SettingsStore(null);
    const out = await store.get();
    assert.deepEqual(out, JSON.parse(JSON.stringify(DEFAULTS)));
    const patched = await store.set({ sniffEnabled: false });
    assert.equal(patched.sniffEnabled, false);
  });

  test("SettingsStore with chrome-like callback storage migrates old shapes", async () => {
    const backing = {
      data: { settings: { port: "4050", sniffEnabled: false, blocklist: ["a.com", 5], categories: { video: false } } },
      get(key, cb) { cb({ [key]: this.data[key] }); },
      set(obj, cb) { Object.assign(this.data, obj); cb(); },
    };
    const store = new SettingsStore(backing);
    const out = await store.get();
    assert.equal(out.port, 4050);
    assert.equal(out.sniffEnabled, false);
    assert.deepEqual(out.blocklist, ["a.com"]);
    assert.equal(out.categories.video, false);
  });
}
