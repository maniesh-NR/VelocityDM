/**
 * VelocityDM shared module: browser-proxy report (S11-i B16 extension side).
 *
 * When the `proxy` permission exists and the API is available, resolve the
 * browser's effective proxy: mode 'system' reports the mode name only (no
 * explicit endpoint); fixed_servers/single_proxy reports {mode:'fixed',
 * scheme, host, port} with the scheme limited to http/https/socks5 (the
 * NMH boundary validates exactly those); other modes report the mode name
 * only. An absent permission, a missing API, or a thrown call skips
 * silently — the add proceeds without the field (best-effort, never a
 * failed add).
 *
 * The API is injected so the node battery drives it with fakes; the
 * resolved endpoint rides add commands as the additive `browserProxy`
 * field (the engine side: suggest, never override — probe-guarded
 * adoption + warnings; landed in the wave-2 engine batch).
 */
(function (root, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else {
    root.VDMProxyReport = factory();
  }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  var SUPPORTED_SCHEMES = ['http', 'https', 'socks5'];

  /**
   * Resolve a chrome.proxy.settings.get result's `value` into a report.
   * Pure; returns null when nothing usable is present.
   */
  function resolveProxy(value) {
    if (!value || typeof value !== 'object' || typeof value.mode !== 'string') {
      return null;
    }
    if (value.mode === 'fixed_servers') {
      var rules = value.rules || {};
      var entry = rules.singleProxy ||
        (Array.isArray(rules.proxyList) && rules.proxyList[0]) ||
        (Array.isArray(rules.proxy) && rules.proxy[0]) ||
        null;
      if (!entry || !entry.host || typeof entry.port !== 'number') {
        return { mode: 'fixed_servers' }; // mode name only, no endpoint
      }
      var scheme = typeof entry.scheme === 'string' ? entry.scheme : 'http';
      if (SUPPORTED_SCHEMES.indexOf(scheme) === -1) {
        return { mode: 'fixed_servers' }; // unsupported scheme: no endpoint
      }
      return { mode: 'fixed', scheme: scheme, host: entry.host, port: entry.port };
    }
    // 'system', 'direct', 'pac_script', 'auto_detect', others: mode only
    return { mode: value.mode };
  }

  /** The add-command endpoint shape, or null when the report has none. */
  function endpointOf(report) {
    if (report && report.mode === 'fixed' &&
      SUPPORTED_SCHEMES.indexOf(report.scheme) !== -1 &&
      typeof report.host === 'string' && report.host &&
      typeof report.port === 'number') {
      return { scheme: report.scheme, host: report.host, port: report.port };
    }
    return null;
  }

  /**
   * Read the effective proxy through a chrome.proxy-like API.
   * api = { settings: { get(callback) } }. Best-effort: absent API,
   * thrown call, or an unusable value resolve to null — never a rejection.
   */
  function browserProxyFor(api) {
    var report = null;
    return new Promise(function (resolve) {
      if (!api || !api.settings || typeof api.settings.get !== 'function') {
        resolve(null);
        return;
      }
      try {
        api.settings.get(function (details) {
          try {
            report = resolveProxy(details && details.value);
          } catch (e) {
            report = null;
          }
          resolve(report);
        });
      } catch (e) {
        resolve(null);
      }
    }).then(function (r) {
      return endpointOf(r); // the endpoint is what rides the add command
    });
  }

  return {
    SUPPORTED_SCHEMES: SUPPORTED_SCHEMES,
    resolveProxy: resolveProxy,
    endpointOf: endpointOf,
    browserProxyFor: browserProxyFor,
  };
});
