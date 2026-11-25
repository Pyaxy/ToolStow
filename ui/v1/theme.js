/* ==========================================================================
   ToolStow Shared Theme Bridge & Sandbox Helpers (/ui/v1/theme.js)
   Load synchronously in <head> before rendering to eliminate theme FOUC.
   Uses safe DOM APIs only and avoids browser storage dependencies.
   ========================================================================== */

(function () {
  "use strict";

  var ROOT = document.documentElement;
  var TOKEN_KEY_RE = /^--ts-[a-z0-9-]+$/;
  var UNSAFE_VAL_RE = /[;{}]|url\s*\(|expression\s*\(|@import/i;

  function detectEmbedded() {
    try {
      return Boolean(window.parent && window.parent !== window);
    } catch (_) {
      return true;
    }
  }

  function normalizeMode(value) {
    if (typeof value !== "string") return null;
    var v = value.trim().toLowerCase();
    if (v === "dark" || v === "light") return v;
    return null;
  }

  function normalizeHexColor(value) {
    if (typeof value !== "string") return null;
    var trimmed = value.trim();
    if (/^#[0-9a-fA-F]{6}$/.test(trimmed)) {
      return trimmed.toLowerCase();
    }
    var shortMatch = trimmed.match(/^#([0-9a-fA-F])([0-9a-fA-F])([0-9a-fA-F])$/);
    if (shortMatch) {
      return (
        "#" +
        shortMatch[1] +
        shortMatch[1] +
        shortMatch[2] +
        shortMatch[2] +
        shortMatch[3] +
        shortMatch[3]
      ).toLowerCase();
    }
    return null;
  }

  function isSafeTokenValue(value) {
    if (typeof value !== "string") return false;
    var trimmed = value.trim();
    if (!trimmed || trimmed.length > 256) return false;
    if (UNSAFE_VAL_RE.test(trimmed)) return false;
    return true;
  }

  function defaultAccentForMode(mode) {
    return mode === "dark" ? "#fafafa" : "#18181b";
  }

  var isEmbedded = detectEmbedded();
  ROOT.dataset.embed = isEmbedded ? "true" : "false";

  var params = null;
  try {
    params = new URLSearchParams(window.location.search);
  } catch (_) {
    params = null;
  }

  var queryMode = params ? normalizeMode(params.get("theme")) : null;
  var queryAccent = params ? normalizeHexColor(params.get("accent")) : null;

  var mediaQuery = null;
  try {
    if (typeof window.matchMedia === "function") {
      mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    }
  } catch (_) {
    mediaQuery = null;
  }

  var initialMode = queryMode || (mediaQuery && mediaQuery.matches ? "dark" : "light");

  var state = {
    mode: initialMode,
    accent: queryAccent || defaultAccentForMode(initialMode),
    explicitAccent: queryAccent,
    embedded: isEmbedded,
    tokens: {},
  };

  var appliedTokenKeys = new Set();
  var listeners = new Set();
  var toastEl = null;
  var toastTimer = null;

  // Apply initial theme synchronously to prevent dark-mode first-frame FOUC.
  ROOT.dataset.theme = state.mode;
  if (state.explicitAccent) {
    ROOT.style.setProperty("--ts-accent", state.explicitAccent);
  }

  function getTheme() {
    return {
      mode: state.mode,
      accent: state.accent,
      embedded: state.embedded,
      tokens: Object.assign({}, state.tokens),
    };
  }

  function notifyThemeChange() {
    var snapshot = getTheme();
    listeners.forEach(function (fn) {
      try {
        fn(snapshot);
      } catch (_) {
        // Isolate listener errors.
      }
    });
    try {
      window.dispatchEvent(new CustomEvent("toolstow:themechange", { detail: snapshot }));
    } catch (_) {
      // Ignore CustomEvent issues in restricted environments.
    }
  }

  function applyThemePayload(payload) {
    if (!payload || typeof payload !== "object") return;

    var nextMode = normalizeMode(payload.mode);
    if (nextMode) {
      state.mode = nextMode;
      ROOT.dataset.theme = nextMode;
    }

    // Apply optional validated --ts-* token overrides.
    if (payload.tokens && typeof payload.tokens === "object") {
      var nextTokens = {};
      var keys = Object.keys(payload.tokens);
      for (var i = 0; i < keys.length; i++) {
        var key = keys[i];
        var rawVal = payload.tokens[key];
        if (!TOKEN_KEY_RE.test(key) || !isSafeTokenValue(rawVal)) continue;
        var cleanVal = rawVal.trim();
        ROOT.style.setProperty(key, cleanVal);
        nextTokens[key] = cleanVal;
      }
      appliedTokenKeys.forEach(function (prevKey) {
        if (
          !Object.prototype.hasOwnProperty.call(nextTokens, prevKey) &&
          prevKey !== "--ts-accent"
        ) {
          ROOT.style.removeProperty(prevKey);
        }
      });
      appliedTokenKeys = new Set(Object.keys(nextTokens));
      state.tokens = nextTokens;
    } else if (appliedTokenKeys.size > 0) {
      // Clear previously applied inline tokens when host omits tokens map.
      appliedTokenKeys.forEach(function (key) {
        if (key !== "--ts-accent") {
          ROOT.style.removeProperty(key);
        }
      });
      appliedTokenKeys.clear();
      state.tokens = {};
    }

    var nextAccent = normalizeHexColor(payload.accent);
    if (nextAccent) {
      state.explicitAccent = nextAccent;
      state.accent = nextAccent;
      ROOT.style.setProperty("--ts-accent", nextAccent);
    } else if (!state.explicitAccent) {
      var tokenAccent = normalizeHexColor(state.tokens["--ts-accent"]);
      state.accent = tokenAccent || defaultAccentForMode(state.mode);
    }

    notifyThemeChange();
  }

  // Subscribe to OS theme changes when opened standalone (without explicit ?theme= override).
  if (!isEmbedded && !queryMode && mediaQuery) {
    var handleMediaChange = function (e) {
      var nextMode = e && e.matches ? "dark" : "light";
      state.mode = nextMode;
      ROOT.dataset.theme = nextMode;
      if (!state.explicitAccent) {
        state.accent = defaultAccentForMode(nextMode);
      }
      notifyThemeChange();
    };
    if (typeof mediaQuery.addEventListener === "function") {
      mediaQuery.addEventListener("change", handleMediaChange);
    } else if (typeof mediaQuery.addListener === "function") {
      mediaQuery.addListener(handleMediaChange);
    }
  }

  // Listen for live theme updates from the ToolStow host shell.
  window.addEventListener("message", function (event) {
    if (!event || !event.data || typeof event.data !== "object") return;
    if (event.data.type !== "toolstow:theme") return;
    applyThemePayload(event.data.payload);
  });

  // Notify host shell that this plugin is ready to receive theme sync.
  if (isEmbedded) {
    try {
      window.parent.postMessage({ type: "toolstow:ready" }, "*");
    } catch (_) {
      // Ignore postMessage failures.
    }
  }

  function onThemeChange(callback) {
    if (typeof callback !== "function") {
      return function () {};
    }
    listeners.add(callback);
    return function unsubscribe() {
      listeners.delete(callback);
    };
  }

  function execTextareaCopy(text) {
    if (!document.body) return false;
    var textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.setAttribute("readonly", "");
    textarea.setAttribute("aria-hidden", "true");
    textarea.style.position = "fixed";
    textarea.style.top = "-9999px";
    textarea.style.left = "-9999px";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);

    var ok = false;
    try {
      textarea.focus({ preventScroll: true });
      textarea.select();
      textarea.setSelectionRange(0, text.length);
      ok = Boolean(document.execCommand && document.execCommand("copy"));
    } catch (_) {
      ok = false;
    } finally {
      if (textarea.parentNode) {
        textarea.parentNode.removeChild(textarea);
      }
    }
    return ok;
  }

  function selectFallbackElement(fallbackEl) {
    if (!fallbackEl || typeof fallbackEl !== "object" || !fallbackEl.nodeType) {
      return false;
    }
    try {
      var range = document.createRange();
      range.selectNodeContents(fallbackEl);
      var selection = window.getSelection ? window.getSelection() : null;
      if (!selection) return false;
      selection.removeAllRanges();
      selection.addRange(range);
      return Boolean(document.execCommand && document.execCommand("copy"));
    } catch (_) {
      return false;
    }
  }

  function copy(text, fallbackEl) {
    var str = String(text == null ? "" : text);

    // Tier 1: Async Clipboard API (when permitted by permissions/sandbox).
    if (navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
      return navigator.clipboard
        .writeText(str)
        .then(function () {
          return true;
        })
        .catch(function () {
          // Tier 2: Temporary textarea + execCommand("copy").
          if (execTextareaCopy(str)) return true;
          // Tier 3: Optional DOM Range selection fallback.
          return selectFallbackElement(fallbackEl);
        });
    }

    // Tier 2 -> Tier 3 synchronously wrapped in a Promise.
    if (execTextareaCopy(str)) {
      return Promise.resolve(true);
    }
    return Promise.resolve(selectFallbackElement(fallbackEl));
  }

  function showToastNow(message, durationMs) {
    if (!document.body) return;
    if (!toastEl || !toastEl.isConnected) {
      toastEl = document.createElement("div");
      toastEl.className = "ts-toast";
      toastEl.setAttribute("role", "status");
      toastEl.setAttribute("aria-live", "polite");
      document.body.appendChild(toastEl);
    }

    toastEl.textContent = String(message == null ? "" : message);
    toastEl.classList.add("show");

    if (toastTimer) {
      window.clearTimeout(toastTimer);
      toastTimer = null;
    }

    var delay = typeof durationMs === "number" && durationMs >= 0 ? durationMs : 1800;
    if (delay > 0) {
      toastTimer = window.setTimeout(function () {
        if (toastEl) {
          toastEl.classList.remove("show");
        }
        toastTimer = null;
      }, delay);
    }
  }

  function toast(message, durationMs) {
    var ms = durationMs === undefined ? 1800 : durationMs;
    if (document.body) {
      showToastNow(message, ms);
    } else {
      document.addEventListener(
        "DOMContentLoaded",
        function () {
          showToastNow(message, ms);
        },
        { once: true }
      );
    }
  }

  window.ToolStow = {
    getTheme: getTheme,
    onThemeChange: onThemeChange,
    copy: copy,
    toast: toast,
  };
})();
