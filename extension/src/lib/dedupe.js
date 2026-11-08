// SPDX-License-Identifier: GPL-3.0-or-later
// Bounded FIFO ring for seen-URL bookkeeping in VelocityDM Companion.
// Membership is tracked in a Set and insertion order in a fixed-size array,
// so every operation is O(1) regardless of capacity.

export class DedupeRing {
  constructor(cap = 500) {
    const requested = Number.isFinite(cap) ? Math.floor(cap) : 500;
    this.capacity = Math.max(1, requested);
    this.slots = new Array(this.capacity).fill(null);
    this.cursor = 0;
    this.members = new Set();
  }

  // Records a URL and returns true when it has not been seen before. Once the
  // ring is full the oldest URL is evicted, so a URL can become "new" again.
  see(url) {
    if (url === null || url === undefined) return false;
    if (this.members.has(url)) return false;
    const evicted = this.slots[this.cursor];
    if (evicted !== null) this.members.delete(evicted);
    this.slots[this.cursor] = url;
    this.members.add(url);
    this.cursor = (this.cursor + 1) % this.capacity;
    return true;
  }

  size() {
    return this.members.size;
  }
}
