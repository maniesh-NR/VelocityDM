/**
 * VelocityDM shared module: interception filters (which browser downloads
 * VelocityDM should take over) and settings model.
 */
(function (root, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else {
    root.VDMFilters = factory();
  }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  var DEFAULTS = {
    // categories handed over to VelocityDM
    interceptCategories: ['video', 'video-stream', 'audio', 'archive', 'document', 'program'],
    // minimum bytes before a generic file is intercepted (0 = always)
    minSizeBytes: 1024 * 1024,
    // sites never touched
    excludedHosts: [],
    // filename extensions always intercepted regardless of size
    alwaysExts: ['exe', 'msi', 'apk', 'dmg', 'deb', 'rpm', 'zip', 'rar', '7z', 'iso', 'torrent'],
    // ask before taking over each download
    confirmTakeover: false,
    // NMH application name
    nativeHostName: 'com.velocitydm.nmh',
  };

  function mergeSettings(stored) {
    var out = JSON.parse(JSON.stringify(DEFAULTS));
    if (stored) {
      Object.keys(stored).forEach(function (k) {
        out[k] = stored[k];
      });
    }
    return out;
  }

  function hostOf(url) {
    try {
      return new URL(url).hostname;
    } catch (e) {
      return '';
    }
  }

  function isExcluded(url, settings) {
    var host = hostOf(url);
    if (!host) return true;
    return (settings.excludedHosts || []).some(function (h) {
      return host === h || host.endsWith('.' + h);
    });
  }

  /** Decide whether a browser download should be taken over. */
  function shouldTakeOver(item, settings) {
    if (!item || !item.url || !/^https?:/i.test(item.url)) return false;
    if (item.byExtensionId) return false; // our own or another extension's
    if (isExcluded(item.url, settings)) return false;
    if (/\.torrent$/i.test(item.url.split('?')[0])) return true;
    var ext = item.filename
      ? item.filename.substring(item.filename.lastIndexOf('.') + 1).toLowerCase()
      : '';
    if (settings.alwaysExts.indexOf(ext) !== -1) return true;
    var cat = categoryOf(item);
    if (settings.interceptCategories.indexOf(cat) !== -1) {
      var size = typeof item.fileSize === 'number' ? item.fileSize : (item.totalBytes || 0);
      if (cat === 'video' || cat === 'video-stream' || cat === 'audio') return true;
      if (size >= settings.minSizeBytes) return true;
      if (ext && settings.interceptCategories.indexOf(cat) !== -1 && size === 0) return true;
    }
    return false;
  }

  function categoryOf(item) {
    var detector = typeof require !== 'undefined'
      ? require('./detector.js')
      : (typeof self !== 'undefined' ? self.VDMDetector : null);
    if (!detector) return 'other';
    return detector.category(item.url || '', item.mime || '');
  }

  return {
    DEFAULTS: DEFAULTS,
    mergeSettings: mergeSettings,
    hostOf: hostOf,
    isExcluded: isExcluded,
    shouldTakeOver: shouldTakeOver,
    categoryOf: categoryOf,
  };
});
