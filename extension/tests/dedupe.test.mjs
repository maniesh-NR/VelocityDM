// SPDX-License-Identifier: GPL-3.0-or-later
import assert from "node:assert/strict";
import { DedupeRing } from "../src/lib/dedupe.js";

export default function register(test) {
  test("first sighting is new, second is not", () => {
    const ring = new DedupeRing(10);
    assert.equal(ring.see("https://example.com/a"), true);
    assert.equal(ring.see("https://example.com/a"), false);
    assert.equal(ring.see("https://example.com/b"), true);
    assert.equal(ring.size(), 2);
  });

  test("null and undefined are never recorded", () => {
    const ring = new DedupeRing(10);
    assert.equal(ring.see(null), false);
    assert.equal(ring.see(undefined), false);
    assert.equal(ring.size(), 0);
  });

  test("evicts the oldest entry when full", () => {
    const ring = new DedupeRing(2);
    assert.equal(ring.see("a"), true);
    assert.equal(ring.see("b"), true);
    assert.equal(ring.see("c"), true);
    assert.equal(ring.size(), 2);
    assert.equal(ring.see("a"), true, "a was evicted so it is new again");
    assert.equal(ring.see("b"), true, "b was evicted when a was re-inserted");
    assert.equal(ring.see("b"), false, "the second b sighting is resident");
  });

  test("capacity is clamped to at least 1", () => {
    const ring = new DedupeRing(0);
    assert.equal(ring.see("x"), true);
    assert.equal(ring.see("x"), false, "re-seeing the resident entry is not new");
    assert.equal(ring.see("y"), true, "y evicts x in a cap-1 ring");
    assert.equal(ring.see("x"), true, "x was evicted so it is new again");
    const ring2 = new DedupeRing(-5);
    assert.equal(ring2.capacity, 1);
    const ring3 = new DedupeRing(Number.NaN);
    assert.equal(ring3.capacity, 500);
  });

  test("handles high-volume churn", () => {
    const ring = new DedupeRing(100);
    for (let i = 0; i < 1000; i += 1) ring.see("u" + i);
    assert.equal(ring.size(), 100);
    assert.equal(ring.see("u0"), true, "u0 has been evicted long ago");
    assert.equal(ring.see("u999"), false, "u999 is resident");
  });
}
