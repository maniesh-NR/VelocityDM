/**
 * VelocityDM shared module: page link extraction and classification
 * ("grab every link on a page" - XDM batch/link grabber).
 */
(function (root, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else {
    root.VDMLinks = factory();
  }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  var detector = typeof require !== 'undefined' ? require('./detector.js') : (root.VDMDetector);

  /**
   * Extract interesting download links from a flat document view.
   * anchors: [{href, text, download}] - gathered by the content script.
   * images: [{src, alt}] (optional).
   * Returns grouped, deduplicated entries with categories.
   */
  function extract(anchors, images, options) {
    options = options || {};
    var seen = Object.create(null);
    var groups = { media: [], video: [], archive: [], document: [], program: [], other: [] };
    (anchors || []).forEach(function (a) {
      var href = a.href;
      if (!href || !/^https?:/i.test(href)) return;
      if (seen[href]) return;
      seen[href] = true;
      var entry = {
        url: href,
        name: displayName(href, a.text, a.download),
        category: detector.category(href, null),
      };
      addTo(groups, entry);
    });
    (images || []).forEach(function (img) {
      var src = img.src;
      if (!src || !/^https?:/i.test(src)) return;
      if (seen[src]) return;
      seen[src] = true;
      var entry = {
        url: src,
        name: displayName(src, img.alt, null),
        category: 'image',
      };
      groups.other.push(entry);
    });
    return groups;
  }

  function addTo(groups, entry) {
    switch (entry.category) {
      case 'video':
      case 'video-stream':
        groups.video.push(entry);
        break;
      case 'audio':
        groups.media.push(entry);
        break;
      case 'archive':
        groups.archive.push(entry);
        break;
      case 'document':
        groups.document.push(entry);
        break;
      case 'program':
        groups.program.push(entry);
        break;
      default:
        // plain pages are not offered unless explicitly requested
        groups.other.push(entry);
    }
  }

  function displayName(url, text, downloadAttr) {
    if (downloadAttr) return downloadAttr;
    try {
      var path = url.split('#')[0].split('?')[0];
      var last = decodeURIComponent(path.substring(path.lastIndexOf('/') + 1));
      if (last && /\.[a-z0-9]{1,6}$/i.test(last)) return last;
      if (text && text.trim().length > 0 && text.trim().length < 80) return text.trim();
      return last || url;
    } catch (e) {
      return url;
    }
  }

  /** Only categories the user asked to intercept. */
  function filterGroups(groups, wanted) {
    var out = {};
    Object.keys(groups).forEach(function (k) {
      if (wanted === true || (wanted && wanted.indexOf(k) !== -1)) {
        out[k] = groups[k];
      }
    });
    return out;
  }

  /** Flatten groups into a URL list (for batch add). */
  function flatten(groups) {
    var urls = [];
    Object.keys(groups).forEach(function (k) {
      groups[k].forEach(function (e) { urls.push(e.url); });
    });
    return urls;
  }

  return { extract: extract, filterGroups: filterGroups, flatten: flatten, displayName: displayName };
});
