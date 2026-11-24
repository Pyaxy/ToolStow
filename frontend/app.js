const CLIENT_HEADER = { "X-ToolStow-Client": "1" };

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
const DEFAULT_ACCENT = "#2563eb";

const state = {
  allTools: [],
  filteredTools: [],
  selectedId: null,
  activeIframe: null,
  filters: {
    q: "",
    category: "",
    tag: "",
  },
  theme: {
    mode: "light",
    accent: DEFAULT_ACCENT,
  },
  statusTimer: null,
  searchTimer: null,
  dragDepth: 0,
};

const els = {
  brandLink: document.getElementById("brand-link"),
  sidebarToggle: document.getElementById("sidebar-toggle"),
  sidebar: document.getElementById("sidebar"),
  sidebarBackdrop: document.getElementById("sidebar-backdrop"),
  searchInput: document.getElementById("search-input"),
  uploadTrigger: document.getElementById("upload-trigger"),
  emptyUploadBtn: document.getElementById("empty-upload-btn"),
  uploadInput: document.getElementById("upload-input"),
  dropzone: document.getElementById("dropzone"),
  dropOverlay: document.getElementById("drop-overlay"),
  accentInput: document.getElementById("accent-input"),
  themeToggle: document.getElementById("theme-toggle"),
  statusBanner: document.getElementById("status-banner"),
  refreshListBtn: document.getElementById("refresh-list-btn"),
  clearFiltersBtn: document.getElementById("clear-filters-btn"),
  categoryList: document.getElementById("category-list"),
  tagList: document.getElementById("tag-list"),
  toolList: document.getElementById("tool-list"),
  toolsCount: document.getElementById("tools-count"),
  emptyState: document.getElementById("empty-state"),
  emptyTitle: document.getElementById("empty-title"),
  emptyDesc: document.getElementById("empty-desc"),
  toolView: document.getElementById("tool-view"),
  activeToolIcon: document.getElementById("active-tool-icon"),
  activeToolName: document.getElementById("active-tool-name"),
  activeToolVersion: document.getElementById("active-tool-version"),
  activeToolThemeBadge: document.getElementById("active-tool-theme-badge"),
  activeToolDesc: document.getElementById("active-tool-desc"),
  activeToolTags: document.getElementById("active-tool-tags"),
  reloadToolBtn: document.getElementById("reload-tool-btn"),
  standaloneLink: document.getElementById("standalone-link"),
  deleteToolBtn: document.getElementById("delete-tool-btn"),
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
    // Ignore storage errors (private mode / quota).
  }
}

function isValidHexColor(value) {
  return typeof value === "string" && /^#[0-9a-fA-F]{6}$/.test(value);
}

function initTheme() {
  const savedMode = readStorage(STORAGE_KEY_MODE);
  if (savedMode === "dark" || savedMode === "light") {
    state.theme.mode = savedMode;
  } else if (window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches) {
    state.theme.mode = "dark";
  }

  const savedAccent = readStorage(STORAGE_KEY_ACCENT);
  if (isValidHexColor(savedAccent)) {
    state.theme.accent = savedAccent;
  }

  applyThemeToHost();
}

function applyThemeToHost() {
  document.documentElement.dataset.theme = state.theme.mode;
  document.documentElement.style.setProperty("--accent", state.theme.accent);
  els.accentInput.value = state.theme.accent;
  els.themeToggle.textContent = state.theme.mode === "dark" ? "亮色" : "暗色";
}

function setThemeMode(mode) {
  state.theme.mode = mode === "dark" ? "dark" : "light";
  writeStorage(STORAGE_KEY_MODE, state.theme.mode);
  applyThemeToHost();
  postThemeToActiveIframe();
}

function setAccentColor(accent) {
  if (!isValidHexColor(accent)) return;
  state.theme.accent = accent;
  writeStorage(STORAGE_KEY_ACCENT, state.theme.accent);
  applyThemeToHost();
  postThemeToActiveIframe();
}

function getSelectedTool() {
  if (!state.selectedId) return null;
  return (
    state.allTools.find((t) => t.id === state.selectedId) ||
    state.filteredTools.find((t) => t.id === state.selectedId) ||
    null
  );
}

function postThemeToActiveIframe() {
  const tool = getSelectedTool();
  if (!tool || !tool.themeAware) return;
  const iframe = state.activeIframe;
  if (!iframe || !iframe.contentWindow) return;

  iframe.contentWindow.postMessage(
    {
      type: "toolstow:theme",
      payload: {
        mode: state.theme.mode,
        accent: state.theme.accent,
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

async function fetchToolsFromApi(filters = {}) {
  const params = new URLSearchParams();
  if (filters.q && filters.q.trim()) params.set("q", filters.q.trim());
  if (filters.category) params.set("category", filters.category);
  if (filters.tag) params.set("tag", filters.tag);

  const query = params.toString();
  const url = query ? `/api/tools?${query}` : "/api/tools";
  const res = await fetch(url, { headers: CLIENT_HEADER, cache: "no-store" });
  if (!res.ok) {
    let body = null;
    try {
      body = await res.json();
    } catch {
      // ignore non-json error body
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

    if (hasActiveFilters()) {
      state.filteredTools = await fetchToolsFromApi(state.filters);
    } else {
      state.filteredTools = allTools;
    }

    renderSidebar();

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
  if (typeof tool.iconUrl === "string" && tool.iconUrl.startsWith("/t/")) {
    return tool.iconUrl;
  }
  if (typeof tool.icon === "string" && tool.icon.startsWith("/t/")) {
    return tool.icon;
  }
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
    const img = document.createElement("img");
    img.src = resolveIconUrl(tool);
    img.alt = "";
    img.loading = "lazy";
    img.addEventListener("error", () => {
      container.replaceChildren(document.createTextNode("🛠️"));
    });
    container.appendChild(img);
    return;
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

  const fragment = document.createDocumentFragment();

  const allItem = document.createElement("li");
  const allBtn = document.createElement("button");
  allBtn.type = "button";
  allBtn.className = "nav-item-btn" + (state.filters.category === "" ? " active" : "");
  const allLabel = document.createElement("span");
  allLabel.textContent = "全部";
  const allCount = document.createElement("span");
  allCount.className = "muted";
  allCount.textContent = String(state.allTools.length);
  allBtn.append(allLabel, allCount);
  allBtn.addEventListener("click", () => {
    if (state.filters.category !== "") {
      state.filters.category = "";
      refreshFilteredOnly();
    }
  });
  allItem.appendChild(allBtn);
  fragment.appendChild(allItem);

  const sortedCategories = Array.from(counts.keys()).sort((a, b) => a.localeCompare(b, "zh-CN"));
  for (const category of sortedCategories) {
    const li = document.createElement("li");
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "nav-item-btn" + (state.filters.category === category ? " active" : "");

    const nameSpan = document.createElement("span");
    nameSpan.textContent = category;
    const countSpan = document.createElement("span");
    countSpan.className = "muted";
    countSpan.textContent = String(counts.get(category));

    btn.append(nameSpan, countSpan);
    btn.addEventListener("click", () => {
      state.filters.category = state.filters.category === category ? "" : category;
      refreshFilteredOnly();
    });

    li.appendChild(btn);
    fragment.appendChild(li);
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
    empty.className = "muted";
    empty.textContent = "暂无标签";
    fragment.appendChild(empty);
  } else {
    for (const tag of sortedTags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "tag-chip" + (state.filters.tag === tag ? " active" : "");
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
    li.className = "muted";
    li.style.padding = "8px";
    li.textContent = state.allTools.length === 0 ? "尚未安装任何工具" : "没有匹配的工具";
    fragment.appendChild(li);
  } else {
    for (const tool of state.filteredTools) {
      const li = document.createElement("li");
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "tool-item-btn" + (state.selectedId === tool.id ? " active" : "");
      btn.title = tool.description || tool.name;

      const main = document.createElement("div");
      main.className = "tool-item-main";

      const iconWrap = document.createElement("span");
      iconWrap.className = "tool-icon";
      renderIconInto(iconWrap, tool);

      const textWrap = document.createElement("div");
      textWrap.className = "tool-item-text";

      const nameEl = document.createElement("span");
      nameEl.className = "tool-item-name";
      nameEl.textContent = tool.name;

      const descEl = document.createElement("span");
      descEl.className = "tool-item-desc";
      descEl.textContent = tool.description || tool.id;

      textWrap.append(nameEl, descEl);
      main.append(iconWrap, textWrap);
      btn.appendChild(main);

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

function updateEmptyState() {
  if (state.allTools.length === 0) {
    els.emptyTitle.textContent = "工具箱为空";
    els.emptyDesc.textContent =
      "将工具目录放入 ./tools，或拖拽 / 点击上传符合规范的 .zip 工具包即可立即使用。";
  } else {
    els.emptyTitle.textContent = "选择一个工具开始使用";
    els.emptyDesc.textContent =
      "从左侧列表选择已安装的工具（按 Cmd/Ctrl+K 快速检索），或拖拽 .zip 上传更新工具。";
  }
}

function renderActiveToolHeader(tool) {
  if (!tool) return;
  renderIconInto(els.activeToolIcon, tool);
  els.activeToolName.textContent = tool.name;
  els.activeToolVersion.textContent = tool.version ? `v${tool.version}` : "";
  els.activeToolVersion.hidden = !tool.version;
  els.activeToolThemeBadge.hidden = !tool.themeAware;
  els.activeToolDesc.textContent = tool.description || "";
  els.activeToolDesc.hidden = !tool.description;
  els.standaloneLink.href = tool.entryUrl;

  const tagFrag = document.createDocumentFragment();
  if (Array.isArray(tool.tags)) {
    for (const tag of tool.tags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "tag-chip" + (state.filters.tag === tag ? " active" : "");
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

function mountToolIframe(tool, cacheBust = false) {
  const iframe = document.createElement("iframe");
  iframe.className = "tool-iframe";
  iframe.title = tool.name;
  iframe.setAttribute("sandbox", sanitizeSandboxTokens(tool.sandbox).join(" "));
  iframe.setAttribute("referrerpolicy", "no-referrer");

  iframe.addEventListener("load", () => {
    if (state.activeIframe === iframe) {
      postThemeToActiveIframe();
    }
  });

  const targetUrl = cacheBust
    ? `${tool.entryUrl}${tool.entryUrl.includes("?") ? "&" : "?"}_ts=${Date.now()}`
    : tool.entryUrl;
  iframe.src = targetUrl;

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

  els.emptyState.hidden = true;
  els.toolView.hidden = false;
  renderActiveToolHeader(tool);
  renderToolList();

  if (!sameTool || forceReload || !state.activeIframe) {
    mountToolIframe(tool, forceReload);
  }
}

function clearSelection(updateHash = true) {
  state.selectedId = null;
  state.activeIframe = null;
  els.iframeHost.replaceChildren();
  els.toolView.hidden = true;
  els.emptyState.hidden = false;
  updateEmptyState();
  renderToolList();

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

async function uploadZipFile(file) {
  if (!file) return;
  if (!file.name.toLowerCase().endsWith(".zip")) {
    showStatus("请上传 .zip 格式的工具压缩包", "error");
    return;
  }

  showStatus(`正在上传 ${file.name}...`, "info", 0);

  const formData = new FormData();
  formData.append("file", file, file.name);

  try {
    const res = await fetch("/api/tools/upload", {
      method: "POST",
      headers: CLIENT_HEADER,
      body: formData,
    });

    let body = null;
    try {
      body = await res.json();
    } catch {
      // ignore
    }

    if (!res.ok) {
      throw new Error(parseErrorPayload(body, `上传失败 (HTTP ${res.status})`));
    }

    const uploadedTool = body && body.id ? body : body && body.entry ? body.entry : null;
    const actionText = res.status === 201 ? "已安装" : "已覆盖更新";
    const label = uploadedTool ? `${uploadedTool.name} (${uploadedTool.id})` : file.name;
    showStatus(`${actionText}：${label}`, "success");

    await refreshAllAndFiltered(true);
    if (uploadedTool && uploadedTool.id) {
      selectTool(uploadedTool.id, { updateHash: true, forceReload: true });
    }
  } catch (err) {
    showStatus(err.message || "上传失败", "error", 7000);
  } finally {
    els.uploadInput.value = "";
  }
}

async function deleteActiveTool() {
  const tool = getSelectedTool();
  if (!tool) return;

  const confirmed = window.confirm(`确定要删除工具“${tool.name}” (${tool.id}) 吗？此操作将移除工具目录。`);
  if (!confirmed) return;

  showStatus(`正在删除 ${tool.id}...`, "info", 0);

  try {
    const res = await fetch(`/api/tools/${encodeURIComponent(tool.id)}`, {
      method: "DELETE",
      headers: CLIENT_HEADER,
    });

    if (!res.ok && res.status !== 204) {
      let body = null;
      try {
        body = await res.json();
      } catch {
        // ignore
      }
      throw new Error(parseErrorPayload(body, `删除失败 (HTTP ${res.status})`));
    }

    showStatus(`已删除工具：${tool.name} (${tool.id})`, "success");
    clearSelection(true);
    await refreshAllAndFiltered(false);
  } catch (err) {
    showStatus(err.message || "删除失败", "error", 7000);
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
  });

  els.sidebarToggle.addEventListener("click", () => {
    if (els.sidebar.classList.contains("open")) {
      closeMobileSidebar();
    } else {
      openMobileSidebar();
    }
  });
  els.sidebarBackdrop.addEventListener("click", closeMobileSidebar);

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
    if (window.innerWidth <= 768 && !els.sidebar.classList.contains("open")) {
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

  els.deleteToolBtn.addEventListener("click", deleteActiveTool);

  const openFilePicker = () => els.uploadInput.click();
  els.uploadTrigger.addEventListener("click", openFilePicker);
  els.emptyUploadBtn.addEventListener("click", openFilePicker);
  els.dropzone.addEventListener("click", openFilePicker);
  els.dropzone.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      openFilePicker();
    }
  });

  els.uploadInput.addEventListener("change", (e) => {
    const file = e.target.files && e.target.files[0];
    if (file) uploadZipFile(file);
  });

  // Drag & drop on dropzone and whole window
  const hasFiles = (e) => e.dataTransfer && Array.from(e.dataTransfer.types || []).includes("Files");

  window.addEventListener("dragenter", (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    state.dragDepth += 1;
    els.dropOverlay.hidden = false;
    els.dropzone.classList.add("drag-over");
  });

  window.addEventListener("dragover", (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
  });

  window.addEventListener("dragleave", (e) => {
    if (!hasFiles(e)) return;
    state.dragDepth = Math.max(0, state.dragDepth - 1);
    if (state.dragDepth === 0) {
      els.dropOverlay.hidden = true;
      els.dropzone.classList.remove("drag-over");
    }
  });

  window.addEventListener("drop", (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    state.dragDepth = 0;
    els.dropOverlay.hidden = true;
    els.dropzone.classList.remove("drag-over");
    const file = e.dataTransfer.files && e.dataTransfer.files[0];
    if (file) uploadZipFile(file);
  });

  // Handshake from theme-aware child tool iframe
  window.addEventListener("message", (event) => {
    if (!state.activeIframe || event.source !== state.activeIframe.contentWindow) return;
    if (event.data && event.data.type === "toolstow:ready") {
      postThemeToActiveIframe();
    }
  });

  // Hash navigation (#tool=<id>)
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
  await refreshAllAndFiltered(false);

  const initialId = parseToolIdFromHash();
  if (initialId) {
    selectTool(initialId, { updateHash: false, forceReload: false });
  } else {
    updateEmptyState();
  }
}

bootstrap();
