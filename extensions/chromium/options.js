/**
 * VelocityDM options page logic.
 */
/* global chrome, document */

(function () {
  'use strict';

  const FILTERS = window.VDMFilters;

  function el(id) { return document.getElementById(id); }

  function send(msg) {
    return new Promise(function (resolve) {
      chrome.runtime.sendMessage(msg, resolve);
    });
  }

  function toForm(settings) {
    const cats = el('cats').querySelectorAll('input[type=checkbox]');
    cats.forEach(function (c) {
      c.checked = (settings.interceptCategories || []).indexOf(c.value) !== -1;
    });
    el('min-size').value = String(settings.minSizeBytes / (1024 * 1024));
    el('excluded').value = (settings.excludedHosts || []).join('\n');
    el('always-exts').value = (settings.alwaysExts || []).join(', ');
  }

  function fromForm() {
    const cats = Array.from(el('cats').querySelectorAll('input[type=checkbox]'))
      .filter(function (c) { return c.checked; })
      .map(function (c) { return c.value; });
    return FILTERS.mergeSettings({
      interceptCategories: cats,
      minSizeBytes: Math.round(parseFloat(el('min-size').value || '1') * 1024 * 1024),
      excludedHosts: el('excluded').value.split(/\r?\n/).map(function (s) { return s.trim(); }).filter(Boolean),
      alwaysExts: el('always-exts').value.split(',').map(function (s) { return s.trim().toLowerCase(); }).filter(Boolean),
    });
  }

  el('save').addEventListener('click', async function () {
    await chrome.storage.local.set({ settings: fromForm() });
    const st = el('save-status');
    st.textContent = 'Saved';
    st.className = 'status ok';
    setTimeout(function () { st.textContent = ''; }, 2000);
  });

  el('reset').addEventListener('click', function () {
    toForm(FILTERS.mergeSettings(null));
  });

  el('test').addEventListener('click', function () {
    const st = el('test-status');
    st.textContent = 'Testing...';
    st.className = 'status';
    send({ type: 'nativePing' }).then(function (r) {
      if (r && r.ok) {
        st.textContent = 'Connected (engine ' + (r.version || '?') + ')';
        st.className = 'status ok';
      } else {
        st.textContent = 'Not reachable: ' + ((r && r.error) || 'no reply');
        st.className = 'status err';
      }
    });
  });

  chrome.storage.local.get('settings').then(function (stored) {
    toForm(FILTERS.mergeSettings(stored.settings));
  });
})();
