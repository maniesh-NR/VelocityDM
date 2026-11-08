/**
 * VelocityDM shared module: bounded recent-media store (LRU) used for the
 * sniffed video list shown in the popup.
 */
(function (root, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else {
    root.VDMState = factory();
  }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  /** Create a store capped at `max` entries; equal URLs update in place. */
  function recentStore(max) {
    var cap = max || 50;
    var items = [];
    return {
      push: function (entry) {
        if (!entry || !entry.url) return items;
        for (var i = 0; i < items.length; i++) {
          if (items[i].url === entry.url) {
            items.splice(i, 1);
            break;
          }
        }
        entry.seenAt = Date.now();
        items.unshift(entry);
        if (items.length > cap) items.length = cap;
        return items;
      },
      list: function () {
        return items.slice();
      },
      clear: function () {
        items = [];
        return items;
      },
      size: function () {
        return items.length;
      },
    };
  }

  return { recentStore: recentStore };
});
