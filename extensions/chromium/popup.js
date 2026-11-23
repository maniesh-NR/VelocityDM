/**
 * VelocityDM popup: sniffed videos, quick add, live download list.
 * Material You dynamic color is initialized before the first render.
 */
/* global chrome, document */

(function () {
  'use strict';

  const MSG = window.VDMMessaging;
  const PAT = window.VDMPatterns;
  const MD3 = window.VDMMd3;
  let grabbedGroups = null;

  function el(id) { return document.getElementById(id); }

  function send(msg) {
    return new Promise(function (resolve) {
      chrome.runtime.sendMessage(msg, resolve);
    });
  }

  // ------------------------------------------------------------------ theme
  const MODE_LABELS = { system: 'auto', light: 'light', dark: 'dark' };
  const MODE_CYCLE = { system: 'light', light: 'dark', dark: 'system' };

  function markSelectedSwatch(seed) {
    document.querySelectorAll('.swatch').forEach(function (b) {
      b.classList.toggle('sel', (b.dataset.seed || '').toLowerCase() === String(seed).toLowerCase());
    });
  }

  function setModeLabel(mode) {
    const btn = el('btn-mode');
    if (btn) btn.textContent = MODE_LABELS[mode] || 'auto';
  }

  function initThemeControls() {
    document.querySelectorAll('.swatch').forEach(function (b) {
      b.addEventListener('click', function () {
        MD3.updateTheme({ seed: b.dataset.seed }).then(function (t) {
          markSelectedSwatch(t.seed);
        });
      });
    });
    const modeBtn = el('btn-mode');
    if (modeBtn) {
      modeBtn.addEventListener('click', function () {
        MD3.getTheme().then(function (cur) {
          const next = MODE_CYCLE[cur.mode] || 'system';
          MD3.updateTheme({ mode: next }).then(function (t) {
            setModeLabel(t.mode);
          });
        });
      });
    }
  }

  MD3.initTheme().then(function (t) {
    markSelectedSwatch(t.seed);
    setModeLabel(t.mode);
  });
  initThemeControls();

  // ------------------------------------------------------------------ videos
  function renderVideos(videos) {
    const box = el('videos');
    box.textContent = '';
    if (!videos || videos.length === 0) {
      const d = document.createElement('div');
      d.className = 'empty';
      d.textContent = 'No videos detected yet. Play a video and reopen this popup.';
      box.appendChild(d);
      return;
    }
    videos.slice(0, 10).forEach(function (v) {
      const item = document.createElement('div');
      item.className = 'item';
      const row1 = document.createElement('div');
      row1.className = 'row1';
      const kind = document.createElement('span');
      kind.className = 'kind';
      kind.textContent = v.kind || 'file';
      const title = document.createElement('span');
      title.className = 'title';
      title.textContent = v.title || v.url;
      title.title = v.url;
      const quality = document.createElement('span');
      quality.className = 'quality';
      quality.textContent = v.quality || '';
      row1.append(kind, title, quality);
      const url = document.createElement('div');
      url.className = 'url';
      url.textContent = v.url;
      const btn = document.createElement('button');
      btn.textContent = 'Download';
      btn.addEventListener('click', function () {
        send({
          type: 'addDownload',
          item: {
            url: v.url,
            referer: v.pageUrl,
            quality: v.quality,
            filename: v.filename || '', // B8-js hint when the URL has no ext
          },
        });
        btn.textContent = 'Added';
        btn.disabled = true;
      });
      item.append(row1, url, btn);
      box.appendChild(item);
    });
  }

  // S11-i B7: a popup-open detection pass asks the active tab's content
  // script for DOM candidates and merges them into the displayed list
  function detectDomVideos() {
    chrome.tabs.query({ active: true, currentWindow: true }, function (tabs) {
      const tab = tabs && tabs[0];
      if (!tab || tab.id == null) return;
      chrome.tabs.sendMessage(tab.id, { type: 'detectVideos' }, function (res) {
        void chrome.runtime.lastError; // no content script on this page
        if (!res || !res.ok || !Array.isArray(res.videos)) return;
        // merge into the stored list via the background (kind 'dom'),
        // then render the union so the pass is visible immediately
        res.videos.slice(0, 10).forEach(function (v) {
          if (v && v.url && /^https?:/i.test(v.url)) {
            chrome.runtime.sendMessage({ type: 'domVideos', videos: [v] }, function () {
              void chrome.runtime.lastError;
            });
          }
        });
        send({ type: 'getVideos' }).then(function (r) {
          const stored = (r && r.videos) || [];
          const seen = {};
          stored.forEach(function (v) { seen[v.url] = true; });
          const fresh = res.videos.filter(function (v) {
            return v && v.url && /^https?:/i.test(v.url) && !seen[v.url];
          });
          renderVideos(stored.concat(fresh));
        });
      });
    });
  }

  // -------------------------------------------------------------- downloads
  function renderDownloads(reply) {
    const box = el('downloads');
    box.textContent = '';
    if (!reply || !reply.ok) {
      const d = document.createElement('div');
      d.className = 'empty';
      d.textContent = reply && reply.error
        ? 'Daemon unreachable: ' + reply.error
        : 'No downloads.';
      box.appendChild(d);
      return;
    }
    const tasks = (reply.tasks || []).slice(0, 8);
    if (tasks.length === 0) {
      const d = document.createElement('div');
      d.className = 'empty';
      d.textContent = 'No active downloads.';
      box.appendChild(d);
      return;
    }
    tasks.forEach(function (t) {
      const item = document.createElement('div');
      item.className = 'item';
      const row1 = document.createElement('div');
      row1.className = 'row1';
      const state = document.createElement('span');
      state.className = 'state ' + (t.state || 'queued');
      state.textContent = t.state || '';
      const title = document.createElement('span');
      title.className = 'title';
      title.textContent = t.name || '#' + t.id;
      row1.append(state, title);
      const bar = document.createElement('div');
      bar.className = 'bar';
      const fill = document.createElement('div');
      fill.style.width = (t.size > 0 ? Math.round(100 * (t.done || 0) / t.size) : 0) + '%';
      bar.appendChild(fill);
      item.append(row1, bar);
      box.appendChild(item);
    });
  }

  // ------------------------------------------------------------------- add
  function quickAdd() {
    const value = el('add-url').value.trim();
    if (!value) return;
    if (PAT.isMagnet(value)) {
      send({ type: 'addTorrent', magnet: value });
    } else if (value.indexOf('[') !== -1 && value.indexOf(']') !== -1) {
      send({ type: 'addBatch', pattern: value });
    } else if (PAT.isHttpUrl(value)) {
      send({ type: 'addDownload', item: { url: value } });
    } else {
      el('add-url').value = '';
      return;
    }
    el('add-url').value = '';
  }

  // ------------------------------------------------------------------ grab
  async function grab() {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab || tab.id == null) return;
    chrome.tabs.sendMessage(tab.id, { type: 'grabLinks' }, function (groups) {
      if (chrome.runtime.lastError || !groups) {
        el('btn-grab-add').style.display = 'none';
        return;
      }
      grabbedGroups = groups;
      const count = Object.keys(groups).reduce(function (n, k) { return n + groups[k].length; }, 0);
      const btn = el('btn-grab-add');
      btn.style.display = count > 0 ? 'block' : 'none';
      btn.textContent = 'Download all ' + count + ' grabbed links';
    });
  }

  function addAllGrabbed() {
    if (!grabbedGroups) return;
    const urls = [];
    Object.keys(grabbedGroups).forEach(function (k) {
      grabbedGroups[k].forEach(function (e) { urls.push(e.url); });
    });
    if (urls.length) send({ type: 'addBatch', urls: urls });
    el('btn-grab-add').style.display = 'none';
  }

  // ---------------------------------------------------------------- notes
  // takeover decline notes (B6) and the engine-unreachable state (B4);
  // rendered via DOM nodes only, from our static strings
  function renderNotes(state, note) {
    const area = el('note-area');
    const text = el('note-text');
    const dismiss = el('note-dismiss');
    let lead = '';
    let rest = '';
    let dismissible = false;
    if (state && state.state && state.state !== 'connected' && state.state !== 'idle') {
      lead = 'Engine unreachable.';
      rest = ' The browser\u2019s own downloads continue.';
    } else if (note && note.text) {
      rest = ' ' + String(note.text);
      dismissible = true;
    } else {
      area.style.display = 'none';
      return;
    }
    text.textContent = '';
    if (lead) {
      const strong = document.createElement('b');
      strong.textContent = lead;
      text.appendChild(strong);
    }
    text.appendChild(document.createTextNode(rest));
    area.style.display = 'block';
    dismiss.style.display = dismissible ? 'inline-block' : 'none';
  }

  function pollNotes() {
    send({ type: 'getTakeoverNote' }).then(function (n) {
      send({ type: 'hostState' }).then(function (s) {
        renderNotes(s, n && n.note);
      });
    });
  }

  el('note-dismiss').addEventListener('click', function () {
    send({ type: 'clearTakeoverNote' }).then(function () { pollNotes(); });
  });

  // ------------------------------------------------------------------ wire
  el('btn-add').addEventListener('click', quickAdd);
  el('add-url').addEventListener('keydown', function (e) {
    if (e.key === 'Enter') quickAdd();
  });
  el('btn-grab').addEventListener('click', grab);
  el('btn-grab-add').addEventListener('click', addAllGrabbed);
  el('btn-options').addEventListener('click', function () { chrome.runtime.openOptionsPage(); });
  el('open-options').addEventListener('click', function (e) {
    e.preventDefault(); chrome.runtime.openOptionsPage();
  });
  el('open-gui').addEventListener('click', function (e) {
    e.preventDefault();
    chrome.runtime.sendNativeMessage('com.velocitydm.nmh', MSG.open(), function () {});
  });

  (function poll() {
    send({ type: 'getVideos' }).then(function (r) { renderVideos(r && r.videos); });
    send({ type: 'getDownloads' }).then(renderDownloads);
    pollNotes();
    detectDomVideos(); // S11-i B7: popup-open detection pass
  })();
  const timer = setInterval(function () {
    send({ type: 'getDownloads' }).then(renderDownloads);
    pollNotes();
  }, 1000);
  window.addEventListener('unload', function () { clearInterval(timer); });
})();
