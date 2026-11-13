/**
 * VelocityDM shared module: Material You (Material Design 3) dynamic color.
 *
 * Seed color -> tonal palettes -> MD3 light/dark scheme, applied to the page
 * as CSS custom properties (--md-*). The same generation model as the
 * desktop egui GUI (crates/velocitydm/src/theme.rs) and the Android
 * dynamicColorScheme fallback: tones are CIELAB L* with chroma clipped to
 * the sRGB gamut at constant tone.
 *
 * Theme prefs live in chrome.storage.local under "theme":
 *   { seed: "#4156A4", mode: "system" | "light" | "dark" }
 */
(function (root, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else {
    root.VDMMd3 = factory();
  }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  // ---------------------------------------------------------------- math
  function srgbToLinear(c) {
    c /= 255;
    return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  }
  function linearToSrgb(c) {
    return c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055;
  }
  function linearRgbToXyz(r, g, b) {
    return [
      0.4124564 * r + 0.3575761 * g + 0.1804375 * b,
      0.2126729 * r + 0.7151522 * g + 0.072175 * b,
      0.0193339 * r + 0.119192 * g + 0.9503041 * b,
    ];
  }
  function xyzToLinearRgb(x, y, z) {
    return [
      3.2404542 * x - 1.5371385 * y - 0.4985314 * z,
      -0.969266 * x + 1.8760108 * y + 0.041556 * z,
      0.0556434 * x - 0.2040259 * y + 1.0572252 * z,
    ];
  }
  var D65 = [0.95047, 1.0, 1.08883];
  function labF(t) {
    return t > 216 / 24389 ? Math.cbrt(t) : (841 / 108) * t + 4 / 29;
  }
  function labInvF(t) {
    var t3 = t * t * t;
    return t3 > 216 / 24389 ? t3 : (108 / 841) * (t - 4 / 29);
  }
  function rgbToLab(rgb) {
    var lin = [rgb[0], rgb[1], rgb[2]].map(srgbToLinear);
    var xyz = linearRgbToXyz(lin[0], lin[1], lin[2]);
    var fx = labF(xyz[0] / D65[0]);
    var fy = labF(xyz[1] / D65[1]);
    var fz = labF(xyz[2] / D65[2]);
    return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)];
  }
  function labToLinearRgb(L, a, b) {
    var fy = (L + 16) / 116;
    var fx = fy + a / 500;
    var fz = fy - b / 200;
    return xyzToLinearRgb(labInvF(fx) * D65[0], labInvF(fy) * D65[1], labInvF(fz) * D65[2]);
  }
  function inGamut(L, a, b) {
    var rgb = labToLinearRgb(L, a, b);
    return rgb.every(function (c) { return c >= 0 && c <= 1; });
  }
  function labToHex(L, a, b) {
    return labToLinearRgb(L, a, b)
      .map(function (c) {
        var s = linearToSrgb(Math.min(1, Math.max(0, c)));
        return ('0' + Math.round(s * 255).toString(16)).slice(-2);
      })
      .join('');
  }

  /**
   * Tone-preserving tonal palette value: fixed hue (degrees) + chroma,
   * tone t in 0..100 (CIELAB L*). Chroma is clipped into the sRGB gamut
   * via binary search, exactly like the Rust implementation.
   */
  function tonal(hueDeg, chroma, tone) {
    var rad = (hueDeg * Math.PI) / 180;
    var a = chroma * Math.cos(rad);
    var b = chroma * Math.sin(rad);
    if (inGamut(tone, a, b)) return '#' + labToHex(tone, a, b);
    var lo = 0, hi = 1;
    for (var i = 0; i < 24; i++) {
      var mid = (lo + hi) / 2;
      if (inGamut(tone, a * mid, b * mid)) lo = mid;
      else hi = mid;
    }
    return '#' + labToHex(tone, a * lo, b * lo);
  }

  // ------------------------------------------------------------- schemes
  var PRESETS = [
    ['brand', '#4156A4'],
    ['violet', '#6750A4'],
    ['ocean', '#00639B'],
    ['teal', '#006A6A'],
    ['forest', '#386A33'],
    ['sunset', '#8F4C00'],
    ['rose', '#984061'],
    ['slate', '#56606F'],
  ];

  function parseHex(s) {
    if (typeof s !== 'string') return null;
    var h = s.trim().replace(/^#/, '');
    if (!/^[0-9a-fA-F]{6}$/.test(h)) return null;
    return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
  }

  var DEFAULT_SEED = PRESETS[0][1];

  /**
   * Full MD3 scheme from a seed color. Role names mirror the spec; returned
   * as hex strings. `dark` selects the dark variant.
   */
  function schemeFromSeed(seedHex, dark) {
    var rgb = parseHex(seedHex) || parseHex(DEFAULT_SEED);
    var lab = rgbToLab(rgb);
    var hue = (Math.atan2(lab[2], lab[1]) * 180) / Math.PI;
    var seedChroma = Math.sqrt(lab[1] * lab[1] + lab[2] * lab[2]);

    var P = function (t) { return tonal(hue, Math.max(48, seedChroma), t); };
    var S = function (t) { return tonal(hue, 16, t); };
    var T = function (t) { return tonal(hue + 60, 24, t); };
    var N = function (t) { return tonal(hue, 4, t); };
    var V = function (t) { return tonal(hue, 8, t); };
    var E = function (t) { return tonal(31, 55, t); };
    var G = function (t) { return tonal(150, 40, t); };

    if (dark) {
      return {
        primary: P(80), onPrimary: P(20), primaryContainer: P(30), onPrimaryContainer: P(90),
        secondary: S(80), onSecondary: S(20), secondaryContainer: S(30), onSecondaryContainer: S(90),
        tertiary: T(80), onTertiary: T(20), tertiaryContainer: T(30), onTertiaryContainer: T(90),
        error: E(80), onError: E(20), errorContainer: E(30), onErrorContainer: E(90),
        success: G(80), onSuccess: G(20),
        background: N(6), onBackground: N(90),
        surface: N(6), onSurface: N(90),
        surfaceVariant: V(30), onSurfaceVariant: V(80),
        surfaceContainer: N(12), surfaceContainerHigh: N(15), surfaceContainerHighest: N(17),
        outline: V(60), outlineVariant: V(30),
      };
    }
    return {
      primary: P(40), onPrimary: P(100), primaryContainer: P(90), onPrimaryContainer: P(10),
      secondary: S(40), onSecondary: S(100), secondaryContainer: S(90), onSecondaryContainer: S(10),
      tertiary: T(40), onTertiary: T(100), tertiaryContainer: T(90), onTertiaryContainer: T(10),
      error: E(40), onError: E(100), errorContainer: E(90), onErrorContainer: E(10),
      success: G(40), onSuccess: G(100),
      background: N(98), onBackground: N(10),
      surface: N(98), onSurface: N(10),
      surfaceVariant: V(90), onSurfaceVariant: V(30),
      surfaceContainer: N(94), surfaceContainerHigh: N(92), surfaceContainerHighest: N(90),
      outline: V(50), outlineVariant: V(80),
    };
  }

  // --------------------------------------------------------------- theme
  function cssVarName(role) {
    return '--md-' + role.replace(/([A-Z])/g, '-$1').toLowerCase();
  }

  function resolveDark(mode) {
    if (mode === 'light') return false;
    if (mode === 'dark') return true;
    try {
      return window.matchMedia('(prefers-color-scheme: dark)').matches;
    } catch (e) {
      return true;
    }
  }

  /** Applies the scheme as CSS custom properties on :root. */
  function applySchemeToDocument(scheme, dark) {
    var doc = document.documentElement;
    Object.keys(scheme).forEach(function (role) {
      doc.style.setProperty(cssVarName(role), scheme[role]);
    });
    doc.setAttribute('data-theme', dark ? 'dark' : 'light');
  }

  function storage() {
    return typeof chrome !== 'undefined' && chrome.storage && chrome.storage.local
      ? chrome.storage.local
      : null;
  }

  /** Reads persisted theme prefs (falls back to defaults). */
  function getTheme() {
    return new Promise(function (resolve) {
      var st = storage();
      if (!st) return resolve({ seed: DEFAULT_SEED, mode: 'system' });
      st.get('theme', function (res) {
        var t = (res && res.theme) || {};
        resolve({
          seed: parseHex(t.seed) ? t.seed : DEFAULT_SEED,
          mode: ['system', 'light', 'dark'].indexOf(t.mode) !== -1 ? t.mode : 'system',
        });
      });
    });
  }

  /** Persists theme prefs. */
  function setTheme(prefs) {
    return new Promise(function (resolve) {
      var st = storage();
      var t = {
        seed: parseHex(prefs.seed) ? prefs.seed : DEFAULT_SEED,
        mode: ['system', 'light', 'dark'].indexOf(prefs.mode) !== -1 ? prefs.mode : 'system',
      };
      if (!st) return resolve(t);
      st.set({ theme: t }, function () { resolve(t); });
    });
  }

  /** Loads prefs and applies them (popup/options entry point). */
  function initTheme() {
    return getTheme().then(function (t) {
      var dark = resolveDark(t.mode);
      applySchemeToDocument(schemeFromSeed(t.seed, dark), dark);
      return t;
    });
  }

  /** Applies prefs now and persists them (theme control handler). */
  function updateTheme(prefs) {
    var merged = {};
    return getTheme()
      .then(function (cur) {
        merged = { seed: prefs.seed || cur.seed, mode: prefs.mode || cur.mode };
        return setTheme(merged);
      })
      .then(function (t) {
        var dark = resolveDark(t.mode);
        applySchemeToDocument(schemeFromSeed(t.seed, dark), dark);
        return t;
      });
  }

  return {
    PRESETS: PRESETS,
    DEFAULT_SEED: DEFAULT_SEED,
    parseHex: parseHex,
    rgbToLab: rgbToLab,
    tonal: tonal,
    schemeFromSeed: schemeFromSeed,
    cssVarName: cssVarName,
    resolveDark: resolveDark,
    applySchemeToDocument: applySchemeToDocument,
    getTheme: getTheme,
    setTheme: setTheme,
    initTheme: initTheme,
    updateTheme: updateTheme,
  };
});
