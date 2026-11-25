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
const THEME_ACCENT_LIGHT = "#2563eb";
const THEME_ACCENT_DARK = "#3b82f6";
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
    isAdmin: false,
    adminEnabled: false,
    activeTheme: "serverstatus",
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
    mode: "light",
    accent: THEME_ACCENT_LIGHT,
  },
  statusTimer: null,
  searchTimer: null,
};

const els = {
  brandLink: document.getElementById("brand-link"),
  brandTitle: document.getElementById("brand-title"),
  siteDescPill: document.getElementById("site-desc-pill"),
  searchInput: document.getElementById("search-input"),
  themeToggle: document.getElementById("theme-toggle"),
  adminLink: document.getElementById("admin-link"),
  statusBanner: document.getElementById("status-banner"),
  directoryView: document.getElementById("directory-view"),
  statTotal: document.getElementById("stat-total"),
  statShown: document.getElementById("stat-shown"),
  statCats: document.getElementById("stat-cats"),
  categoryTabs: document.getElementById("category-tabs"),
  tagList: document.getElementById("tag-list"),
  clearFiltersBtn: document.getElementById("clear-filters-btn"),
  refreshListBtn: document.getElementById("refresh-list-btn"),
  toolTableBody: document.getElementById("tool-table-body"),
  toolView: document.getElementById("tool-view"),
  backToListBtn: document.getElementById("back-to-list-btn"),
  quickSwitcher: document.getElementById("quick-switcher"),
  activeToolIcon: document.getElementById("active-tool-icon"),
  activeToolName: document.getElementById("active-tool-name"),
  activeToolId: document.getElementById("active-tool-id"),
  activeToolVersion: document.getElementById("active-tool-version"),
  activeToolThemeBadge: document.getElementById("active-tool-theme-badge"),
  activeToolDesc: document.getElementById("active-tool-desc"),
  activeToolTags: document.getElementById("active-tool-tags"),
  reloadToolBtn: document.getElementById("reload-tool-btn"),
  standaloneLink: document.getElementById("standalone-link"),
  iframeHost: document.getElementById("iframe-host"),
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
  } else if (window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches) {
    state.theme.mode = "dark";
  }

  applyThemeToHost();
}

function applyThemeToHost() {
  document.documentElement.dataset.theme = state.theme.mode;
  const computed = normalizeHexColor(
    window.getComputedStyle(document.documentElement).getPropertyValue("--ts-accent")
  );
  state.theme.accent = computed || defaultAccentForMode(state.theme.mode);
  els.themeToggle.textContent = state.theme.mode === "dark" ? "亮色" : "暗色";
}

function setThemeMode(mode) {
  state.theme.mode = mode === "dark" ? "dark" : "light";
  writeStorage(STORAGE_KEY_MODE, state.theme.mode);
  applyThemeToHost();
  postThemeToActiveIframe();
}

function updateDocumentTitle(tool = null) {
  const siteTitle = state.site.siteTitle || DEFAULT_SITE_TITLE;
  document.title = tool ? `${tool.name} · ${siteTitle}` : siteTitle;
}

function applySiteInfo() {
  const title = state.site.siteTitle || DEFAULT_SITE_TITLE;
  els.brandTitle.textContent = title;
  els.siteDescPill.textContent = state.site.siteDescription || "";
  els.adminLink.hidden = state.site.isAdmin !== true;
  updateDocumentTitle(getSelectedTool());
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
  els.statusBanner.textContent = message;
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
      activeTheme: typeof data.activeTheme === "string" ? data.activeTheme : "serverstatus",
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

async function refreshAllAndFiltered(keepSelection = true) {
  try {
    const allTools = await fetchToolsFromApi({});
    state.allTools = allTools;
    state.filteredTools = hasActiveFilters()
      ? await fetchToolsFromApi(state.filters)
      : allTools;

    renderDirectory();
    renderQuickSwitcher();

    if (keepSelection && state.selectedId) {
      const stillExists = state.allTools.some((t) => t.id === state.selectedId);
      if (!stillExists) {
        clearSelection(true);
      } else {
        renderActiveToolHeader(getSelectedTool());
      }
    }
  } catch (err) {
    showStatus(err.message || "无法连接到 ToolStow 服务", "error", 0);
  }
}

async function refreshFilteredOnly() {
  try {
    state.filteredTools = await fetchToolsFromApi(state.filters);
    renderDirectory();
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
        container.replaceChildren(document.createTextNode("🛠️"));
      });
      container.appendChild(img);
      return;
    }
  }
  container.textContent = tool.icon || "🛠️";
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

  els.statTotal.textContent = String(state.allTools.length);
  els.statShown.textContent = String(state.filteredTools.length);
  els.statCats.textContent = String(counts.size);

  const fragment = document.createDocumentFragment();
  const makeTab = (label, value, count) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.setAttribute("role", "tab");
    const isActive = state.filters.category === value;
    btn.className = "ops-cat-tab" + (isActive ? " active" : "");
    btn.setAttribute("aria-selected", isActive ? "true" : "false");

    const nameSpan = document.createElement("span");
    nameSpan.textContent = label;
    const countSpan = document.createElement("span");
    countSpan.className = "ops-cat-count";
    countSpan.textContent = String(count);

    btn.append(nameSpan, countSpan);
    btn.addEventListener("click", () => {
      state.filters.category =
        value === "" ? "" : state.filters.category === value ? "" : value;
      refreshFilteredOnly();
    });
    return btn;
  };

  fragment.appendChild(makeTab("全部", "", state.allTools.length));

  const sortedCategories = Array.from(counts.keys()).sort((a, b) => a.localeCompare(b, "zh-CN"));
  for (const category of sortedCategories) {
    fragment.appendChild(makeTab(category, category, counts.get(category)));
  }

  els.categoryTabs.replaceChildren(fragment);
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
    empty.className = "ops-tag-empty";
    empty.textContent = "暂无标签";
    fragment.appendChild(empty);
  } else {
    for (const tag of sortedTags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "ops-tag-pill" + (state.filters.tag === tag ? " active" : "");
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

function renderTable() {
  const fragment = document.createDocumentFragment();

  if (state.filteredTools.length === 0) {
    const tr = document.createElement("tr");
    const td = document.createElement("td");
    td.colSpan = 6;
    td.className = "ops-empty-cell";
    td.textContent =
      state.allTools.length === 0
        ? "暂无已发布的工具。管理员可在后台上传并发布工具包。"
        : "当前筛选条件下没有匹配的工具。";
    tr.appendChild(td);
    fragment.appendChild(tr);
  } else {
    for (const tool of state.filteredTools) {
      const tr = document.createElement("tr");
      tr.className = "ops-row";

      // Column 1: Icon + Name + Desc
      const tdTool = document.createElement("td");
      tdTool.className = "col-tool";
      const toolCell = document.createElement("div");
      toolCell.className = "ops-tool-cell";

      const dot = document.createElement("span");
      dot.className = "ops-row-dot";
      dot.setAttribute("aria-hidden", "true");

      const iconWrap = document.createElement("span");
      iconWrap.className = "ops-tool-icon";
      renderIconInto(iconWrap, tool);

      const textWrap = document.createElement("div");
      textWrap.className = "ops-tool-cell-text";

      const nameEl = document.createElement("span");
      nameEl.className = "ops-tool-cell-name";
      nameEl.textContent = tool.name;

      const descEl = document.createElement("span");
      descEl.className = "ops-tool-cell-desc";
      descEl.textContent = tool.description || "点击打开此微工具";

      textWrap.append(nameEl, descEl);
      toolCell.append(dot, iconWrap, textWrap);
      tdTool.appendChild(toolCell);

      // Column 2: ID
      const tdId = document.createElement("td");
      tdId.className = "col-id";
      const idCode = document.createElement("code");
      idCode.className = "ops-code-pill";
      idCode.textContent = tool.id;
      tdId.appendChild(idCode);

      // Column 3: Category
      const tdCat = document.createElement("td");
      tdCat.className = "col-cat";
      const catBadge = document.createElement("span");
      catBadge.className = "ops-cat-badge";
      catBadge.textContent = tool.category || "未分类";
      tdCat.appendChild(catBadge);

      // Column 4: Tags
      const tdTags = document.createElement("td");
      tdTags.className = "col-tags";
      const tagsWrap = document.createElement("div");
      tagsWrap.className = "ops-row-tags";
      if (Array.isArray(tool.tags) && tool.tags.length > 0) {
        for (const tag of tool.tags.slice(0, 4)) {
          const pill = document.createElement("button");
          pill.type = "button";
          pill.className = "ops-tag-pill" + (state.filters.tag === tag ? " active" : "");
          pill.textContent = `#${tag}`;
          pill.addEventListener("click", (e) => {
            e.stopPropagation();
            state.filters.tag = state.filters.tag === tag ? "" : tag;
            refreshFilteredOnly();
          });
          tagsWrap.appendChild(pill);
        }
      } else {
        const dash = document.createElement("span");
        dash.className = "ops-tag-empty";
        dash.textContent = "—";
        tagsWrap.appendChild(dash);
      }
      tdTags.appendChild(tagsWrap);

      // Column 5: Version + Sync
      const tdVer = document.createElement("td");
      tdVer.className = "col-ver";
      const verWrap = document.createElement("div");
      verWrap.className = "ops-ver-cell";
      const verCode = document.createElement("span");
      verCode.className = "ops-code-pill";
      verCode.textContent = tool.version ? `v${tool.version}` : "static";
      verWrap.appendChild(verCode);
      if (tool.themeAware) {
        const syncPill = document.createElement("span");
        syncPill.className = "ops-sync-pill";
        syncPill.textContent = "SYNC";
        verWrap.appendChild(syncPill);
      }
      tdVer.appendChild(verWrap);

      // Column 6: Actions
      const tdActions = document.createElement("td");
      tdActions.className = "col-actions";
      const actionsWrap = document.createElement("div");
      actionsWrap.className = "ops-row-actions";

      const openBtn = document.createElement("button");
      openBtn.type = "button";
      openBtn.className = "ops-btn ops-btn-xs ops-btn-primary";
      openBtn.textContent = "打开";
      openBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        selectTool(tool.id, { updateHash: true, forceReload: false });
      });
      actionsWrap.appendChild(openBtn);

      if (isSafeToolUrl(tool.entryUrl)) {
        const extLink = document.createElement("a");
        extLink.className = "ops-btn ops-btn-xs";
        extLink.href = tool.entryUrl;
        extLink.target = "_blank";
        extLink.rel = "noopener noreferrer";
        extLink.title = "独立打开";
        extLink.textContent = "↗";
        extLink.addEventListener("click", (e) => e.stopPropagation());
        actionsWrap.appendChild(extLink);
      }

      tdActions.appendChild(actionsWrap);

      tr.append(tdTool, tdId, tdCat, tdTags, tdVer, tdActions);
      tr.addEventListener("click", () => {
        selectTool(tool.id, { updateHash: true, forceReload: false });
      });

      fragment.appendChild(tr);
    }
  }

  els.toolTableBody.replaceChildren(fragment);
}

function renderDirectory() {
  renderCategories();
  renderTags();
  renderTable();
}

function renderQuickSwitcher() {
  const fragment = document.createDocumentFragment();
  for (const tool of state.allTools) {
    const opt = document.createElement("option");
    opt.value = tool.id;
    opt.textContent = `${tool.name} (${tool.id})`;
    if (tool.id === state.selectedId) {
      opt.selected = true;
    }
    fragment.appendChild(opt);
  }
  els.quickSwitcher.replaceChildren(fragment);
  els.quickSwitcher.hidden = state.allTools.length === 0;
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

  if (els.quickSwitcher.value !== tool.id) {
    els.quickSwitcher.value = tool.id;
  }

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
      btn.className = "ops-tag-pill" + (state.filters.tag === tag ? " active" : "");
      btn.textContent = `#${tag}`;
      btn.addEventListener("click", () => {
        state.filters.tag = state.filters.tag === tag ? "" : tag;
        clearSelection(true);
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
    showStatus(`工具入口路径无效：${tool.id}`, "error");
    return;
  }

  state.activeIframeReady = false;
  els.activeToolThemeBadge.hidden = !tool.themeAware;

  const iframe = document.createElement("iframe");
  iframe.className = "ops-iframe";
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
    showStatus(`未找到工具：${id}`, "error");
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

  els.directoryView.hidden = true;
  els.toolView.hidden = false;
  updateDocumentTitle(tool);
  renderQuickSwitcher();
  renderActiveToolHeader(tool);

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
  els.directoryView.hidden = false;
  updateDocumentTitle(null);

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

function bindEvents() {
  els.brandLink.addEventListener("click", (e) => {
    e.preventDefault();
    clearSelection(true);
  });

  els.backToListBtn.addEventListener("click", () => {
    clearSelection(true);
  });

  els.quickSwitcher.addEventListener("change", (e) => {
    const nextId = e.target.value;
    if (nextId) {
      selectTool(nextId, { updateHash: true, forceReload: false });
    }
  });

  els.themeToggle.addEventListener("click", () => {
    setThemeMode(state.theme.mode === "dark" ? "light" : "dark");
  });

  els.searchInput.addEventListener("input", (e) => {
    state.filters.q = e.target.value;
    if (state.selectedId) {
      clearSelection(true);
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
  }
}

bootstrap();
