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
    return cmd;
  }

  function addBatch(patternOrUrls) {
    if (typeof patternOrUrls === 'string') {
      return { cmd: 'addBatch', pattern: patternOrUrls };
    }
    return { cmd: 'addBatch', urls: patternOrUrls || [] };
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
