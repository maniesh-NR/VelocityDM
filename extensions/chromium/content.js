/**
 * VelocityDM content script: link/media grabber for the popup and the
 * "download all links" context-menu action, plus the on-demand DOM video
 * detection pass (S11-i B7): a bounded detection window (MutationObserver
 * debounced 250 ms + ResizeObserver on current candidates, full teardown)
 * opens on media play events only; a popup-open detection runs one pass.
 */
/* global chrome, document, window */

(function () {
  'use strict';

  if (window.__vdmContentLoaded) return;
  window.__vdmContentLoaded = true;

  var DET = window.VDMDetector;

  if (!DET) {
    // shared module missing from the injection list: degrade gracefully
    // (no DOM detection; grab still works when VDMLinks is present)
    DET = {
      domCandidate: function () { return { ok: false }; },
      scanInlineScript: function () { return []; },
      loadPacks: function () { return []; },
      titleFor: function (u) { return u; },
      detectionWindow: function () { return { open: function () {}, close: function () {}, isOpen: function () { return false; } }; },
    };
  }

  function collect() {
    const anchors = Array.from(document.querySelectorAll('a[href]')).map(function (a) {
      return {
        href: a.href,
        text: (a.textContent || '').trim().substring(0, 120),
        download: a.hasAttribute('download') ? a.getAttribute('download') : null,
      };
    });
    const images = Array.from(document.querySelectorAll('img[src]')).map(function (img) {
      return { src: img.src, alt: img.alt || '' };
    });
    // inline <video>/<audio> sources (page-embedded media)
    const media = Array.from(document.querySelectorAll('video, audio')).map(function (el) {
      const src = el.currentSrc || el.src || (el.querySelector('source') && el.querySelector('source').src) || '';
      return { href: src, text: el.title || '', download: null };
    });
    const groups = window.VDMLinks
      ? window.VDMLinks.extract(anchors.concat(media), images)
      : { media: [], video: [], archive: [], document: [], program: [], other: [] };
    return groups;
  }

  // -------------------------------------------------------------------------
  // S11-i B7: DOM video detection (geometry >= 200x120 ours, state gate,
  // overlay filter; inline-script scan capped at the first 128 KiB; site
  // packs data-driven, OFF by default)
  // -------------------------------------------------------------------------

  var computedOf = function (el) {
    try { return window.getComputedStyle(el); } catch (e) { return null; }
  };

  var settingsCache = null;

  function refreshSettings() {
    return new Promise(function (resolve) {
      try {
        chrome.storage.local.get('settings', function (stored) {
          settingsCache = (window.VDMFilters && stored && stored.settings)
            ? window.VDMFilters.mergeSettings(stored.settings)
            : (window.VDMFilters ? window.VDMFilters.mergeSettings(null) : null);
          resolve(settingsCache);
        });
      } catch (e) {
        settingsCache = window.VDMFilters ? window.VDMFilters.mergeSettings(null) : null;
        resolve(settingsCache);
      }
    });
  }

  function mediaElements() {
    return Array.from(document.querySelectorAll('video, audio'));
  }

  function packSelectorElements(settings) {
    // data-driven site packs (selectors only; no site-specific code);
    // OFF by default — none ship enabled
    var packs = DET.loadPacks(settings || settingsCache);
    if (!packs.length) return [];
    var out = [];
    packs.forEach(function (p) {
      (p.selectors || []).forEach(function (sel) {
        try { out = out.concat(Array.from(document.querySelectorAll(sel))); } catch (e) { /* bad selector skipped */ }
      });
    });
    return out;
  }

  /** One bounded DOM detection pass (sync; settings passed in). */
  function detectDomVideos(settings) {
    var settings_ = settings || settingsCache;
    var els = mediaElements().concat(packSelectorElements(settings_));
    var out = [];
    var seen = {};
    els.forEach(function (el) {
      var c = DET.domCandidate(el, computedOf);
      if (!c.ok || !c.url || seen[c.url]) return;
      seen[c.url] = true;
      out.push({
        url: c.url,
        title: c.title || DET.titleFor(c.url),
        quality: c.prominent ? 'large video' : '',
        kind: 'dom',
        pageUrl: location.href,
        size: 0,
      });
    });
    // inline-script scan: media URLs found in the FIRST 128 KiB of each
    // inline <script> join ONLY when a media element passed the gates —
    // the geometry-gated element is the one that plays them (source
    // matches are deduped via `seen` above)
    if (out.length) {
      Array.from(document.querySelectorAll('script:not([src])')).forEach(function (s) {
        DET.scanInlineScript(s.textContent || '').forEach(function (u) {
          if (seen[u]) return;
          seen[u] = true;
          out.push({
            url: u,
            title: DET.titleFor(u),
            quality: '',
            kind: 'dom',
            pageUrl: location.href,
            size: 0,
          });
        });
      });
    }
    return out;
  }

  var window_ = null;

  function openDetectionWindow() {
    if (window_) return; // bounded: ONE window at a time
    refreshSettings().then(function () {
      if (window_) return;
      var w = DET.detectionWindow({
        target: document.documentElement || document.body || {},
        MutationObserver: (typeof MutationObserver !== 'undefined') ? MutationObserver : undefined,
        ResizeObserver: (typeof ResizeObserver !== 'undefined') ? ResizeObserver : undefined,
        setTimeout: function (fn, ms) { return window.setTimeout(fn, ms); },
        clearTimeout: function (h) { return window.clearTimeout(h); },
        gather: function () { return detectDomVideos(); },
        onChange: function (videos) {
          try {
            chrome.runtime.sendMessage({ type: 'domVideos', videos: videos }, function () {
              void chrome.runtime.lastError; // background may be asleep
            });
          } catch (e) { /* extension context gone */ }
        },
      });
      w.open();
      window_ = w;
      // bounded window lifetime: full teardown after 10 s of activity
      window.setTimeout(closeDetectionWindow, 10000);
    });
  }

  function closeDetectionWindow() {
    if (window_) { window_.close(); window_ = null; }
  }

  // media play events open the bounded detection window (on demand)
  document.addEventListener('play', openDetectionWindow, true);
  document.addEventListener('loadedmetadata', openDetectionWindow, true);

  chrome.runtime.onMessage.addListener(function (msg, _sender, sendResponse) {
    if (msg && msg.type === 'grabLinks') {
      sendResponse(collect());
      return false;
    }
    if (msg && msg.type === 'detectVideos') {
      // popup open / context-menu query: one bounded pass, no observers
      refreshSettings().then(function (s) {
        sendResponse({ ok: true, videos: detectDomVideos(s) });
      });
      return true; // async sendResponse
    }
    return false;
  });
})();
