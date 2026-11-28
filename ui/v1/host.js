/* ==========================================================================
   ToolStow Headless Host SDK (/ui/v1/host.js)
   Zero-DOM-opinion state + iframe/theme orchestration for shell themes.
   Load before the shell's app.js:  <script src="/ui/v1/host.js"></script>

   - Builds no layout and injects no markup; shells render from getState().
   - Only DOM writes: documentElement.dataset.theme, document.title, location
     hash, and the tool <iframe> it mounts (via replaceChildren / textContent).
   - Accent is always READ from computed --ts-accent; never written inline.
   ========================================================================== */

(function (global) {
  "use strict";

  if (!global || !global.document) return;

  var doc = global.document;
  var ROOT = doc.documentElement;

  var STORAGE_KEY_MODE = "toolstow:mode";
  var DEFAULT_SITE_TITLE = "ToolStow";
  var DEFAULT_SITE_DESC = "Personal Micro-App Toolbox";
  var DEFAULT_ICON_GLYPH = "🛠️";

  var SANDBOX_ALLOWLIST = new Set([
    "allow-scripts",
    "allow-downloads",
    "allow-forms",
    "allow-modals",
    "allow-popups",
    "allow-popups-to-escape-sandbox",
  ]);

  var DEFAULT_SANDBOX = Object.freeze(["allow-scripts", "allow-downloads", "allow-forms"]);

  var SYNCED_TOKENS = Object.freeze([
    "--ts-canvas",
    "--ts-surface",
    "--ts-surface-hover",
    "--ts-border",
    "--ts-border-strong",
    "--ts-ink",
    "--ts-ink-secondary",
    "--ts-ink-muted",
    "--ts-inverted-bg",
    "--ts-inverted-bg-hover",
    "--ts-inverted-ink",
    "--ts-accent",
    "--ts-danger",
    "--ts-danger-bg",
    "--ts-danger-border",
    "--ts-success",
    "--ts-code-bg",
    "--ts-code-ink",
    "--ts-radius-sm",
    "--ts-radius-md",
    "--ts-space-1",
    "--ts-space-2",
    "--ts-space-3",
    "--ts-space-4",
    "--ts-space-5",
    "--ts-font-sans",
    "--ts-font-mono",
  ]);

  /* ------------------------------------------------------------------------
     Static helpers
     ------------------------------------------------------------------------ */

  function isSafeToolUrl(url) {
    if (typeof url !== "string" || url.startsWith("//")) return false;
    if (/[\u0000-\u001f\u007f]/.test(url)) return false;
    return /^\/(?:t\/|tp\/[0-9a-f]{64}\/)/i.test(url);
  }

  function normalizeVisibility(tool) {
    if (!tool || typeof tool !== "object") return "public";
    if (
      tool.visibility === "private" ||
      tool.visibility === "draft" ||
      tool.visibility === "public"
    ) {
      return tool.visibility;
    }
    return tool.draft === true ? "draft" : "public";
  }

  function normalizeHexColor(val) {
    if (typeof val !== "string") return null;
    var trimmed = val.trim();
    if (/^#[0-9a-fA-F]{6}$/.test(trimmed)) {
      return trimmed.toLowerCase();
    }
    var m = trimmed.match(/^#([0-9a-fA-F])([0-9a-fA-F])([0-9a-fA-F])$/);
    if (m) {
      return ("#" + m[1] + m[1] + m[2] + m[2] + m[3] + m[3]).toLowerCase();
    }
    return null;
  }

  // Like normalizeHexColor, but also accepts computed rgb()/rgba() values.
  function cssColorToHex(val) {
    var hex = normalizeHexColor(val);
    if (hex) return hex;
    if (typeof val !== "string") return null;
    var m = val
      .trim()
      .match(
        /^rgba?\(\s*(\d{1,3}(?:\.\d+)?)[\s,]+(\d{1,3}(?:\.\d+)?)[\s,]+(\d{1,3}(?:\.\d+)?)\s*(?:[,/]\s*[\d.]+%?\s*)?\)$/i
      );
    if (!m) return null;
    var out = "#";
    for (var i = 1; i <= 3; i++) {
      var n = Math.round(Number(m[i]));
      if (!(n >= 0 && n <= 255)) return null;
      out += (n < 16 ? "0" : "") + n.toString(16);
    }
    return out;
  }

  function sanitizeSandboxTokens(rawTokens) {
    if (!Array.isArray(rawTokens)) {
      return DEFAULT_SANDBOX.slice();
    }
    return rawTokens.filter(function (token) {
      return typeof token === "string" && SANDBOX_ALLOWLIST.has(token);
    });
  }

  function isPathLikeIcon(tool) {
    if (!tool || typeof tool !== "object") return false;
    if (tool.iconIsPath === true) return true;
    var icon = tool.icon;
    if (typeof icon !== "string" || !icon) return false;
    if (/^(https?:|data:|javascript:|vbscript:|blob:|file:)/i.test(icon.trim())) return false;
    return icon.indexOf("/") !== -1 || /\.(svg|png|jpe?g|webp|gif|ico)$/i.test(icon);
  }

  function resolveIconUrl(tool) {
    if (!tool || typeof tool !== "object") return null;
    if (isSafeToolUrl(tool.iconUrl)) {
      return tool.iconUrl;
    }
    if (!isPathLikeIcon(tool)) return null;
    if (isSafeToolUrl(tool.icon)) {
      return tool.icon;
    }
    if (typeof tool.icon !== "string" || typeof tool.id !== "string" || !tool.id) {
      return null;
    }
    var segments = tool.icon
      .split("/")
      .filter(function (s) {
        return s && s !== "." && s !== "..";
      })
      .map(function (s) {
        return encodeURIComponent(s);
      });
    if (segments.length === 0) return null;
    var prefix = "/t/";
    if (typeof tool.entryUrl === "string") {
      var tpMatch = tool.entryUrl.match(/^(\/tp\/[0-9a-f]{64}\/)/i);
      if (tpMatch) prefix = tpMatch[1];
    }
    var url = prefix + encodeURIComponent(tool.id) + "/" + segments.join("/");
    return isSafeToolUrl(url) ? url : null;
  }

  function renderIcon(container, tool, fallbackGlyph) {
    if (!container || typeof container.replaceChildren !== "function") return;
    var glyph =
      typeof fallbackGlyph === "string" && fallbackGlyph ? fallbackGlyph : DEFAULT_ICON_GLYPH;
    var url = resolveIconUrl(tool);
    if (url) {
      var img = doc.createElement("img");
      img.alt = "";
      img.loading = "lazy";
      img.decoding = "async";
      img.addEventListener(
        "error",
        function () {
          container.replaceChildren(doc.createTextNode(glyph));
        },
        { once: true }
      );
      img.src = url;
      container.replaceChildren(img);
      return;
    }
    var text =
      tool && typeof tool.icon === "string" && tool.icon.trim() && !isPathLikeIcon(tool)
        ? tool.icon.trim()
        : glyph;
    container.replaceChildren();
    container.textContent = text;
  }

  function readStorage(key) {
    try {
      return global.localStorage ? global.localStorage.getItem(key) : null;
    } catch (_) {
      return null;
    }
  }

  function writeStorage(key, value) {
    try {
      if (global.localStorage) global.localStorage.setItem(key, value);
      return true;
    } catch (_) {
      return false;
    }
  }

  function normalizeMode(value) {
    if (typeof value !== "string") return null;
    var v = value.trim().toLowerCase();
    return v === "dark" || v === "light" ? v : null;
  }

  function str(value) {
    return typeof value === "string" ? value : "";
  }

  function lower(value) {
    return str(value).trim().toLowerCase();
  }

  function parseErrorPayload(body, fallback) {
    if (body && typeof body === "object") {
      var msg = body.error || body.message;
      if (msg && body.code) return "[" + body.code + "] " + msg;
      if (msg) return String(msg);
    }
    return fallback;
  }

  /* ------------------------------------------------------------------------
     Host instance factory
     ------------------------------------------------------------------------ */

  function create(options) {
    var opts = options && typeof options === "object" ? options : {};

    var iframeHost =
      opts.iframeHost && typeof opts.iframeHost.replaceChildren === "function"
        ? opts.iframeHost
        : null;
    var defaultMode =
      opts.defaultMode === "light" || opts.defaultMode === "dark" ? opts.defaultMode : "auto";
    var lockMode = normalizeMode(opts.lockMode);
    var fallbackAccentLight = normalizeHexColor(opts.fallbackAccentLight) || "#18181b";
    var fallbackAccentDark = normalizeHexColor(opts.fallbackAccentDark) || "#fafafa";
    var hashRouting = opts.hashRouting !== false;
    var titleSeparator = typeof opts.titleSeparator === "string" ? opts.titleSeparator : " · ";
    var onStateChange = typeof opts.onStateChange === "function" ? opts.onStateChange : null;
    var onError = typeof opts.onError === "function" ? opts.onError : null;
    var onToolReady = typeof opts.onToolReady === "function" ? opts.onToolReady : null;
    var onSearchShortcut = typeof opts.onSearchShortcut === "function" ? opts.onSearchShortcut : null;

    var mediaQuery = null;
    try {
      if (typeof global.matchMedia === "function") {
        mediaQuery = global.matchMedia("(prefers-color-scheme: dark)");
      }
    } catch (_) {
      mediaQuery = null;
    }

    var savedMode = normalizeMode(readStorage(STORAGE_KEY_MODE));

    function systemMode() {
      return mediaQuery && mediaQuery.matches ? "dark" : "light";
    }

    function initialMode() {
      if (lockMode) return lockMode;
      if (savedMode) return savedMode;
      if (defaultMode !== "auto") return defaultMode;
      return systemMode();
    }

    var state = {
      site: {
        siteTitle: DEFAULT_SITE_TITLE,
        siteDescription: DEFAULT_SITE_DESC,
        isAdmin: false,
        adminEnabled: false,
        activeTheme: "default",
        themeSource: "embedded",
      },
      allTools: [],
      filteredTools: [],
      filters: { q: "", category: "", tag: "" },
      selectedId: null,
      activeIframe: null,
      activeIframeReady: false,
      theme: { mode: initialMode(), accent: fallbackAccentLight },
      loading: false,
      error: null,
    };

    var activeTool = null;
    var tokens = {};
    var subscribers = new Set();
    var bootstrapPromise = null;
    var refreshSeq = 0;
    var listenersBound = false;

    /* ---------- notifications ---------- */

    function reportError(message, err) {
      if (!onError) return;
      try {
        onError(message, err);
      } catch (_) {
        // Isolate callback errors.
      }
    }

    function notify() {
      var snapshot = getState();
      if (onStateChange) {
        try {
          onStateChange(snapshot);
        } catch (_) {
          // Isolate callback errors.
        }
      }
      subscribers.forEach(function (fn) {
        try {
          fn(snapshot);
        } catch (_) {
          // Isolate subscriber errors.
        }
      });
    }

    function subscribe(fn) {
      if (typeof fn !== "function") return function () {};
      subscribers.add(fn);
      return function unsubscribe() {
        subscribers.delete(fn);
      };
    }

    /* ---------- derived data ---------- */

    function findTool(id) {
      if (typeof id !== "string" || !id) return null;
      for (var i = 0; i < state.allTools.length; i++) {
        if (state.allTools[i] && state.allTools[i].id === id) return state.allTools[i];
      }
      for (var j = 0; j < state.filteredTools.length; j++) {
        if (state.filteredTools[j] && state.filteredTools[j].id === id) {
          return state.filteredTools[j];
        }
      }
      return null;
    }

    function resolveTool(toolOrId) {
      if (typeof toolOrId === "string") return findTool(toolOrId);
      if (toolOrId && typeof toolOrId === "object" && typeof toolOrId.id === "string") {
        return toolOrId;
      }
      return null;
    }

    function getSelectedTool() {
      return findTool(state.selectedId);
    }

    function toolTags(tool) {
      return Array.isArray(tool.tags)
        ? tool.tags.filter(function (t) {
            return typeof t === "string";
          })
        : [];
    }

    // Mirrors backend GET /api/tools filtering (registry.rs list_filtered).
    function matchesFilters(tool, filters) {
      if (!tool || typeof tool !== "object") return false;
      var cat = lower(filters.category);
      var tag = lower(filters.tag);
      var q = lower(filters.q);
      var tags = toolTags(tool);

      if (cat && lower(tool.category) !== cat) return false;
      if (
        tag &&
        !tags.some(function (t) {
          return lower(t) === tag;
        })
      ) {
        return false;
      }
      if (q) {
        var hit =
          str(tool.name).toLowerCase().indexOf(q) !== -1 ||
          str(tool.description).toLowerCase().indexOf(q) !== -1 ||
          str(tool.id).toLowerCase().indexOf(q) !== -1 ||
          str(tool.category).toLowerCase().indexOf(q) !== -1 ||
          tags.some(function (t) {
            return t.toLowerCase().indexOf(q) !== -1;
          });
        if (!hit) return false;
      }
      return true;
    }

    function applyFilters() {
      if (!hasActiveFilters()) {
        state.filteredTools = state.allTools.slice();
        return;
      }
      var f = state.filters;
      state.filteredTools = state.allTools.filter(function (tool) {
        return matchesFilters(tool, f);
      });
    }

    function computeCategories() {
      var counts = new Map();
      state.allTools.forEach(function (tool) {
        var name = str(tool && tool.category).trim();
        if (!name) return;
        counts.set(name, (counts.get(name) || 0) + 1);
      });
      return Array.from(counts.keys())
        .sort(function (a, b) {
          return a.localeCompare(b);
        })
        .map(function (name) {
          return { name: name, count: counts.get(name) };
        });
    }

    function computeTags() {
      var seen = new Map();
      state.allTools.forEach(function (tool) {
        if (!tool) return;
        toolTags(tool).forEach(function (t) {
          var trimmed = t.trim();
          var key = trimmed.toLowerCase();
          if (trimmed && !seen.has(key)) seen.set(key, trimmed);
        });
      });
      return Array.from(seen.values()).sort(function (a, b) {
        return a.localeCompare(b);
      });
    }

    function getState() {
      return {
        site: Object.assign({}, state.site),
        allTools: state.allTools.slice(),
        filteredTools: state.filteredTools.slice(),
        categories: computeCategories(),
        tags: computeTags(),
        filters: Object.assign({}, state.filters),
        selectedId: state.selectedId,
        selectedTool: getSelectedTool(),
        activeIframe: state.activeIframe,
        activeIframeReady: state.activeIframeReady,
        theme: {
          mode: state.theme.mode,
          accent: state.theme.accent,
          locked: Boolean(lockMode),
        },
        loading: state.loading,
        error: state.error,
      };
    }

    /* ---------- theme ---------- */

    function fallbackAccent(mode) {
      return mode === "dark" ? fallbackAccentDark : fallbackAccentLight;
    }

    function readComputed() {
      try {
        return global.getComputedStyle(ROOT);
      } catch (_) {
        return null;
      }
    }

    function readThemeAccent() {
      var cs = readComputed();
      var computed = cs ? cssColorToHex(cs.getPropertyValue("--ts-accent")) : null;
      return computed || fallbackAccent(state.theme.mode);
    }

    function collectThemeTokens() {
      var cs = readComputed();
      var out = {};
      if (!cs) return out;
      for (var i = 0; i < SYNCED_TOKENS.length; i++) {
        var name = SYNCED_TOKENS[i];
        var val = String(cs.getPropertyValue(name) || "").trim();
        if (val) out[name] = val;
      }
      return out;
    }

    function readThemeFromCss() {
      state.theme.accent = readThemeAccent();
      tokens = collectThemeTokens();
    }

    function readThemeSkin() {
      var skin = ROOT.dataset.tsSkin || "";
      return /^[a-z0-9][a-z0-9-]{0,63}$/.test(skin) ? skin : null;
    }

    function applyModeToDocument() {
      if (lockMode) state.theme.mode = lockMode;
      ROOT.dataset.theme = state.theme.mode;
      readThemeFromCss();
    }

    function postThemeToActiveIframe() {
      var iframe = state.activeIframe;
      if (!iframe || !iframe.contentWindow) return false;
      var tool = activeTool || getSelectedTool();
      var shouldSync = Boolean((tool && tool.themeAware === true) || state.activeIframeReady);
      if (!shouldSync) return false;
      try {
        // Sandboxed tool frames have an opaque origin, so "*" is the only valid target.
        iframe.contentWindow.postMessage(
          {
            type: "toolstow:theme",
            payload: {
              mode: state.theme.mode,
              accent: state.theme.accent,
              skin: readThemeSkin(),
              searchShortcut: Boolean(onSearchShortcut),
              tokens: Object.assign({}, tokens),
            },
          },
          "*"
        );
        return true;
      } catch (_) {
        return false;
      }
    }

    function setThemeMode(mode) {
      var next = normalizeMode(mode) || "light";
      if (!lockMode) {
        state.theme.mode = next;
        savedMode = next;
        writeStorage(STORAGE_KEY_MODE, next);
      }
      applyModeToDocument();
      postThemeToActiveIframe();
      notify();
      return state.theme.mode;
    }

    function toggleThemeMode() {
      return setThemeMode(state.theme.mode === "dark" ? "light" : "dark");
    }

    function syncTheme() {
      readThemeFromCss();
      postThemeToActiveIframe();
      notify();
      return { mode: state.theme.mode, accent: state.theme.accent };
    }

    function handleSystemModeChange() {
      // Follow the OS only when nothing pins the mode.
      if (lockMode || savedMode || defaultMode !== "auto") return;
      var next = systemMode();
      if (next === state.theme.mode) return;
      state.theme.mode = next;
      applyModeToDocument();
      postThemeToActiveIframe();
      notify();
    }

    /* ---------- title & hash ---------- */

    function siteTitle() {
      return state.site.siteTitle || DEFAULT_SITE_TITLE;
    }

    function updateDocumentTitle(tool) {
      try {
        doc.title =
          tool && typeof tool.name === "string" && tool.name
            ? tool.name + titleSeparator + siteTitle()
            : siteTitle();
      } catch (_) {
        // Ignore title errors.
      }
    }

    function parseToolIdFromHash() {
      var hash = (global.location && global.location.hash) || "";
      var match = hash.match(/^#tool=(.+)$/);
      if (!match) return null;
      try {
        return decodeURIComponent(match[1]);
      } catch (_) {
        return match[1];
      }
    }

    function clearHash() {
      if (!global.location || !global.location.hash) return;
      try {
        global.history.replaceState(
          null,
          "",
          global.location.pathname + global.location.search
        );
      } catch (_) {
        // Ignore history errors (sandboxed documents).
      }
    }

    function restoreToolFromHash() {
      if (!hashRouting || state.error) return false;
      var id = parseToolIdFromHash();
      if (!id) return false;
      if (findTool(id)) {
        selectTool(id, { updateHash: false });
        return true;
      }
      reportError("未找到工具：" + id);
      clearHash();
      return false;
    }

    /* ---------- URLs & iframe ---------- */

    function buildToolUrl(entryUrl, buildOpts) {
      if (!isSafeToolUrl(entryUrl)) return null;
      var cacheBust = Boolean(buildOpts && buildOpts.cacheBust);
      var url;
      try {
        url = new URL(entryUrl, global.location.origin);
      } catch (_) {
        return null;
      }
      if (url.origin !== global.location.origin || !isSafeToolUrl(url.pathname)) return null;
      url.searchParams.set("theme", state.theme.mode);
      url.searchParams.set("accent", state.theme.accent);
      var skin = readThemeSkin();
      if (skin) url.searchParams.set("ts-skin", skin);
      if (cacheBust) {
        url.searchParams.set("_ts", String(Date.now()));
      }
      var out = url.pathname + url.search + url.hash;
      return isSafeToolUrl(out) ? out : null;
    }

    function getStandaloneUrl(toolOrId) {
      var tool = resolveTool(toolOrId);
      if (!tool) return null;
      return buildToolUrl(tool.entryUrl);
    }

    function detachActiveIframe() {
      var iframe = state.activeIframe;
      state.activeIframe = null;
      state.activeIframeReady = false;
      activeTool = null;
      if (iframe && iframe.parentNode) {
        iframe.parentNode.removeChild(iframe);
      }
    }

    function mountTool(containerEl, toolOrId, mountOpts) {
      var tool = resolveTool(toolOrId);
      if (!tool) {
        reportError("未找到工具：" + String(toolOrId && toolOrId.id ? toolOrId.id : toolOrId));
        return null;
      }
      if (!containerEl || typeof containerEl.replaceChildren !== "function") {
        reportError("工具容器无效：" + tool.id);
        return null;
      }
      var src = isSafeToolUrl(tool.entryUrl)
        ? buildToolUrl(tool.entryUrl, { cacheBust: Boolean(mountOpts && mountOpts.cacheBust) })
        : null;
      if (!src) {
        reportError("工具入口路径无效：" + tool.id);
        return null;
      }

      var previous = state.activeIframe;
      if (previous && previous.parentNode && previous.parentNode !== containerEl) {
        previous.parentNode.removeChild(previous);
      }

      var iframe = doc.createElement("iframe");
      iframe.className = "tool-iframe";
      iframe.title = typeof tool.name === "string" && tool.name ? tool.name : tool.id;
      iframe.setAttribute("sandbox", sanitizeSandboxTokens(tool.sandbox).join(" "));
      iframe.setAttribute("referrerpolicy", "no-referrer");
      iframe.addEventListener("load", function () {
        if (state.activeIframe === iframe) {
          postThemeToActiveIframe();
        }
      });
      iframe.src = src;

      state.activeIframe = iframe;
      state.activeIframeReady = false;
      activeTool = tool;
      containerEl.replaceChildren(iframe);
      notify();
      return iframe;
    }

    function reloadTool() {
      var tool = getSelectedTool();
      if (!tool) return null;
      var container =
        (state.activeIframe && state.activeIframe.parentNode) || iframeHost || null;
      if (!container) return null;
      return mountTool(container, tool, { cacheBust: true });
    }

    /* ---------- selection ---------- */

    function clearSelection(clearOpts) {
      var updateHash = !clearOpts || clearOpts.updateHash !== false;
      state.selectedId = null;
      detachActiveIframe();
      if (iframeHost) iframeHost.replaceChildren();
      updateDocumentTitle(null);
      if (updateHash && hashRouting) clearHash();
      notify();
    }

    function selectTool(id, selectOpts) {
      var updateHash = !selectOpts || selectOpts.updateHash !== false;
      var forceReload = Boolean(selectOpts && selectOpts.forceReload);
      var tool = findTool(id);

      if (!tool) {
        reportError("未找到工具：" + String(id));
        clearSelection({ updateHash: false });
        return null;
      }

      var sameTool = state.selectedId === tool.id;
      state.selectedId = tool.id;

      if (updateHash && hashRouting && global.location) {
        var expectedHash = "#tool=" + encodeURIComponent(tool.id);
        if (global.location.hash !== expectedHash) {
          global.location.hash = expectedHash;
        }
      }

      updateDocumentTitle(tool);

      if (iframeHost && (!sameTool || forceReload || !state.activeIframe)) {
        // mountTool notifies on success.
        if (mountTool(iframeHost, tool, { cacheBust: forceReload })) return tool;
        // Mount failed (unsafe entryUrl): never leave the previous tool's frame visible.
        detachActiveIframe();
        iframeHost.replaceChildren();
      } else if (!sameTool && state.activeIframe) {
        // Shell mounts its own frame: drop the previous tool's frame.
        detachActiveIframe();
      }
      notify();
      return tool;
    }

    /* ---------- filters ---------- */

    function hasActiveFilters() {
      var f = state.filters;
      return Boolean(str(f.q).trim() || str(f.category).trim() || str(f.tag).trim());
    }

    function setFilters(partial) {
      if (partial && typeof partial === "object") {
        ["q", "category", "tag"].forEach(function (key) {
          if (Object.prototype.hasOwnProperty.call(partial, key)) {
            var v = partial[key];
            state.filters[key] = v == null ? "" : String(v);
          }
        });
      }
      applyFilters();
      notify();
      return getState();
    }

    function clearFilters() {
      state.filters = { q: "", category: "", tag: "" };
      applyFilters();
      notify();
      return getState();
    }

    /* ---------- network ---------- */

    function fetchJson(url) {
      if (typeof global.fetch !== "function") {
        return Promise.reject(new Error("fetch unavailable"));
      }
      return global.fetch(url, { credentials: "same-origin", cache: "no-store" });
    }

    function fetchSiteMe() {
      return fetchJson("/api/me")
        .then(function (res) {
          if (!res.ok) return null;
          return res.json();
        })
        .then(function (data) {
          if (!data || typeof data !== "object") return null;
          return {
            siteTitle:
              typeof data.siteTitle === "string" && data.siteTitle.trim()
                ? data.siteTitle.trim()
                : DEFAULT_SITE_TITLE,
            siteDescription:
              typeof data.siteDescription === "string"
                ? data.siteDescription.trim()
                : DEFAULT_SITE_DESC,
            isAdmin: data.isAdmin === true,
            adminEnabled: data.adminEnabled === true,
            activeTheme: typeof data.activeTheme === "string" ? data.activeTheme : "default",
            themeSource: typeof data.themeSource === "string" ? data.themeSource : "embedded",
          };
        })
        .catch(function () {
          // Keep safe defaults (isAdmin: false) if /api/me is unavailable.
          return null;
        });
    }

    function fetchTools() {
      return fetchJson("/api/tools").then(function (res) {
        if (!res.ok) {
          return res
            .json()
            .catch(function () {
              return null;
            })
            .then(function (body) {
              throw new Error(
                parseErrorPayload(body, "加载工具列表失败 (HTTP " + res.status + ")")
              );
            });
        }
        return res.json().then(function (data) {
          return Array.isArray(data)
            ? data.filter(function (t) {
                return t && typeof t === "object" && typeof t.id === "string";
              })
            : [];
        });
      });
    }

    function loadData() {
      var seq = ++refreshSeq;
      state.loading = true;
      notify();
      return Promise.all([
        fetchSiteMe(),
        fetchTools().then(
          function (tools) {
            return { ok: true, tools: tools };
          },
          function (err) {
            return { ok: false, err: err };
          }
        ),
      ]).then(function (results) {
        if (seq !== refreshSeq) return false; // A newer refresh superseded this one.
        var site = results[0];
        var toolsResult = results[1];
        if (site) state.site = site;
        if (toolsResult.ok) {
          state.allTools = toolsResult.tools;
          state.error = null;
          applyFilters();
        } else {
          var message =
            (toolsResult.err && toolsResult.err.message) || "无法连接到 ToolStow 服务";
          state.error = message;
          reportError(message, toolsResult.err);
        }
        state.loading = false;
        return true;
      });
    }

    function refresh(refreshOpts) {
      var keepSelection = !refreshOpts || refreshOpts.keepSelection !== false;
      return loadData().then(function (applied) {
        if (!applied) return getState();
        if (state.selectedId && (!keepSelection || !findTool(state.selectedId))) {
          clearSelection({ updateHash: true }); // notifies
        } else if (state.selectedId || !restoreToolFromHash()) {
          updateDocumentTitle(getSelectedTool());
          notify();
        }
        return getState();
      });
    }

    /* ---------- events & bootstrap ---------- */

    function handleMessage(event) {
      var iframe = state.activeIframe;
      if (!iframe || !event || event.source !== iframe.contentWindow) return;
      var data = event.data;
      if (!data || typeof data !== "object") return;
      if (data.type === "toolstow:search" && onSearchShortcut && doc.activeElement === iframe) {
        onSearchShortcut();
        return;
      }
      if (data.type !== "toolstow:ready") return;
      state.activeIframeReady = true;
      readThemeFromCss();
      postThemeToActiveIframe();
      var tool = activeTool || getSelectedTool();
      if (onToolReady && tool) {
        try {
          onToolReady(tool);
        } catch (_) {
          // Isolate callback errors.
        }
      }
      notify();
    }

    function handleHashChange() {
      var id = parseToolIdFromHash();
      if (id) {
        if (id !== state.selectedId) {
          selectTool(id, { updateHash: false, forceReload: false });
        }
      } else if (state.selectedId) {
        clearSelection({ updateHash: false });
      }
    }

    function bindListeners() {
      if (listenersBound) return;
      listenersBound = true;
      global.addEventListener("message", handleMessage);
      if (hashRouting) global.addEventListener("hashchange", handleHashChange);
      if (mediaQuery) {
        if (typeof mediaQuery.addEventListener === "function") {
          mediaQuery.addEventListener("change", handleSystemModeChange);
        } else if (typeof mediaQuery.addListener === "function") {
          mediaQuery.addListener(handleSystemModeChange);
        }
      }
    }

    function destroy() {
      if (!listenersBound) return;
      listenersBound = false;
      global.removeEventListener("message", handleMessage);
      global.removeEventListener("hashchange", handleHashChange);
      if (mediaQuery) {
        if (typeof mediaQuery.removeEventListener === "function") {
          mediaQuery.removeEventListener("change", handleSystemModeChange);
        } else if (typeof mediaQuery.removeListener === "function") {
          mediaQuery.removeListener(handleSystemModeChange);
        }
      }
      subscribers.clear();
    }

    function bootstrap() {
      if (bootstrapPromise) return bootstrapPromise;
      applyModeToDocument();
      bindListeners();
      bootstrapPromise = loadData().then(function (applied) {
        if (applied && !restoreToolFromHash()) {
          updateDocumentTitle(getSelectedTool());
          notify();
        }
        return getState();
      });
      return bootstrapPromise;
    }

    // Prime accent/tokens from CSS without touching the DOM until bootstrap().
    state.theme.accent = readThemeAccent();

    return {
      bootstrap: bootstrap,
      getState: getState,
      refresh: refresh,
      setFilters: setFilters,
      clearFilters: clearFilters,
      hasActiveFilters: hasActiveFilters,
      selectTool: selectTool,
      clearSelection: clearSelection,
      mountTool: mountTool,
      reloadTool: reloadTool,
      buildToolUrl: buildToolUrl,
      getStandaloneUrl: getStandaloneUrl,
      setThemeMode: setThemeMode,
      toggleThemeMode: toggleThemeMode,
      syncTheme: syncTheme,
      subscribe: subscribe,
      destroy: destroy,
    };
  }

  global.ToolStowHost = {
    SANDBOX_ALLOWLIST: SANDBOX_ALLOWLIST,
    DEFAULT_SANDBOX: DEFAULT_SANDBOX,
    SYNCED_TOKENS: SYNCED_TOKENS,
    STORAGE_KEY_MODE: STORAGE_KEY_MODE,
    isSafeToolUrl: isSafeToolUrl,
    normalizeVisibility: normalizeVisibility,
    normalizeHexColor: normalizeHexColor,
    sanitizeSandboxTokens: sanitizeSandboxTokens,
    resolveIconUrl: resolveIconUrl,
    renderIcon: renderIcon,
    readStorage: readStorage,
    writeStorage: writeStorage,
    create: create,
  };
})(typeof window !== "undefined" ? window : undefined);
