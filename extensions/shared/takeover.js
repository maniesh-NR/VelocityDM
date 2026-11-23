/**
 * VelocityDM shared module: download-takeover UX (S11-i B6).
 *
 * Shelf suppression while takeover is enabled (feature-detected;
 * Firefox no-ops), filename participation via downloads.
 * onDeterminingFilename (suggest our filename once, on our takeover —
 * never fighting a user rename), the decline path surfaced to the
 * popup's error note with our static words, and the static in-tab
 * confirmation page (taken.html) used by content-script video
 * takeovers: the hash fragment is read once and rendered via
 * textContent only — never innerHTML, never a location assignment from
 * the fragment — then the page goes back immediately.
 *
 * All logic lives here (gate 0007 INT: shared modules, thin wiring);
 * every dependency is injectable for the node battery.
 */
(function (root, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else {
    root.VDMTakeover = factory();
  }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  // Static decline notes — our words, chosen once.
  var NOTE_UNREACHABLE =
    'VelocityDM could not be reached, so the browser is finishing this download itself.';
  var NOTE_DECLINED =
    'VelocityDM declined this download; the browser is finishing it itself.';

  /** Takeover is enabled in settings when any intercept category is on. */
  function isEnabled(settings) {
    return !!(settings && Array.isArray(settings.interceptCategories) &&
      settings.interceptCategories.length > 0);
  }

  /**
   * The engine-side filename mirror: the basename of the browser item's
   * filename (falling back to the URL basename); when that name has NO
   * extension and the MIME table resolves one, the extension is appended
   * (the engine's own rule never overrides an explicit filename or an
   * existing extension — this mirrors it for the browser item).
   */
  function filenameFor(item, extForMime) {
    item = item || {};
    var name = '';
    if (typeof item.filename === 'string' && item.filename) {
      var slash = item.filename.lastIndexOf('/');
      name = slash === -1 ? item.filename : item.filename.substring(slash + 1);
    }
    if (!name && typeof item.url === 'string') {
      try {
        var path = item.url.split('#')[0].split('?')[0];
        name = path.substring(path.lastIndexOf('/') + 1);
      } catch (e) { name = ''; }
    }
    if (!name) return '';
    var dot = name.lastIndexOf('.');
    var hasExt = dot > 0 && dot < name.length - 1;
    if (!hasExt && typeof extForMime === 'function' && typeof item.mime === 'string') {
      var ext = extForMime(item.mime);
      if (ext) name = name + '.' + ext;
    }
    return name;
  }

  /** The static decline note for a normalized host reply. */
  function noteFor(normalized) {
    if (normalized && normalized.ok) return '';
    if (!normalized || !normalized.error) return NOTE_DECLINED;
    var low = String(normalized.error).toLowerCase();
    if (low.indexOf('unreachable') !== -1 || low.indexOf('malformed') !== -1) {
      return NOTE_UNREACHABLE;
    }
    return NOTE_DECLINED;
  }

  /** The taken.html navigation target for a content-script video takeover. */
  function confirmTarget(url, baseUrl) {
    return baseUrl + '#' + encodeURIComponent(url);
  }

  /**
   * The taken.html render rule: read the hash ONCE, decode it, render via
   * textContent only. Returns the decoded URL (or null when the hash is
   * empty). The page script calls this; never innerHTML.
   */
  function renderConfirmInto(el, hash) {
    if (!el || typeof hash !== 'string' || hash.length < 2) return null;
    var raw = hash.substring(1);
    var decoded;
    try {
      decoded = decodeURIComponent(raw);
    } catch (e) {
      decoded = raw;
    }
    el.textContent = decoded;
    return decoded;
  }

  /**
   * The hand-off orchestration (the downloads-pipeline takeover path).
   * deps (all injectable):
   *   getSettings () -> Promise<settings>
   *   shouldTakeOver (probe, settings) -> bool
   *   withJar (item, jarApi) -> Promise<item>       (B5 helper)
   *   jarApi        the cookies API (or null)
   *   send (command) -> Promise<normalizedReply>    (B4 hostSend)
   *   downloads     { pause(id), cancel(id), erase(id), resume(id) } —
   *                 each returning a promise
   *   note (text, url) -> void                       (decline note sink)
   *   extForMime (mime) -> ext|null                 (B8-js wires the
   *                                                 real table later)
   *   onTaken (item, command) -> void               (accept hook)
   * Returns 'taken' | 'declined' | 'skipped'.
   */
  function handOff(item, deps) {
    deps = deps || {};
    return Promise.resolve()
      .then(function () { return deps.getSettings ? deps.getSettings() : {}; })
      .then(function (settings) {
        var filename = filenameFor(item, deps.extForMime);
        var probe = {
          url: item.url,
          filename: filename,
          mime: item.mime,
          fileSize: item.fileSize || item.totalBytes || 0,
          byExtensionId: item.byExtensionId,
        };
        if (!deps.shouldTakeOver || !deps.shouldTakeOver(probe, settings)) {
          return 'skipped';
        }
        var pause = deps.downloads && deps.downloads.pause;
        var paused = (typeof pause === 'function')
          ? pause(item.id).catch(function () { /* already done */ })
          : Promise.resolve();
        return paused.then(function () {
          var ready = {
            url: item.url,
            filename: filename,
            referer: item.referrer || '',
          };
          var jarStep = deps.withJar
            ? deps.withJar(ready, deps.jarApi)
            : Promise.resolve(ready);
          return jarStep.then(function (withCookies) {
            var command = deps.buildAdd
              ? deps.buildAdd(withCookies)
              : withCookies;
            return deps.send(command).then(function (normalized) {
              if (normalized && normalized.ok) {
                var d = deps.downloads || {};
                if (typeof d.cancel === 'function') d.cancel(item.id).catch(function () {});
                if (typeof d.erase === 'function') d.erase({ id: item.id }).catch(function () {});
                if (typeof deps.onTaken === 'function') deps.onTaken(item, command);
                return 'taken';
              }
              if (typeof (deps.downloads || {}).resume === 'function') {
                deps.downloads.resume(item.id).catch(function () {});
              }
              if (typeof deps.note === 'function') {
                deps.note(noteFor(normalized), item.url);
              }
              return 'declined';
            });
          });
        });
      });
  }

  return {
    NOTE_UNREACHABLE: NOTE_UNREACHABLE,
    NOTE_DECLINED: NOTE_DECLINED,
    isEnabled: isEnabled,
    filenameFor: filenameFor,
    noteFor: noteFor,
    confirmTarget: confirmTarget,
    renderConfirmInto: renderConfirmInto,
    handOff: handOff,
  };
});
