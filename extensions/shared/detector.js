/**
 * VelocityDM shared module: URL/mime detection for videos and downloads.
 * UMD: usable from Chrome/Opera/Edge service workers (importScripts),
 * Firefox event pages (script tag order) and Node (require).
 */
(function (root, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else {
    root.VDMDetector = factory();
  }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  var VIDEO_EXTS = ['mp4', 'webm', 'mkv', 'flv', 'avi', 'mov', 'm4v', 'wmv', 'mpg', 'mpeg', '3gp'];
  var AUDIO_EXTS = ['mp3', 'm4a', 'aac', 'flac', 'ogg', 'opus', 'wav'];
  var ARCHIVE_EXTS = ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'iso', 'cab'];
  var DOC_EXTS = ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'odt', 'ods'];
  var PROG_EXTS = ['exe', 'msi', 'apk', 'dmg', 'deb', 'rpm', 'appimage', 'bin', 'sh'];
  var MEDIA_EXTS = VIDEO_EXTS.concat(AUDIO_EXTS);

  function extOf(url) {
    try {
      var path = url.split('#')[0].split('?')[0];
      var last = path.substring(path.lastIndexOf('/') + 1);
      var dot = last.lastIndexOf('.');
      if (dot < 0) return '';
      return last.substring(dot + 1).toLowerCase();
    } catch (e) {
      return '';
    }
  }

  function isHls(url, mime) {
    return extOf(url) === 'm3u8' ||
      (mime && /mpegurl/i.test(mime)) ||
      (url && url.indexOf('.m3u8') !== -1);
  }

  function isDash(url, mime) {
    return extOf(url) === 'mpd' ||
      (mime && /dash\+xml/i.test(mime)) ||
      (url && url.indexOf('.mpd') !== -1);
  }

  function isVideo(url, mime, contentLength) {
    if (isHls(url, mime) || isDash(url, mime)) return true;
    var looksVideo = (mime && /^video\//i.test(mime)) || VIDEO_EXTS.indexOf(extOf(url)) !== -1;
    if (!looksVideo) return false;
    // small clips are usually page assets (ads, previews)
    if (typeof contentLength === 'number' && contentLength > 0 && contentLength < 5 * 1024 * 1024) {
      return false;
    }
    return true;
  }

  function isAudio(url, mime) {
    if (mime && /^audio\//i.test(mime)) return true;
    return AUDIO_EXTS.indexOf(extOf(url)) !== -1;
  }

  function isLargeFile(url, mime, contentLength) {
    if (typeof contentLength === 'number' && contentLength >= 5 * 1024 * 1024) return true;
    var e = extOf(url);
    return ARCHIVE_EXTS.indexOf(e) !== -1 ||
      PROG_EXTS.indexOf(e) !== -1 ||
      DOC_EXTS.indexOf(e) !== -1 ||
      MEDIA_EXTS.indexOf(e) !== -1;
  }

  function category(url, mime) {
    if (isHls(url, mime)) return 'video-stream';
    if (isDash(url, mime)) return 'video-stream';
    if (isVideo(url, mime)) return 'video';
    if (isAudio(url, mime)) return 'audio';
    var e = extOf(url);
    if (ARCHIVE_EXTS.indexOf(e) !== -1) return 'archive';
    if (PROG_EXTS.indexOf(e) !== -1) return 'program';
    if (DOC_EXTS.indexOf(e) !== -1) return 'document';
    if (/^text\/html/i.test(mime || '')) return 'page';
    return 'other';
  }

  /** Best-effort quality label from URL/mime/headers. */
  function qualityLabel(url, mime, headers) {
    var m;
    if (headers && headers['x-resolution']) return String(headers['x-resolution']);
    m = /(\d{3,4})\s*[x×]\s*(\d{3,4})/.exec(url || '');
    if (m) return m[2] + 'p';
    m = /[-_.](2160|1440|1080|720|480|360|240)p?\b/i.exec(url || '');
    if (m) return m[1] + 'p';
    m = /\b(4k|uhd|hd|sd)\b/i.exec(url || '');
    if (m) return m[1].toLowerCase();
    if (isHls(url, mime)) return 'HLS';
    if (isDash(url, mime)) return 'DASH';
    if (/^video\//i.test(mime || '')) return 'video';
    return '';
  }

  /** A readable title for a sniffed video entry. */
  function titleFor(url) {
    try {
      var path = url.split('#')[0].split('?')[0];
      var last = decodeURIComponent(path.substring(path.lastIndexOf('/') + 1));
      if (last) return last;
      var u = new URL(url);
      return u.hostname;
    } catch (e) {
      return url;
    }
  }

  // -----------------------------------------------------------------------
  // S11-i B7: DOM candidate gating (thresholds ours; deps injectable so
  // the node battery drives the table with hand-rolled element-likes)
  // -----------------------------------------------------------------------

  var GEOM_MIN_W = 200;   // popup-list gate (ours; NOT the reference's)
  var GEOM_MIN_H = 120;
  var PROMINENT_W = 640;  // "large video" hint sub-tier
  var PROMINENT_H = 360;
  var INLINE_SCAN_CAP = 131072; // first 128 KiB of each inline script
  var OBSERVER_DEBOUNCE_MS = 250;
  var CHAIN_WALK_LIMIT = 32;

  function rectOf(el) {
    try { return el.getBoundingClientRect(); } catch (e) { return null; }
  }

  /** Geometry gate: attached, footprint >= 200x120, not clipped to zero. */
  function passesGeometry(el) {
    if (!el) return false;
    if (el.isConnected === false) return false; // detached
    var r = rectOf(el);
    if (!r) return false;
    if (r.width <= 0 || r.height <= 0) return false; // clipped to zero
    return r.width >= GEOM_MIN_W && r.height >= GEOM_MIN_H;
  }

  /** Prominence sub-tier (>= 640x360) feeding the "large video" hint. */
  function isProminent(el) {
    var r = rectOf(el);
    return !!(r && r.width >= PROMINENT_W && r.height >= PROMINENT_H);
  }

  /** State gate: playable now, or loading with known dimensions. */
  function passesState(el) {
    if (!el) return false;
    if (typeof el.readyState === 'number' && el.readyState >= 2) return true;
    return el.videoWidth > 0 && el.networkState === 2 &&
      (typeof el.readyState !== 'number' || el.readyState < 1);
  }

  /**
   * Overlay filter (implemented rule): the element and its ancestor
   * chain (bounded walk) carry no `display:none` / `visibility:hidden`.
   */
  function chainVisible(el, computedOf) {
    var node = el;
    var steps = 0;
    while (node && steps < CHAIN_WALK_LIMIT) {
      var cs;
      try { cs = computedOf ? computedOf(node) : null; } catch (e) { cs = null; }
      if (cs && (cs.display === 'none' || cs.visibility === 'hidden')) return false;
      if (node.parentElement) node = node.parentElement; else break;
      steps++;
    }
    return true;
  }

  /** A full DOM candidate: geometry + state + visibility gates. */
  function domCandidate(el, computedOf) {
    if (!passesGeometry(el) || !passesState(el) || !chainVisible(el, computedOf)) {
      return { ok: false };
    }
    var src = el.currentSrc || el.src || '';
    if (!src && typeof el.querySelector === 'function') {
      var s = el.querySelector('source');
      if (s && s.src) src = s.src;
    }
    return {
      ok: true,
      prominent: isProminent(el),
      url: src,
      title: el.title || '',
    };
  }

  /**
   * Inline-script scan: URL matches in the FIRST 128 KiB of the script
   * body only — http(s) URLs ending in a media extension (the trailing
   * boundary must not continue with a letter, so .mp4x is not a match).
   */
  function scanInlineScript(text) {
    if (typeof text !== 'string' || !text) return [];
    var head = text.length > INLINE_SCAN_CAP ? text.substring(0, INLINE_SCAN_CAP) : text;
    var re = /https?:\/\/[^\s"'<>\\]+?\.(?:mp4|webm|m3u8|mpd|m4s|ts)(?![a-z])/gi;
    var out = [];
    var m;
    while ((m = re.exec(head)) !== null) {
      if (out.indexOf(m[0]) === -1) out.push(m[0]);
    }
    return out;
  }

  /** Site pack match (data-driven; packs load OFF by default). */
  function packMatches(pack, url) {
    if (!pack || typeof url !== 'string') return false;
    var pats = Array.isArray(pack.urlPatterns) ? pack.urlPatterns : [];
    for (var i = 0; i < pats.length; i++) {
      if (typeof pats[i] === 'string' && pats[i] && url.indexOf(pats[i]) !== -1) {
        return true;
      }
    }
    return false;
  }

  /** Load packs from settings: absent/empty by default (none ship on). */
  function loadPacks(settings) {
    if (!settings || !Array.isArray(settings.sitePacks)) return [];
    return settings.sitePacks.filter(function (p) {
      return p && (Array.isArray(p.selectors) || Array.isArray(p.urlPatterns));
    });
  }

  /**
   * Bounded detection window (S11-i B7): one MutationObserver
   * (childList+subtree, media-tag-filtered) + a ResizeObserver on the
   * current candidates — debounced 250 ms, active ONLY while the window
   * is open; close() fully tears both down. Observer constructors and
   * timers are injectable for the battery.
   */
  function detectionWindow(deps) {
    deps = deps || {};
    var moCtor = deps.MutationObserver;
    var roCtor = deps.ResizeObserver;
    var timers = deps.setTimeout || setTimeout;
    var clearTimers = deps.clearTimeout || clearTimeout;
    var gather = deps.gather || function () { return []; };
    var onChange = deps.onChange || function () {};
    var mo = null;
    var ro = null;
    var pending = null;
    var open = false;
    var MEDIA_TAGS = { VIDEO: 1, AUDIO: 1, SOURCE: 1 };

    function refresh() {
      pending = null;
      onChange(gather());
    }

    function open_() {
      if (open) return;
      open = true;
      if (typeof moCtor === 'function') {
        mo = new moCtor(function (records) {
          var media = false;
          for (var i = 0; i < records.length; i++) {
            var r = records[i];
            var nodes = (r.addedNodes || []).concat(r.removedNodes || []);
            for (var j = 0; j < nodes.length; j++) {
              if (nodes[j] && nodes[j].tagName && MEDIA_TAGS[nodes[j].tagName]) {
                media = true;
                break;
              }
            }
            if (media) break;
          }
          if (!media) return; // media-related mutations only
          if (pending !== null) clearTimers(pending);
          pending = timers(refresh, OBSERVER_DEBOUNCE_MS);
        });
        mo.observe(deps.target || {}, { childList: true, subtree: true });
      }
      if (typeof roCtor === 'function') {
        ro = new roCtor(function () {
          if (pending !== null) clearTimers(pending);
          pending = timers(refresh, OBSERVER_DEBOUNCE_MS);
        });
        (gather() || []).forEach(function (c) {
          if (c && c.el && typeof ro.observe === 'function') ro.observe(c.el);
        });
      }
    }

    function close_() {
      open = false;
      if (pending !== null) { clearTimers(pending); pending = null; }
      if (mo) { mo.disconnect(); mo = null; }
      if (ro) { ro.disconnect(); ro = null; }
    }

    function isOpen() { return open; }

    return { open: open_, close: close_, isOpen: isOpen };
  }

  return {
    VIDEO_EXTS: VIDEO_EXTS,
    AUDIO_EXTS: AUDIO_EXTS,
    ARCHIVE_EXTS: ARCHIVE_EXTS,
    DOC_EXTS: DOC_EXTS,
    PROG_EXTS: PROG_EXTS,
    extOf: extOf,
    isHls: isHls,
    isDash: isDash,
    isVideo: isVideo,
    isAudio: isAudio,
    isLargeFile: isLargeFile,
    category: category,
    qualityLabel: qualityLabel,
    titleFor: titleFor,
    // S11-i B7
    GEOM_MIN_W: GEOM_MIN_W,
    GEOM_MIN_H: GEOM_MIN_H,
    PROMINENT_W: PROMINENT_W,
    PROMINENT_H: PROMINENT_H,
    INLINE_SCAN_CAP: INLINE_SCAN_CAP,
    OBSERVER_DEBOUNCE_MS: OBSERVER_DEBOUNCE_MS,
    passesGeometry: passesGeometry,
    isProminent: isProminent,
    passesState: passesState,
    chainVisible: chainVisible,
    domCandidate: domCandidate,
    scanInlineScript: scanInlineScript,
    packMatches: packMatches,
    loadPacks: loadPacks,
    detectionWindow: detectionWindow,
  };
});
