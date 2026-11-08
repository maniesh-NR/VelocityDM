/**
 * VelocityDM content script: link/media grabber for the popup and the
 * "download all links" context-menu action.
 */
/* global chrome */

(function () {
  'use strict';

  if (window.__vdmContentLoaded) return;
  window.__vdmContentLoaded = true;

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

  chrome.runtime.onMessage.addListener(function (msg, _sender, sendResponse) {
    if (msg && msg.type === 'grabLinks') {
      sendResponse(collect());
    }
    return false;
  });
})();
