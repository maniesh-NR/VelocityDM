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
  };
});
