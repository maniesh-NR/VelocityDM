/**
 * VelocityDM shared module: the native-messaging host link (S11-i B4).
 *
 * Persistent port preferred (connectNative) with a one-shot
 * sendNativeMessage fallback; on disconnect an exponential backoff
 * (500 ms doubling to an 8 s cap, reset after 30 s of stable connection)
 * bounded by a 10-minute episode budget; commands issued while
 * disconnected are queued with a cap of 32 (drop-oldest) and a
 * first-send failure re-queues its command exactly once. Host-absence
 * error patterns classify the link as absent so the extension degrades
 * gracefully (browser downloads continue on their own).
 *
 * All orchestration logic lives here so the per-browser wiring stays a
 * few lines; every dependency is injectable for the node battery.
 * Commands and replies are never logged (cookies ride commands — B5
 * secrecy); only static error strings reach the log channel.
 */
(function (root, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else {
    root.VDMNMHost = factory();
  }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  var BACKOFF_START_MS = 500;
  var BACKOFF_CAP_MS = 8000;
  var STABLE_RESET_MS = 30000;
  var EPISODE_BUDGET_MS = 600000; // 10 minutes of reconnect attempts
  var QUEUE_CAP = 32;

  // Host-absence patterns (lowercase substring match). Ours, chosen from
  // the observed native-messaging error families across Chromium/Opera/
  // Edge/Firefox — "not found" (host manifest missing), "not installed",
  // "access is denied" (manifest ACL), "native host has exited".
  var HOST_ABSENCE_PATTERNS = [
    'not found',
    'not installed',
    'access is denied',
    'native host has exited',
  ];

  function nextBackoffMs(currentMs) {
    var next = (typeof currentMs === 'number' && currentMs > 0)
      ? currentMs * 2
      : BACKOFF_START_MS;
    return Math.min(next, BACKOFF_CAP_MS);
  }

  function shouldResetBackoff(stableMs) {
    return typeof stableMs === 'number' && stableMs >= STABLE_RESET_MS;
  }

  function isHostAbsence(text) {
    if (typeof text !== 'string' || !text) return false;
    var low = text.toLowerCase();
    for (var i = 0; i < HOST_ABSENCE_PATTERNS.length; i++) {
      if (low.indexOf(HOST_ABSENCE_PATTERNS[i]) !== -1) return true;
    }
    return false;
  }

  /** Bounded queue push: cap 32, drop-oldest. Returns {queue, dropped}. */
  function queuePush(queue, entry) {
    var q = queue.slice();
    var dropped = null;
    if (q.length >= QUEUE_CAP) dropped = q.shift(); // drop-oldest
    q.push(entry);
    return { queue: q, dropped: dropped };
  }

  function withinBudget(episodeStartMs, nowMs) {
    if (typeof episodeStartMs !== 'number') return true;
    return nowMs - episodeStartMs < EPISODE_BUDGET_MS;
  }

  function defaultNormalize(reply) {
    if (!reply || typeof reply !== 'object') {
      return { ok: false, error: 'malformed host reply' };
    }
    if (reply.ok === true) return reply;
    return { ok: false, error: reply.error || 'unknown host error' };
  }

  /**
   * Create the host link. deps (all injectable for the battery):
   *   hostName           native host name (e.g. 'com.velocitydm.nmh')
   *   connectNative      optional (name) -> port {postMessage, disconnect,
   *                      onMessage.addListener, onDisconnect.addListener}
   *   sendNativeMessage  (name, command, callback(reply)) — one-shot path
   *   now                () -> ms
   *   setTimeout         (fn, ms) -> handle; clearTimeout(handle)
   *   log                optional (msg) — static strings only
   *   normalize          optional reply normalizer
   */
  function createLink(deps) {
    deps = deps || {};
    var normalize = deps.normalize || defaultNormalize;
    var log = deps.log || function () {};
    var now = deps.now || function () { return Date.now(); };
    var timers = deps.setTimeout || setTimeout;
    var clearTimer = deps.clearTimeout || clearTimeout;

    var queue = [];        // [{command, requeued, resolve, reject}]
    var inFlight = null;  // entry while awaiting its reply
    var port = null;
    var state = 'idle';   // idle | connected | backoff | absent | dead
    var backoffMs = BACKOFF_START_MS;
    var episodeStartMs = null; // first reconnect attempt of this episode
    var connectedAtMs = null;  // for the 30 s stability reset
    var retryTimer = null;
    var retryTimerWaitMs = null; // the currently scheduled retry delay

    function currentState() { return state; }

    function lastErrorText() {
      try {
        if (typeof chrome !== 'undefined' && chrome.runtime &&
            chrome.runtime.lastError && chrome.runtime.lastError.message) {
          return String(chrome.runtime.lastError.message);
        }
      } catch (e) { /* not in a chrome context */ }
      return '';
    }

    // ---- queue -----------------------------------------------------------

    function rejectEntry(entry, errText) {
      if (entry && entry.reject) entry.reject({ ok: false, error: errText });
    }

    function enqueue(entry) {
      var pushed = queuePush(queue, entry);
      queue = pushed.queue;
      if (pushed.dropped) {
        log('NM host queue full: dropped the oldest queued command');
        rejectEntry(pushed.dropped, 'engine unreachable (queue full)');
      }
    }

    // ---- backoff / reconnect ------------------------------------------------

    function cancelRetry() {
      if (retryTimer !== null) {
        clearTimer(retryTimer);
        retryTimer = null;
        retryTimerWaitMs = null;
      }
    }

    function scheduleRetry() {
      if (retryTimer !== null) return;
      if (episodeStartMs === null) episodeStartMs = now();
      if (!withinBudget(episodeStartMs, now())) {
        state = 'dead';
        episodeStartMs = null;
        while (queue.length) rejectEntry(queue.shift(), 'engine unreachable');
        return;
      }
      var wait = backoffMs;
      backoffMs = nextBackoffMs(backoffMs);
      retryTimerWaitMs = wait;
      retryTimer = timers(function () {
        retryTimer = null;
        retryTimerWaitMs = null;
        connectOrDrain();
      }, wait);
    }

    /** A retry tick: re-connect the port path, or re-pump the one-shot. */
    function connectOrDrain() {
      if (typeof deps.connectNative === 'function') {
        // success: the queued commands drain through the port; a refusal
        // re-arms the backoff inside attemptConnect (failFast=false) until
        // the episode budget expires
        attemptConnect(false);
      } else {
        pumpOneShot(); // a failure inside re-arms the backoff
      }
    }

    function onStableDisconnected() {
      var stableMs = connectedAtMs === null ? null : now() - connectedAtMs;
      connectedAtMs = null;
      if (shouldResetBackoff(stableMs)) {
        // a stable connection closes the reconnect episode: the backoff
        // AND the 10-minute attempt budget both restart from here
        backoffMs = BACKOFF_START_MS;
        episodeStartMs = null;
      }
    }

    // ---- persistent port -----------------------------------------------------

    function wirePort(p) {
      p.onMessage.addListener(function (reply) {
        if (inFlight) {
          var entry = inFlight;
          inFlight = null;
          if (entry.resolve) entry.resolve(normalize(reply));
        }
        pumpPort();
      });
      p.onDisconnect.addListener(function (errTextArg) {
        if (port !== p) return; // stale port event
        onPortDown(typeof errTextArg === 'string' ? errTextArg : errTextOfDisconnect());
      });
    }

    function errTextOfDisconnect() {
      // the disconnect listener runs right after lastError is populated
      return lastErrorText();
    }

    function onPortDown(errText) {
      port = null;
      var absent = isHostAbsence(errText);
      // first-send failure: the in-flight command re-queues exactly once
      // (transient drop); host absence fails fast instead so the caller's
      // decline path (browser download resume) stays correct
      var failed = inFlight;
      inFlight = null;
      if (failed) {
        if (absent) {
          rejectEntry(failed, 'engine unreachable');
        } else if (!failed.requeued) {
          failed.requeued = true;
          queue.unshift(failed);
        } else {
          rejectEntry(failed, 'engine unreachable');
        }
      }

      if (absent) {
        // graceful degradation: no timer churn; queued commands fail
        // fast (the extension keeps working, browser downloads continue)
        state = 'absent';
        cancelRetry();
        while (queue.length) rejectEntry(queue.shift(), 'engine unreachable');
        return;
      }

      onStableDisconnected();
      state = 'backoff';
      scheduleRetry();
    }

    function refuseConnect(failFast) {
      if (failFast) {
        // probe refusal when the caller is issuing a command: the host is
        // gone — absent state, queued commands fail fast (never parked:
        // the caller's decline path stays correct)
        state = 'absent';
        while (queue.length) rejectEntry(queue.shift(), 'engine unreachable');
        return;
      }
      // refusal during a scheduled reconnect: one more failed attempt —
      // the backoff loop continues until the episode budget expires
      state = 'backoff';
      scheduleRetry();
    }

    function attemptConnect(failFast) {
      if (port) return true;
      var connect = deps.connectNative;
      if (typeof connect !== 'function') return false;
      var p;
      try {
        p = connect(deps.hostName);
      } catch (e) {
        refuseConnect(failFast);
        return false;
      }
      if (!p || !p.onMessage || !p.onDisconnect) {
        refuseConnect(failFast);
        return false;
      }
      port = p;
      state = 'connected';
      connectedAtMs = now();
      wirePort(p);
      pumpPort();
      return true;
    }

    function pumpPort() {
      if (!port || inFlight || queue.length === 0) return;
      var entry = queue.shift();
      inFlight = entry;
      try {
        port.postMessage(entry.command);
      } catch (e) {
        // port not usable: treat as a disconnect without waiting for the
        // event (which may never fire after a postMessage throw)
        inFlight = null;
        onPortDown('');
      }
    }

    // ---- one-shot fallback ------------------------------------------------

    function pumpOneShot() {
      if (inFlight || queue.length === 0) return;
      if (typeof deps.sendNativeMessage !== 'function') {
        while (queue.length) rejectEntry(queue.shift(), 'engine unreachable');
        return;
      }
      var entry = queue.shift();
      inFlight = entry;
      try {
        deps.sendNativeMessage(deps.hostName, entry.command, function (reply) {
          inFlight = null;
          if (reply) {
            if (entry.resolve) entry.resolve(normalize(reply));
            pumpOneShot();
            return;
          }
          // send failure: host absence fails fast (graceful
          // degradation); a transient failure re-queues exactly once and
          // retries on the backoff
          var errText = lastErrorText();
          if (isHostAbsence(errText)) {
            rejectEntry(entry, 'engine unreachable');
            state = 'absent';
            cancelRetry();
            while (queue.length) rejectEntry(queue.shift(), 'engine unreachable');
            return;
          }
          if (!entry.requeued) {
            entry.requeued = true;
            queue.unshift(entry);
          } else {
            rejectEntry(entry, 'engine unreachable');
          }
          onStableDisconnected();
          state = 'backoff';
          scheduleRetry();
        });
      } catch (e) {
        inFlight = null;
        if (!entry.requeued) {
          entry.requeued = true;
          queue.unshift(entry);
        } else {
          rejectEntry(entry, 'engine unreachable');
        }
        state = 'backoff';
        scheduleRetry();
      }
    }

    // ---- send -------------------------------------------------------------

    function send(command) {
      return new Promise(function (resolve, reject) {
        var entry = { command: command, requeued: false, resolve: resolve, reject: reject };
        if (state === 'dead') {
          reject({ ok: false, error: 'engine unreachable' });
          return;
        }
        if (typeof deps.connectNative === 'function') {
          if (state === 'idle' || state === 'absent') {
            attemptConnect(true); // first use, or an absent-host re-probe
          }
          if (port) {
            enqueue(entry);
            pumpPort();
          } else if (state === 'absent') {
            // probe refused again: fail fast, never park the command
            reject({ ok: false, error: 'engine unreachable' });
          } else {
            // backoff: queue until the scheduled retry reconnects
            enqueue(entry);
          }
          return;
        }
        // one-shot transport owns delivery
        if (state === 'idle') state = 'connected';
        enqueue(entry);
        pumpOneShot();
      });
    }

    return {
      send: send,
      state: currentState,
      _attemptConnect: attemptConnect, // test seam; wiring never uses it
      _nextDelayMs: function () {
        // test seam: the pending retry delay (null when no retry armed)
        return retryTimer === null ? null : retryTimerWaitMs;
      },
    };
  }

  return {
    BACKOFF_START_MS: BACKOFF_START_MS,
    BACKOFF_CAP_MS: BACKOFF_CAP_MS,
    STABLE_RESET_MS: STABLE_RESET_MS,
    EPISODE_BUDGET_MS: EPISODE_BUDGET_MS,
    QUEUE_CAP: QUEUE_CAP,
    nextBackoffMs: nextBackoffMs,
    shouldResetBackoff: shouldResetBackoff,
    isHostAbsence: isHostAbsence,
    queuePush: queuePush,
    withinBudget: withinBudget,
    createLink: createLink,
  };
});
