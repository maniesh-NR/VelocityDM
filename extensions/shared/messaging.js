/**
 * VelocityDM shared module: command builders for the native messaging host
 * and response normalization.
 */
(function (root, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else {
    root.VDMMessaging = factory();
  }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  var patterns = typeof require !== 'undefined'
    ? require('./patterns.js')
    : (typeof self !== 'undefined' ? self.VDMPatterns : null);

  function ping() {
    return { cmd: 'ping' };
  }

  /** Build an add command from a sniffed/intercepted item. */
  function add(item) {
    item = item || {};
    const cmd = { cmd: 'add', url: item.url };
    ['filename', 'referer', 'cookies', 'userAgent', 'quality'].forEach(function (k) {
      if (item[k]) cmd[k] = item[k];
    });
    // S11-i B16: the browser-reported effective proxy endpoint (additive;
    // the engine suggests-never-overrides and probes before adopting)
    if (item.browserProxy && typeof item.browserProxy === 'object') {
      cmd.browserProxy = item.browserProxy;
    }
    return cmd;
  }

  function addBatch(patternOrUrls, cookiesBy) {
    if (typeof patternOrUrls === 'string') {
      return { cmd: 'addBatch', pattern: patternOrUrls };
    }
    const cmd = { cmd: 'addBatch', urls: patternOrUrls || [] };
    // S11-i B5: per-URL cookie jars for explicit URL-list batches
    // (capped at the first 32 URLs; the host applies them per-URL).
    if (cookiesBy && typeof cookiesBy === 'object') cmd.cookiesBy = cookiesBy;
    return cmd;
  }

  function addTorrent(magnetOrPath) {
    if (patterns && patterns.isMagnet(magnetOrPath)) {
      return { cmd: 'addTorrent', magnet: magnetOrPath };
    }
    return { cmd: 'addTorrent', torrentPath: magnetOrPath };
  }

  function status() {
    return { cmd: 'status' };
  }

  function open() {
    return { cmd: 'open' };
  }

  /** Normalize NMH replies; throws nothing, always returns {ok, ...}. */
  function normalize(reply) {
    if (!reply || typeof reply !== 'object') {
      return { ok: false, error: 'malformed host reply' };
    }
    if (reply.ok === true) return reply;
    return {
      ok: false,
      error: reply.error || 'unknown host error',
    };
  }

  return {
    ping: ping,
    add: add,
    addBatch: addBatch,
    addTorrent: addTorrent,
    status: status,
    open: open,
    normalize: normalize,
  };
});
