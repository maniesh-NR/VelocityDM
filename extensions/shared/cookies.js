/**
 * VelocityDM shared module: cookie-jar forwarding (S11-i B5).
 *
 * Every add path with an http(s) URL forwards the page's cookie jar so
 * authenticated downloads do not fail: context-menu link/image/video
 * adds, popup manual adds, batch adds (per-URL, capped at the first 32
 * URLs of a batch) and the takeover path. The jar read uses
 * cookies.getAll({url}) — session AND persistent cookies (our choice:
 * auth commonly rides session cookies). Secrecy: jar contents never
 * reach any log channel; only static strings are logged anywhere.
 *
 * The API is injected so the node battery can drive it with a fake;
 * callers pass the real chrome.cookies.
 */
(function (root, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else {
    root.VDMCookies = factory();
  }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  var BATCH_COOKIE_CAP = 32;

  function isHttpUrl(url) {
    return typeof url === 'string' && /^https?:/i.test(url);
  }

  /**
   * Serialize a cookies.getAll({url}) result the way the takeover path
   * always did: name=value joined with '; '. An empty jar yields ''.
   */
  function serializeJar(cookies) {
    if (!Array.isArray(cookies)) return '';
    return cookies
      .map(function (c) { return c && c.name && c.value ? c.name + '=' + c.value : ''; })
      .filter(function (s) { return s !== ''; })
      .join('; ');
  }

  /**
   * Read the jar for one URL. api = { getAll({url}) -> Promise<cookies> }.
   * Best-effort: a restricted permission or a thrown API yields '' (the
   * add proceeds, exactly like today's takeover path).
   */
  function jarFor(url, api) {
    if (!isHttpUrl(url)) return Promise.resolve('');
    if (!api || typeof api.getAll !== 'function') return Promise.resolve('');
    return api.getAll({ url: url })
      .then(function (cookies) { return serializeJar(cookies); })
      .catch(function () { return ''; });
  }

  /**
   * Attach the jar to an add item (mutates and resolves the item):
   * item.cookies = jar. Non-http(s) items (magnets, torrent paths) are
   * left untouched — cookies do not apply to them.
   */
  function withJar(item, api) {
    item = item || {};
    return jarFor(item.url, api).then(function (jar) {
      if (isHttpUrl(item.url)) item.cookies = jar;
      return item;
    });
  }

  /**
   * The batch cookie plan: which URLs of a batch get a jar read — the
   * FIRST 32 http(s) URLs (per-URL, capped; the rest of the batch still
   * downloads, just without per-URL cookies).
   */
  function batchCookiePlan(urls, cap) {
    var limit = typeof cap === 'number' ? cap : BATCH_COOKIE_CAP;
    var withCookies = [];
    var skipped = 0;
    (urls || []).forEach(function (u) {
      if (isHttpUrl(u) && withCookies.length < limit) withCookies.push(u);
      else skipped++;
    });
    return { withCookies: withCookies, skipped: skipped };
  }

  /**
   * Build the per-URL cookiesBy map for a URL-list batch (cap 32).
   * Resolves {map, covered} — the map only carries NON-EMPTY jars (an
   * empty jar adds no header, matching the add path's behavior).
   */
  function jarsForBatch(urls, api, cap) {
    var plan = batchCookiePlan(urls, cap);
    return Promise.all(plan.withCookies.map(function (u) {
      return jarFor(u, api).then(function (jar) { return [u, jar]; });
    })).then(function (pairs) {
      var map = {};
      pairs.forEach(function (p) {
        if (p[1]) map[p[0]] = p[1];
      });
      return { map: map, covered: plan.withCookies.length, skipped: plan.skipped };
    });
  }

  return {
    BATCH_COOKIE_CAP: BATCH_COOKIE_CAP,
    isHttpUrl: isHttpUrl,
    serializeJar: serializeJar,
    jarFor: jarFor,
    withJar: withJar,
    batchCookiePlan: batchCookiePlan,
    jarsForBatch: jarsForBatch,
  };
});
