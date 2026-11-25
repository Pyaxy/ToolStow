const SANDBOX_ALLOWLIST = new Set([
  "allow-scripts",
  "allow-downloads",
  "allow-forms",
  "allow-modals",
  "allow-popups",
  "allow-popups-to-escape-sandbox",
]);

const DEFAULT_SANDBOX = ["allow-scripts", "allow-downloads", "allow-forms"];
const STORAGE_KEY_MODE = "toolstow:mode";
const STORAGE_KEY_ACCENT = "toolstow:accent";
const THEME_ACCENT_LIGHT = "#047857";
const THEME_ACCENT_DARK = "#10b981";
const DEFAULT_SITE_TITLE = "ToolStow";
const DEFAULT_SITE_DESC = "Personal Micro-App Toolbox";
const SYNCED_TOKEN_NAMES = [
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
];

const state = {
  site: {
    siteTitle: DEFAULT_SITE_TITLE,
    siteDescription: DEFAULT_SITE_DESC,
    defaultAccent: THEME_ACCENT_DARK,
    isAdmin: false,
    adminEnabled: false,
    themeSource: "custom",
  },
  allTools: [],
  filteredTools: [],
  selectedId: null,
  activeIframe: null,
  activeIframeReady: false,
  filters: {
    q: "",
    category: "",
    tag: "",
  },
  theme: {
    mode: "dark",
    accent: THEME_ACCENT_DARK,
    hasUserAccent: false,
  },
  statusTimer: null,
  searchTimer: null,
};

const els = {
  brandLink: document.getElementById("brand-link"),
  brandTitle: document.getElementById("brand-title"),
  sidebarToggle: document.getElementById("sidebar-toggle"),
  sidebar: document.getElementById("sidebar"),
  sidebarBackdrop: document.getElementById("sidebar-backdrop"),
  searchInput: document.getElementById("search-input"),
  accentInput: document.getElementById("accent-input"),
  accentSwatch: document.getElementById("accent-swatch"),
  themeToggle: document.getElementById("theme-toggle"),
  adminLink: document.getElementById("admin-link"),
  statusBanner: document.getElementById("status-banner"),
  refreshListBtn: document.getElementById("refresh-list-btn"),
  clearFiltersBtn: document.getElementById("clear-filters-btn"),
  categoryList: document.getElementById("category-list"),
  tagList: document.getElementById("tag-list"),
  toolList: document.getElementById("tool-list"),
  toolsCount: document.getElementById("tools-count"),
  emptyState: document.getElementById("empty-state"),
  heroCmdText: document.getElementById("hero-cmd-text"),
  emptyTitle: document.getElementById("empty-title"),
  emptyCount: document.getElementById("empty-count"),
  emptyDesc: document.getElementById("empty-desc"),
  emptyGrid: document.getElementById("empty-grid"),
  toolView: document.getElementById("tool-view"),
  mobileBackBtn: document.getElementById("mobile-back-btn"),
  activeToolIcon: document.getElementById("active-tool-icon"),
  activeToolName: document.getElementById("active-tool-name"),
  activeToolId: document.getElementById("active-tool-id"),
  activeToolVersion: document.getElementById("active-tool-version"),
  activeToolThemeBadge: document.getElementById("active-tool-theme-badge"),
  activeToolDesc: document.getElementById("active-tool-desc"),
  activeToolTags: document.getElementById("active-tool-tags"),
  reloadToolBtn: document.getElementById("reload-tool-btn"),
  standaloneLink: document.getElementById("standalone-link"),
  closeToolBtn: document.getElementById("close-tool-btn"),
  iframeHost: document.getElementById("iframe-host"),
  footerStatus: document.getElementById("footer-status"),
  footerToolsStat: document.getElementById("footer-tools-stat"),
};

function readStorage(key) {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key, value) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Ignore storage failures.
  }
}

function normalizeHexColor(value) {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (/^#[0-9a-fA-F]{6}$/.test(trimmed)) return trimmed.toLowerCase();
  const shortMatch = trimmed.match(/^#([0-9a-fA-F])([0-9a-fA-F])([0-9a-fA-F])$/);
  if (shortMatch) {
    const [, r, g, b] = shortMatch;
    return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
  }
  return null;
}

function isSafeToolUrl(url) {
  return typeof url === "string" && url.startsWith("/t/") && !url.startsWith("//");
}

function defaultAccentForMode(mode) {
  return mode === "dark" ? THEME_ACCENT_DARK : THEME_ACCENT_LIGHT;
}

function initTheme() {
  const savedMode = readStorage(STORAGE_KEY_MODE);
  if (savedMode === "dark" || savedMode === "light") {
    state.theme.mode = savedMode;
  } else if (window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches) {
    state.theme.mode = "light";
  } else {
    state.theme.mode = "dark";
  }

  const savedAccent = normalizeHexColor(readStorage(STORAGE_KEY_ACCENT));
  if (savedAccent) {
    state.theme.accent = savedAccent;
    state.theme.hasUserAccent = true;
  } else {
    state.theme.accent = defaultAccentForMode(state.theme.mode);
  }

  applyThemeToHost();
}

function applyThemeToHost() {
  document.documentElement.dataset.theme = state.theme.mode;
  if (state.theme.hasUserAccent) {
    document.documentElement.style.setProperty("--ts-accent", state.theme.accent);
  } else {
    document.documentElement.style.removeProperty("--ts-accent");
    const computed = normalizeHexColor(
      window.getComputedStyle(document.documentElement).getPropertyValue("--ts-accent")
    );
    state.theme.accent = computed || defaultAccentForMode(state.theme.mode);
  }

  els.accentInput.value = state.theme.accent;
  els.accentSwatch.style.backgroundColor = state.theme.accent;
  els.themeToggle.textContent = state.theme.mode === "dark" ? "MODE:LIGHT" : "MODE:DARK";
}

function setThemeMode(mode) {
  state.theme.mode = mode === "dark" ? "dark" : "light";
  writeStorage(STORAGE_KEY_MODE, state.theme.mode);
  applyThemeToHost();
  postThemeToActiveIframe();
}

function setAccentColor(accent) {
  const normalized = normalizeHexColor(accent);
  if (!normalized) return;
  state.theme.accent = normalized;
  state.theme.hasUserAccent = true;
  writeStorage(STORAGE_KEY_ACCENT, state.theme.accent);
  applyThemeToHost();
  postThemeToActiveIframe();
}

function updateDocumentTitle(tool = null) {
  const siteTitle = state.site.siteTitle || DEFAULT_SITE_TITLE;
  document.title = tool ? `${tool.name} :: ${siteTitle}` : siteTitle;
}

function applySiteInfo() {
  const title = state.site.siteTitle || DEFAULT_SITE_TITLE;
  els.brandTitle.textContent = title;
  els.adminLink.hidden = state.site.isAdmin !== true;
  updateDocumentTitle(getSelectedTool());

  if (!state.theme.hasUserAccent) {
    const serverAccent = normalizeHexColor(state.site.defaultAccent);
    if (serverAccent && serverAccent !== "#18181b" && serverAccent !== "#fafafa") {
      state.theme.accent = serverAccent;
      document.documentElement.style.setProperty("--ts-accent", serverAccent);
      els.accentInput.value = serverAccent;
      els.accentSwatch.style.backgroundColor = serverAccent;
      postThemeToActiveIframe();
    }
  }

  if (!state.selectedId) {
    updateEmptyState();
  }
}

function getSelectedTool() {
  if (!state.selectedId) return null;
  return (
    state.allTools.find((t) => t.id === state.selectedId) ||
    state.filteredTools.find((t) => t.id === state.selectedId) ||
    null
  );
}

function collectThemeTokens() {
  const computed = window.getComputedStyle(document.documentElement);
  const tokens = {};
  for (const name of SYNCED_TOKEN_NAMES) {
    const val = computed.getPropertyValue(name).trim();
    if (val) {
      tokens[name] = val;
    }
  }
  return tokens;
}

function postThemeToActiveIframe() {
  const tool = getSelectedTool();
  const shouldSync = Boolean((tool && tool.themeAware === true) || state.activeIframeReady);
  if (!shouldSync) return;
  const iframe = state.activeIframe;
  if (!iframe || !iframe.contentWindow) return;

  iframe.contentWindow.postMessage(
    {
      type: "toolstow:theme",
      payload: {
        mode: state.theme.mode,
        accent: state.theme.accent,
        tokens: collectThemeTokens(),
      },
    },
    "*"
  );
}

function showStatus(message, kind = "info", autoHideMs = 4500) {
  if (state.statusTimer) {
    window.clearTimeout(state.statusTimer);
    state.statusTimer = null;
  }
  els.statusBanner.textContent = `[${kind.toUpperCase()}] ${message}`;
  els.statusBanner.dataset.kind = kind;
  els.statusBanner.hidden = false;

  if (autoHideMs > 0) {
    state.statusTimer = window.setTimeout(() => {
      els.statusBanner.hidden = true;
    }, autoHideMs);
  }
}

function parseErrorPayload(body, fallback) {
  if (body && typeof body === "object") {
    const msg = body.error || body.message;
    if (msg && body.code) return `[${body.code}] ${msg}`;
    if (msg) return String(msg);
  }
  return fallback;
}

async function fetchSiteMe() {
  try {
    const res = await fetch("/api/me", {
      credentials: "same-origin",
      cache: "no-store",
    });
    if (!res.ok) return;
    const data = await res.json();
    if (!data || typeof data !== "object") return;

    state.site = {
      siteTitle:
        typeof data.siteTitle === "string" && data.siteTitle.trim()
          ? data.siteTitle.trim()
          : DEFAULT_SITE_TITLE,
      siteDescription:
        typeof data.siteDescription === "string"
          ? data.siteDescription.trim()
          : DEFAULT_SITE_DESC,
      defaultAccent: normalizeHexColor(data.defaultAccent) || THEME_ACCENT_DARK,
      isAdmin: data.isAdmin === true,
      adminEnabled: data.adminEnabled === true,
      themeSource: typeof data.themeSource === "string" ? data.themeSource : "custom",
    };

    applySiteInfo();
  } catch {
    // Keep safe defaults (isAdmin: false).
  }
}

async function fetchToolsFromApi(filters = {}) {
  const params = new URLSearchParams();
  if (filters.q && filters.q.trim()) params.set("q", filters.q.trim());
  if (filters.category) params.set("category", filters.category);
  if (filters.tag) params.set("tag", filters.tag);

  const query = params.toString();
  const url = query ? `/api/tools?${query}` : "/api/tools";
  const res = await fetch(url, {
    credentials: "same-origin",
    cache: "no-store",
  });
  if (!res.ok) {
    let body = null;
    try {
      body = await res.json();
    } catch {
      // ignore non-json error
    }
    throw new Error(parseErrorPayload(body, `加载工具列表失败 (HTTP ${res.status})`));
  }
  const data = await res.json();
  return Array.isArray(data) ? data : [];
}

function hasActiveFilters() {
  return Boolean(state.filters.q.trim() || state.filters.category || state.filters.tag);
}

function updateFooterStats() {
  const shown = state.filteredTools.length;
  const total = state.allTools.length;
  els.footerToolsStat.textContent = `TOOLS:${shown}/${total}`;
  const selected = getSelectedTool();
  els.footerStatus.textContent = selected ? `PROC:${selected.id}` : "SYS:READY";
}

async function refreshAllAndFiltered(keepSelection = true) {
  try {
    const allTools = await fetchToolsFromApi({});
    state.allTools = allTools;
    state.filteredTools = hasActiveFilters()
      ? await fetchToolsFromApi(state.filters)
      : allTools;

    renderSidebar();
    updateFooterStats();

    if (keepSelection && state.selectedId) {
      const stillExists = state.allTools.some((t) => t.id === state.selectedId);
      if (!stillExists) {
        clearSelection(true);
      } else {
        renderActiveToolHeader(getSelectedTool());
      }
    } else if (!state.selectedId) {
      updateEmptyState();
    }
  } catch (err) {
    showStatus(err.message || "无法连接到 ToolStow 服务", "error", 0);
  }
}

async function refreshFilteredOnly() {
  try {
    state.filteredTools = await fetchToolsFromApi(state.filters);
    renderSidebar();
    updateFooterStats();
    if (!state.selectedId) {
      updateEmptyState();
    }
  } catch (err) {
    showStatus(err.message || "筛选工具失败", "error", 5000);
  }
}

function isRelativeIconPath(tool) {
  if (tool.iconIsPath === true) return true;
  const icon = tool.icon;
  if (typeof icon !== "string" || !icon) return false;
  if (/^(https?:|data:|javascript:|vbscript:)/i.test(icon)) return false;
  return icon.includes("/") || /\.(svg|png|jpe?g|webp|gif|ico)$/i.test(icon);
}

function resolveIconUrl(tool) {
  if (isSafeToolUrl(tool.iconUrl)) return tool.iconUrl;
  if (isSafeToolUrl(tool.icon)) return tool.icon;
  const encodedSegments = String(tool.icon)
    .split("/")
    .filter((s) => s && s !== "." && s !== "..")
    .map((s) => encodeURIComponent(s))
    .join("/");
  return `/t/${encodeURIComponent(tool.id)}/${encodedSegments}`;
}

function renderIconInto(container, tool) {
  container.replaceChildren();
  if (isRelativeIconPath(tool)) {
    const url = resolveIconUrl(tool);
    if (isSafeToolUrl(url)) {
      const img = document.createElement("img");
      img.src = url;
      img.alt = "";
      img.loading = "lazy";
      img.addEventListener("error", () => {
        container.replaceChildren(document.createTextNode("#"));
      });
      container.appendChild(img);
      return;
    }
  }
  container.textContent = tool.icon || "#";
}

function sanitizeSandboxTokens(rawTokens) {
  if (!Array.isArray(rawTokens)) {
    return DEFAULT_SANDBOX;
  }
  return rawTokens.filter((token) => typeof token === "string" && SANDBOX_ALLOWLIST.has(token));
}

function renderCategories() {
  const counts = new Map();
  for (const tool of state.allTools) {
    const cat = tool.category || "未分类";
    counts.set(cat, (counts.get(cat) || 0) + 1);
  }

  const fragment = document.createDocumentFragment();
  const makeItem = (label, value, count) => {
    const li = document.createElement("li");
    const btn = document.createElement("button");
    btn.type = "button";
    const isActive = state.filters.category === value;
    btn.className = "tui-nav-btn" + (isActive ? " active" : "");

    const nameSpan = document.createElement("span");
    nameSpan.textContent = `${isActive ? ">" : " "} ${label}`;

    const countSpan = document.createElement("span");
    countSpan.className = "tui-nav-count";
    countSpan.textContent = `[${count}]`;

    btn.append(nameSpan, countSpan);
    btn.addEventListener("click", () => {
      state.filters.category =
        value === "" ? "" : state.filters.category === value ? "" : value;
      refreshFilteredOnly();
    });

    li.appendChild(btn);
    return li;
  };

  fragment.appendChild(makeItem("ALL", "", state.allTools.length));

  const sortedCategories = Array.from(counts.keys()).sort((a, b) => a.localeCompare(b, "zh-CN"));
  for (const category of sortedCategories) {
    fragment.appendChild(makeItem(category, category, counts.get(category)));
  }

  els.categoryList.replaceChildren(fragment);
}

function renderTags() {
  const tagSet = new Set();
  for (const tool of state.allTools) {
    if (Array.isArray(tool.tags)) {
      for (const tag of tool.tags) {
        if (tag) tagSet.add(tag);
      }
    }
  }

  const sortedTags = Array.from(tagSet).sort((a, b) => a.localeCompare(b));
  const fragment = document.createDocumentFragment();

  if (sortedTags.length === 0) {
    const empty = document.createElement("span");
    empty.className = "tui-list-empty";
    empty.textContent = "(none)";
    fragment.appendChild(empty);
  } else {
    for (const tag of sortedTags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "tui-tag-chip" + (state.filters.tag === tag ? " active" : "");
      btn.textContent = `#${tag}`;
      btn.addEventListener("click", () => {
        state.filters.tag = state.filters.tag === tag ? "" : tag;
        refreshFilteredOnly();
      });
      fragment.appendChild(btn);
    }
  }

  els.tagList.replaceChildren(fragment);
  els.clearFiltersBtn.hidden = !hasActiveFilters();
}

function renderToolList() {
  els.toolsCount.textContent = String(state.filteredTools.length);
  const fragment = document.createDocumentFragment();

  if (state.filteredTools.length === 0) {
    const li = document.createElement("li");
    li.className = "tui-list-empty";
    li.textContent = state.allTools.length === 0 ? "0 tools mounted." : "No matches.";
    fragment.appendChild(li);
  } else {
    for (const tool of state.filteredTools) {
      const li = document.createElement("li");
      const btn = document.createElement("button");
      btn.type = "button";
      const isSelected = state.selectedId === tool.id;
      btn.className = "tui-tool-btn" + (isSelected ? " active" : "");
      btn.title = tool.description || tool.name;

      const row = document.createElement("div");
      row.className = "tui-tool-row";

      const iconWrap = document.createElement("span");
      iconWrap.className = "tui-tool-icon";
      renderIconInto(iconWrap, tool);

      const metaCol = document.createElement("div");
      metaCol.className = "tui-tool-meta-col";

      const nameEl = document.createElement("span");
      nameEl.className = "tui-tool-row-name";
      nameEl.textContent = `${isSelected ? "▸ " : ""}${tool.name}`;

      const idEl = document.createElement("span");
      idEl.className = "tui-tool-row-id";
      idEl.textContent = tool.id;

      metaCol.append(nameEl, idEl);
      row.append(iconWrap, metaCol);
      btn.appendChild(row);

      btn.addEventListener("click", () => {
        selectTool(tool.id, { updateHash: true, forceReload: false });
        closeMobileSidebar();
      });

      li.appendChild(btn);
      fragment.appendChild(li);
    }
  }

  els.toolList.replaceChildren(fragment);
}

function renderSidebar() {
  renderCategories();
  renderTags();
  renderToolList();
}

function buildHeroCommand() {
  const parts = ["toolstow --list"];
  if (state.filters.category) parts.push(`--cat="${state.filters.category}"`);
  if (state.filters.tag) parts.push(`--tag="${state.filters.tag}"`);
  if (state.filters.q.trim()) parts.push(`--grep="${state.filters.q.trim()}"`);
  if (parts.length === 1) parts.push("--all");
  return parts.join(" ");
}

function renderEmptyGrid() {
  const fragment = document.createDocumentFragment();
  const list = state.filteredTools;

  if (list.length === 0) {
    const box = document.createElement("div");
    box.className = "tui-card-empty";
    box.textContent =
      state.allTools.length === 0
        ? "[EMPTY] 暂无已发布工具，请管理员在后台上传发布。"
        : "[NO MATCH] 当前过滤条件下没有匹配的工具。";
    fragment.appendChild(box);
  } else {
    for (const tool of list) {
      const card = document.createElement("button");
      card.type = "button";
      card.className = "tui-card";

      const head = document.createElement("div");
      head.className = "tui-card-head";

      const titleWrap = document.createElement("div");
      titleWrap.className = "tui-card-title-wrap";

      const iconWrap = document.createElement("span");
      iconWrap.className = "tui-tool-icon";
      renderIconInto(iconWrap, tool);

      const nameEl = document.createElement("span");
      nameEl.className = "tui-card-name";
      nameEl.textContent = tool.name;

      titleWrap.append(iconWrap, nameEl);

      const verEl = document.createElement("span");
      verEl.className = "tui-mono-pill";
      verEl.textContent = tool.version ? `v${tool.version}` : "static";

      head.append(titleWrap, verEl);

      const descEl = document.createElement("p");
      descEl.className = "tui-card-desc";
      descEl.textContent = tool.description || `id: ${tool.id}`;

      const foot = document.createElement("div");
      foot.className = "tui-card-foot";

      const metaEl = document.createElement("span");
      metaEl.textContent = `${tool.category || "未分类"} · ${tool.id}`;

      const runEl = document.createElement("span");
      runEl.className = "tui-card-run";
      runEl.textContent = "[RUN ->]";

      foot.append(metaEl, runEl);
      card.append(head, descEl, foot);

      card.addEventListener("click", () => {
        selectTool(tool.id, { updateHash: true, forceReload: false });
      });

      fragment.appendChild(card);
    }
  }

  els.emptyGrid.replaceChildren(fragment);
}

function updateEmptyState() {
  const title = state.site.siteTitle || DEFAULT_SITE_TITLE;
  els.emptyTitle.textContent = title;
  els.heroCmdText.textContent = buildHeroCommand();

  const total = state.filteredTools.length;
  els.emptyCount.textContent = `${total} READY`;

  const desc = state.site.siteDescription;
  els.emptyDesc.replaceChildren();
  if (desc) {
    els.emptyDesc.appendChild(document.createTextNode(`${desc} // `));
  }
  els.emptyDesc.appendChild(document.createTextNode("选择任意条目运行，或按 "));
  const kbd = document.createElement("kbd");
  kbd.className = "tui-inline-kbd";
  kbd.textContent = "⌘K";
  els.emptyDesc.appendChild(kbd);
  els.emptyDesc.appendChild(document.createTextNode(" 快速检索。"));

  renderEmptyGrid();
}

function renderActiveToolHeader(tool) {
  if (!tool) return;
  renderIconInto(els.activeToolIcon, tool);
  els.activeToolName.textContent = tool.name;
  els.activeToolId.textContent = tool.id;
  els.activeToolVersion.textContent = tool.version ? `v${tool.version}` : "";
  els.activeToolVersion.hidden = !tool.version;
  els.activeToolThemeBadge.hidden = !(tool.themeAware || state.activeIframeReady);
  els.activeToolDesc.textContent = tool.description || "";
  els.activeToolDesc.hidden = !tool.description;

  if (isSafeToolUrl(tool.entryUrl)) {
    els.standaloneLink.href = tool.entryUrl;
    els.standaloneLink.hidden = false;
  } else {
    els.standaloneLink.removeAttribute("href");
    els.standaloneLink.hidden = true;
  }

  const tagFrag = document.createDocumentFragment();
  if (Array.isArray(tool.tags)) {
    for (const tag of tool.tags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "tui-tag-chip" + (state.filters.tag === tag ? " active" : "");
      btn.textContent = `#${tag}`;
      btn.addEventListener("click", () => {
        state.filters.tag = state.filters.tag === tag ? "" : tag;
        refreshFilteredOnly();
      });
      tagFrag.appendChild(btn);
    }
  }
  els.activeToolTags.replaceChildren(tagFrag);
}

function buildIframeUrl(entryUrl, cacheBust = false) {
  const url = new URL(entryUrl, window.location.origin);
  url.searchParams.set("theme", state.theme.mode);
  url.searchParams.set("accent", state.theme.accent);
  if (cacheBust) {
    url.searchParams.set("_ts", String(Date.now()));
  }
  return `${url.pathname}${url.search}${url.hash}`;
}

function mountToolIframe(tool, cacheBust = false) {
  if (!isSafeToolUrl(tool.entryUrl)) {
    showStatus(`无效入口路径: ${tool.id}`, "error");
    return;
  }

  state.activeIframeReady = false;
  els.activeToolThemeBadge.hidden = !tool.themeAware;

  const iframe = document.createElement("iframe");
  iframe.className = "tui-iframe";
  iframe.title = tool.name;
  iframe.setAttribute("sandbox", sanitizeSandboxTokens(tool.sandbox).join(" "));
  iframe.setAttribute("referrerpolicy", "no-referrer");

  iframe.addEventListener("load", () => {
    if (state.activeIframe === iframe) {
      postThemeToActiveIframe();
    }
  });

  iframe.src = buildIframeUrl(tool.entryUrl, cacheBust);
  state.activeIframe = iframe;
  els.iframeHost.replaceChildren(iframe);
}

function selectTool(id, { updateHash = true, forceReload = false } = {}) {
  const tool =
    state.allTools.find((t) => t.id === id) ||
    state.filteredTools.find((t) => t.id === id);

  if (!tool) {
    showStatus(`未找到工具: ${id}`, "error");
    clearSelection(false);
    return;
  }

  const sameTool = state.selectedId === tool.id;
  state.selectedId = tool.id;

  if (updateHash) {
    const expectedHash = `#tool=${encodeURIComponent(tool.id)}`;
    if (window.location.hash !== expectedHash) {
      window.location.hash = expectedHash;
    }
  }

  els.emptyState.hidden = true;
  els.toolView.hidden = false;
  updateDocumentTitle(tool);
  renderActiveToolHeader(tool);
  renderToolList();
  updateFooterStats();

  if (!sameTool || forceReload || !state.activeIframe) {
    mountToolIframe(tool, forceReload);
  }
}

function clearSelection(updateHash = true) {
  state.selectedId = null;
  state.activeIframe = null;
  state.activeIframeReady = false;
  els.iframeHost.replaceChildren();
  els.toolView.hidden = true;
  els.emptyState.hidden = false;
  updateDocumentTitle(null);
  updateEmptyState();
  renderToolList();
  updateFooterStats();

  if (updateHash && window.location.hash) {
    history.replaceState(null, "", window.location.pathname + window.location.search);
  }
}

function parseToolIdFromHash() {
  const hash = window.location.hash || "";
  const match = hash.match(/^#tool=(.+)$/);
  if (!match) return null;
  try {
    return decodeURIComponent(match[1]);
  } catch {
    return match[1];
  }
}

function openMobileSidebar() {
  els.sidebar.classList.add("open");
  els.sidebarBackdrop.hidden = false;
  els.sidebarToggle.setAttribute("aria-expanded", "true");
}

function closeMobileSidebar() {
  els.sidebar.classList.remove("open");
  els.sidebarBackdrop.hidden = true;
  els.sidebarToggle.setAttribute("aria-expanded", "false");
}

function bindEvents() {
  els.brandLink.addEventListener("click", (e) => {
    e.preventDefault();
    clearSelection(true);
    closeMobileSidebar();
  });

  els.sidebarToggle.addEventListener("click", () => {
    if (els.sidebar.classList.contains("open")) {
      closeMobileSidebar();
    } else {
      openMobileSidebar();
    }
  });

  els.sidebarBackdrop.addEventListener("click", closeMobileSidebar);

  els.mobileBackBtn.addEventListener("click", () => {
    clearSelection(true);
    openMobileSidebar();
  });

  els.closeToolBtn.addEventListener("click", () => {
    clearSelection(true);
  });

  els.themeToggle.addEventListener("click", () => {
    setThemeMode(state.theme.mode === "dark" ? "light" : "dark");
  });

  els.accentInput.addEventListener("input", (e) => {
    setAccentColor(e.target.value);
  });

  els.accentInput.addEventListener("change", (e) => {
    setAccentColor(e.target.value);
  });

  els.searchInput.addEventListener("input", (e) => {
    state.filters.q = e.target.value;
    if (window.innerWidth <= 780 && state.selectedId && !els.sidebar.classList.contains("open")) {
      openMobileSidebar();
    }
    if (state.searchTimer) window.clearTimeout(state.searchTimer);
    state.searchTimer = window.setTimeout(() => {
      refreshFilteredOnly();
    }, 150);
  });

  els.searchInput.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      if (els.searchInput.value) {
        els.searchInput.value = "";
        state.filters.q = "";
        refreshFilteredOnly();
      } else {
        els.searchInput.blur();
      }
    }
  });

  window.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      els.searchInput.focus();
      els.searchInput.select();
    }
  });

  els.refreshListBtn.addEventListener("click", () => {
    refreshAllAndFiltered(true);
  });

  els.clearFiltersBtn.addEventListener("click", () => {
    state.filters.q = "";
    state.filters.category = "";
    state.filters.tag = "";
    els.searchInput.value = "";
    refreshFilteredOnly();
  });

  els.reloadToolBtn.addEventListener("click", () => {
    const tool = getSelectedTool();
    if (tool) mountToolIframe(tool, true);
  });

  window.addEventListener("message", (event) => {
    if (!state.activeIframe || event.source !== state.activeIframe.contentWindow) return;
    if (event.data && event.data.type === "toolstow:ready") {
      state.activeIframeReady = true;
      els.activeToolThemeBadge.hidden = false;
      postThemeToActiveIframe();
    }
  });

  window.addEventListener("hashchange", () => {
    const id = parseToolIdFromHash();
    if (id) {
      if (id !== state.selectedId) {
        selectTool(id, { updateHash: false, forceReload: false });
      }
    } else if (state.selectedId) {
      clearSelection(false);
    }
  });
}

async function bootstrap() {
  initTheme();
  bindEvents();

  await Promise.all([fetchSiteMe(), refreshAllAndFiltered(false)]);

  const initialId = parseToolIdFromHash();
  if (initialId) {
    selectTool(initialId, { updateHash: false, forceReload: false });
  } else {
    updateEmptyState();
  }
}

bootstrap();
