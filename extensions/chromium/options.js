/**
 * VelocityDM options page logic: intercept settings + Material You theme.
 */
/* global chrome, document */

(function () {
  'use strict';

  const FILTERS = window.VDMFilters;
  const MD3 = window.VDMMd3;

  function el(id) { return document.getElementById(id); }

  function send(msg) {
    return new Promise(function (resolve) {
      chrome.runtime.sendMessage(msg, resolve);
    });
  }

  // ------------------------------------------------------------------ theme
  function markSelectedSwatch(seed) {
    document.querySelectorAll('.swatch').forEach(function (b) {
      b.classList.toggle('sel', (b.dataset.seed || '').toLowerCase() === String(seed).toLowerCase());
    });
  }

  function markMode(mode) {
    document.querySelectorAll('#modes input[name=mode]').forEach(function (r) {
      r.checked = r.value === mode;
    });
  }

  MD3.initTheme().then(function (t) {
    markSelectedSwatch(t.seed);
    markMode(t.mode);
    el('custom-seed').value = t.seed;
  });

  document.querySelectorAll('.swatch').forEach(function (b) {
    b.addEventListener('click', function () {
      MD3.updateTheme({ seed: b.dataset.seed }).then(function (t) {
        markSelectedSwatch(t.seed);
        el('custom-seed').value = t.seed;
      });
    });
  });

  el('apply-seed').addEventListener('click', function () {
    const value = el('custom-seed').value.trim();
    if (MD3.parseHex(value)) {
      MD3.updateTheme({ seed: value }).then(function (t) {
        markSelectedSwatch(t.seed);
      });
    } else {
      el('custom-seed').value = '';
    }
  });

  document.querySelectorAll('#modes input[name=mode]').forEach(function (r) {
    r.addEventListener('change', function () {
      if (r.checked) MD3.updateTheme({ mode: r.value });
    });
  });

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
