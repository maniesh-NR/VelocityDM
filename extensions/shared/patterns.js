/**
 * VelocityDM shared module: URL pattern expansion (batch downloads),
 * mirroring the Rust `vdm_core::batch` semantics.
 */
(function (root, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else {
    root.VDMPatterns = factory();
  }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  /** Expand "[1:5]" ranges and "[a,b,c]" lists; padding is preserved. */
  function expand(pattern) {
    if (typeof pattern !== 'string' || pattern.length === 0) {
      return { ok: false, error: 'empty pattern', urls: [] };
    }
    var out = [''];
    var i = 0;
    while (i < pattern.length) {
      if (pattern[i] === '[') {
        var close = pattern.indexOf(']', i + 1);
        if (close === -1) {
          return { ok: false, error: 'unbalanced [', urls: [] };
        }
        var body = pattern.substring(i + 1, close);
        var alts;
        if (body.indexOf(':') !== -1) {
          var parts = body.split(':');
          if (parts.length !== 2) {
            return { ok: false, error: 'bad range', urls: [] };
          }
          var from = parts[0], to = parts[1];
          if (from.length !== to.length || !/^\d+$/.test(from) || !/^\d+$/.test(to)) {
            return { ok: false, error: 'bad range endpoints', urls: [] };
          }
          var a = parseInt(from, 10), b = parseInt(to, 10);
          if (a > b || b - a > 100000) {
            return { ok: false, error: 'bad or oversized range', urls: [] };
          }
          alts = [];
          for (var n = a; n <= b; n++) {
            var s = String(n);
            while (s.length < from.length) s = '0' + s;
            alts.push(s);
          }
        } else if (body.indexOf(',') !== -1) {
          alts = body.split(',').map(function (x) { return x.trim(); }).filter(Boolean);
          if (alts.length === 0) {
            return { ok: false, error: 'empty list', urls: [] };
          }
        } else {
          return { ok: false, error: 'bracket must be [a:b] or [a,b,c]', urls: [] };
        }
        if (alts.length * out.length > 200000) {
          return { ok: false, error: 'too many urls', urls: [] };
        }
        var next = [];
        out.forEach(function (prefix) {
          alts.forEach(function (alt) {
            next.push(prefix + alt);
          });
        });
        out = next;
        i = close + 1;
      } else {
        for (var k = 0; k < out.length; k++) out[k] += pattern[i];
        i++;
      }
    }
    out = out.filter(Boolean);
    if (out.length === 0) {
      return { ok: false, error: 'empty pattern', urls: [] };
    }
    return { ok: true, urls: out, error: '' };
  }

  /** Parse pasted link lists (one per line, # comments). */
  function parseList(text) {
    return (text || '')
      .split(/\r?\n/)
      .map(function (l) { return l.trim(); })
      .filter(function (l) { return l && l.charAt(0) !== '#'; });
  }

  function isHttpUrl(s) {
    return /^https?:\/\/\S+$/i.test(s);
  }

  function isMagnet(s) {
    return /^magnet:\?\S+/i.test(s);
  }

  return { expand: expand, parseList: parseList, isHttpUrl: isHttpUrl, isMagnet: isMagnet };
});
