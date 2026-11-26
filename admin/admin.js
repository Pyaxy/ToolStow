const CLIENT_HEADERS = { "X-ToolStow-Client": "1" };
const STORAGE_KEY_MODE = "toolstow:mode";
const STORAGE_KEY_AUTO_UPDATE_CHECK = "toolstow:auto-update-check";
const STORAGE_KEY_UPDATE_RESULT = "toolstow:update-result";
const UPDATE_CHECK_INTERVAL = 24 * 60 * 60 * 1000;
const RELEASES_URL = "https://github.com/Pyaxy/ToolStow/releases/tag/";
const LATEST_RELEASE_API = "https://api.github.com/repos/Pyaxy/ToolStow/releases/latest";
const STABLE_RELEASE_TAG = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const RUNNING_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

const state = {
  authenticated: false,
  adminEnabled: true,
  tools: [],
  themes: [],
  filter: "all", // "all" | "published" | "draft"
  search: "",
  themeMode: "light",
  pendingConfirmAction: null,
  toastTimer: null,
  dragDepth: 0,
  themeDragDepth: 0,
  activeTab: "tools",
  html: null, // active single-HTML ingestion session (see openHtmlDialog)
  updateCheck: null,
};

const TAB_NAMES = ["tools", "themes", "settings"];
const TOOL_ID_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const MAX_HTML_BYTES = 8 * 1024 * 1024;
const SDK_CSS_PATH = "/ui/v1/toolstow.css";
const DEFAULT_TOOL_ICON = "🛠️";
const STANDARD_CATEGORIES = ["效率", "开发", "文本", "设计", "数据", "实用工具", "学习", "娱乐"];
const ADMIN_COLOR_TOKENS = [
  "--ts-canvas", "--ts-surface", "--ts-surface-hover", "--ts-border", "--ts-border-strong",
  "--ts-ink", "--ts-ink-secondary", "--ts-ink-muted", "--ts-inverted-bg", "--ts-inverted-bg-hover",
  "--ts-inverted-ink", "--ts-accent", "--ts-danger", "--ts-danger-bg", "--ts-danger-border",
  "--ts-success", "--ts-code-bg", "--ts-code-ink",
];

const els = {
  themePalette: document.getElementById("theme-palette"),
  bootView: document.getElementById("boot-view"),
  loginView: document.getElementById("login-view"),
  dashboardView: document.getElementById("dashboard-view"),

  // Login elements
  loginThemeToggle: document.getElementById("login-theme-toggle"),
  loginDisabledNotice: document.getElementById("login-disabled-notice"),
  loginError: document.getElementById("login-error"),
  loginForm: document.getElementById("login-form"),
  adminKeyInput: document.getElementById("admin-key-input"),
  toggleKeyVisibility: document.getElementById("toggle-key-visibility"),
  loginSubmitBtn: document.getElementById("login-submit-btn"),

  // Dashboard topbar & toast
  dashThemeToggle: document.getElementById("dash-theme-toggle"),
  logoutBtn: document.getElementById("logout-btn"),
  globalToast: document.getElementById("global-toast"),

  // Stats
  statTotal: document.getElementById("stat-total"),
  statPublished: document.getElementById("stat-published"),
  statDraft: document.getElementById("stat-draft"),

  // Upload
  uploadPickBtn: document.getElementById("upload-pick-btn"),
  uploadFileInput: document.getElementById("upload-file-input"),
  uploadDropzone: document.getElementById("upload-dropzone"),
  uploadStatus: document.getElementById("upload-status"),
  pasteHtmlBtn: document.getElementById("paste-html-btn"),

  // Top-level tabs
  mainTabs: Array.from(document.querySelectorAll(".main-tab")),
  tabPanels: Array.from(document.querySelectorAll(".tab-panel")),

  // Single-HTML ingestion dialog
  htmlDialog: document.getElementById("html-dialog"),
  htmlForm: document.getElementById("html-form"),
  htmlCloseBtn: document.getElementById("html-close-btn"),
  htmlCancelBtn: document.getElementById("html-cancel-btn"),
  htmlSubmitBtn: document.getElementById("html-submit-btn"),
  htmlSourceField: document.getElementById("html-source-field"),
  htmlSource: document.getElementById("html-source"),
  htmlName: document.getElementById("html-name"),
  htmlId: document.getElementById("html-id"),
  htmlIdHint: document.getElementById("html-id-hint"),
  htmlCategory: document.getElementById("html-category"),
  htmlCategoryOptions: document.getElementById("html-category-options"),
  htmlIcon: document.getElementById("html-icon"),
  htmlDesc: document.getElementById("html-desc"),
  htmlDraft: document.getElementById("html-draft"),
  htmlInjectSdk: document.getElementById("html-inject-sdk"),
  htmlError: document.getElementById("html-error"),
  htmlPreviewFrame: document.getElementById("html-preview-frame"),

  // Tools Manager
  filterTabs: Array.from(document.querySelectorAll(".filter-tab")),
  tabCountAll: document.getElementById("tab-count-all"),
  tabCountPublished: document.getElementById("tab-count-published"),
  tabCountDraft: document.getElementById("tab-count-draft"),
  toolsSearchInput: document.getElementById("tools-search-input"),
  refreshToolsBtn: document.getElementById("refresh-tools-btn"),
  toolsTableWrap: document.getElementById("tools-table-wrap"),
  toolsTbody: document.getElementById("tools-tbody"),
  toolsEmpty: document.getElementById("tools-empty"),

  // Theme Management
  themesCountBadge: document.getElementById("themes-count-badge"),
  themeUploadPickBtn: document.getElementById("theme-upload-pick-btn"),
  refreshThemesBtn: document.getElementById("refresh-themes-btn"),
  themeUploadFileInput: document.getElementById("theme-upload-file-input"),
  themeUploadDropzone: document.getElementById("theme-upload-dropzone"),
  themeUploadStatus: document.getElementById("theme-upload-status"),
  themesGrid: document.getElementById("themes-grid"),
  themesEmpty: document.getElementById("themes-empty"),

  // Settings
  settingsForm: document.getElementById("settings-form"),
  settingSiteTitle: document.getElementById("setting-site-title"),
  settingSiteDesc: document.getElementById("setting-site-desc"),
  saveSettingsBtn: document.getElementById("save-settings-btn"),
  settingsFeedback: document.getElementById("settings-feedback"),
  currentVersion: document.getElementById("current-version"),
  checkUpdatesBtn: document.getElementById("check-updates-btn"),
  autoCheckUpdates: document.getElementById("auto-check-updates"),
  updateStatus: document.getElementById("update-status"),
  updateReleaseLink: document.getElementById("update-release-link"),
  updateAvailableBadge: document.getElementById("update-available-badge"),

  // API Reference
  copyCurlBtn: document.getElementById("copy-curl-btn"),
  curlExample: document.getElementById("curl-example"),

  // Confirm Dialog
  confirmDialog: document.getElementById("confirm-dialog"),
  confirmTitle: document.getElementById("confirm-title"),
  confirmMessage: document.getElementById("confirm-message"),
  confirmCancelBtn: document.getElementById("confirm-cancel-btn"),
  confirmOkBtn: document.getElementById("confirm-ok-btn"),
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
    // Ignore storage errors
  }
}

function normalizeHexColor(value) {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (/^#[0-9a-fA-F]{6}$/.test(trimmed)) {
    return trimmed.toLowerCase();
  }
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

function isSafeThemePreviewUrl(url) {
  return typeof url === "string" && url.startsWith("/api/admin/themes/") && !url.startsWith("//");
}

function isSafeExternalUrl(url) {
  if (typeof url !== "string") return false;
  const trimmed = url.trim();
  if (!/^https?:\/\//i.test(trimmed)) return false;
  try {
    const parsed = new URL(trimmed);
    return parsed.protocol === "https:" || parsed.protocol === "http:";
  } catch {
    return false;
  }
}

function initTheme() {
  els.themePalette.addEventListener("load", applyPalette);
  const saved = readStorage(STORAGE_KEY_MODE);
  if (saved === "dark" || saved === "light") {
    state.themeMode = saved;
  } else if (window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches) {
    state.themeMode = "dark";
  }
  applyTheme();
}

function applyTheme() {
  document.documentElement.dataset.theme = state.themeMode;
  applyPalette();
  const label = state.themeMode === "dark" ? "亮色" : "暗色";
  els.loginThemeToggle.textContent = label;
  els.dashThemeToggle.textContent = label;
}

function applyPalette() {
  const sample = els.themePalette.contentDocument;
  if (!sample?.styleSheets.length) return;

  // Resolve colors in the script-free sample without applying theme selectors to admin controls.
  sample.documentElement.dataset.theme = state.themeMode;
  const computed = els.themePalette.contentWindow.getComputedStyle(sample.documentElement);
  const style = document.documentElement.style;
  for (const token of ADMIN_COLOR_TOKENS) {
    const value = computed.getPropertyValue(token).trim();
    if (CSS.supports("color", value)) style.setProperty(token, value);
    else style.removeProperty(token);
  }
  style.colorScheme = ["light", "dark"].includes(computed.colorScheme) ? computed.colorScheme : "";
}

function reloadThemePalette() {
  if (!els.themePalette) return;
  els.themePalette.srcdoc = `<!doctype html><link rel="stylesheet" href="/ui/v1/toolstow.css?_ts=${Date.now()}">`;
}

function toggleTheme() {
  state.themeMode = state.themeMode === "dark" ? "light" : "dark";
  writeStorage(STORAGE_KEY_MODE, state.themeMode);
  applyTheme();
}

function showToast(message, kind = "info", durationMs = 4000) {
  if (state.toastTimer) {
    window.clearTimeout(state.toastTimer);
    state.toastTimer = null;
  }
  els.globalToast.textContent = message;
  els.globalToast.dataset.kind = kind;
  els.globalToast.hidden = false;

  if (durationMs > 0) {
    state.toastTimer = window.setTimeout(() => {
      els.globalToast.hidden = true;
    }, durationMs);
  }
}

function setLoginError(message) {
  if (!message) {
    els.loginError.textContent = "";
    els.loginError.hidden = true;
    return;
  }
  els.loginError.textContent = message;
  els.loginError.hidden = false;
}

function setUploadStatus(message, kind = "info") {
  if (!message) {
    els.uploadStatus.textContent = "";
    els.uploadStatus.hidden = true;
    return;
  }
  els.uploadStatus.textContent = message;
  els.uploadStatus.dataset.kind = kind;
  els.uploadStatus.hidden = false;
}

function setThemeUploadStatus(message, kind = "info") {
  if (!message) {
    els.themeUploadStatus.textContent = "";
    els.themeUploadStatus.hidden = true;
    return;
  }
  els.themeUploadStatus.textContent = message;
  els.themeUploadStatus.dataset.kind = kind;
  els.themeUploadStatus.hidden = false;
}

function setSettingsFeedback(message, kind = "info") {
  els.settingsFeedback.textContent = message || "";
  els.settingsFeedback.dataset.kind = kind;
}

function parseErrorPayload(body, fallback) {
  if (body && typeof body === "object") {
    const msg = body.error || body.message;
    if (msg && body.code) return `[${body.code}] ${msg}`;
    if (msg) return String(msg);
  }
  return fallback;
}

async function readJsonSafe(res) {
  try {
    return await res.json();
  } catch {
    return null;
  }
}

async function apiRequest(url, options = {}) {
  const headers = Object.assign({}, CLIENT_HEADERS, options.headers || {});
  return fetch(url, {
    ...options,
    headers,
    credentials: "same-origin",
    cache: "no-store",
  });
}

function readRecentUpdateResult(now) {
  try {
    const result = JSON.parse(readStorage(STORAGE_KEY_UPDATE_RESULT));
    if (result && Number.isFinite(result.at) && result.at <= now && now - result.at < UPDATE_CHECK_INTERVAL) {
      if (result.tag === null || (typeof result.tag === "string" && STABLE_RELEASE_TAG.test(result.tag))) {
        return result;
      }
    }
  } catch {
    // Ignore malformed cache entries.
  }
  return null;
}

function isNewerRelease(latest, running) {
  for (let i = 1; i <= 3; i += 1) {
    const releasePart = BigInt(latest[i]);
    const runningPart = BigInt(running[i]);
    if (releasePart !== runningPart) return releasePart > runningPart;
  }
  return Boolean(running[4]);
}

function renderUpdateStatus(message, { kind = "info", tag = null } = {}) {
  els.updateStatus.textContent = message;
  els.updateStatus.dataset.kind = kind;
  els.updateAvailableBadge.hidden = !tag;
  els.updateReleaseLink.hidden = !tag;
  if (tag) {
    els.updateReleaseLink.href = RELEASES_URL + tag;
  } else {
    els.updateReleaseLink.removeAttribute("href");
  }
}

function renderUpdateResult(tag, running) {
  if (!tag) {
    renderUpdateStatus("上次检查更新未成功，请手动重试。", { kind: "error" });
  } else if (isNewerRelease(tag.match(STABLE_RELEASE_TAG), running)) {
    renderUpdateStatus(`发现新版本 ${tag}。`, { tag });
  } else {
    renderUpdateStatus("未发现更高的正式版本。", { kind: "success" });
  }
}

async function checkForUpdates({ manual = false } = {}) {
  if (!state.authenticated || state.updateCheck) return;

  const controller = new AbortController();
  const { signal } = controller;
  state.updateCheck = controller;
  els.checkUpdatesBtn.disabled = true;
  renderUpdateStatus("正在读取当前版本…");
  const timer = window.setTimeout(() => controller.abort(), 8000);
  let running = null;
  let fetchStarted = false;
  let checkAt = 0;

  try {
    const health = await apiRequest("/healthz", { signal });
    const version = health.ok ? (await readJsonSafe(health))?.version : null;
    running = typeof version === "string" ? version.match(RUNNING_VERSION) : null;
    if (state.updateCheck !== controller) return;
    if (!running) throw new Error("Invalid running version");
    els.currentVersion.textContent = `v${version}`;

    const now = Date.now();
    const cached = manual ? null : readRecentUpdateResult(now);
    if (cached) {
      renderUpdateResult(cached.tag, running);
      return;
    }
    if (!manual && !els.autoCheckUpdates.checked) {
      renderUpdateStatus("尚未检查更新。");
      return;
    }

    renderUpdateStatus("正在检查更新…");
    checkAt = now;
    fetchStarted = true;
    const release = await fetch(LATEST_RELEASE_API, { credentials: "omit", cache: "no-store", signal });
    const tag = release.ok ? (await readJsonSafe(release))?.tag_name : null;
    if (state.updateCheck !== controller) return;
    if (typeof tag !== "string" || !STABLE_RELEASE_TAG.test(tag)) throw new Error("Invalid release tag");
    writeStorage(STORAGE_KEY_UPDATE_RESULT, JSON.stringify({ at: now, tag }));
    renderUpdateResult(tag, running);
  } catch {
    if (state.updateCheck !== controller) return;
    if (fetchStarted) {
      writeStorage(STORAGE_KEY_UPDATE_RESULT, JSON.stringify({ at: checkAt || Date.now(), tag: null }));
    }
    if (!running) els.currentVersion.textContent = "不可用";
    const message = running ? "无法检查更新，请稍后重试。" : "无法读取当前版本，请稍后重试。";
    renderUpdateStatus(message, { kind: "error" });
  } finally {
    window.clearTimeout(timer);
    if (state.updateCheck === controller) {
      state.updateCheck = null;
      els.checkUpdatesBtn.disabled = false;
    }
  }
}

function showLoginView(reasonMessage = "") {
  state.authenticated = false;
  state.updateCheck?.abort();
  state.updateCheck = null;
  els.checkUpdatesBtn.disabled = false;
  els.updateAvailableBadge.hidden = true;
  els.updateReleaseLink.hidden = true;
  els.updateReleaseLink.removeAttribute("href");
  els.bootView.hidden = true;
  els.dashboardView.hidden = true;
  els.loginView.hidden = false;

  const disabled = state.adminEnabled === false;
  els.loginDisabledNotice.hidden = !disabled;
  els.adminKeyInput.disabled = disabled;
  els.loginSubmitBtn.disabled = disabled;
  els.toggleKeyVisibility.disabled = disabled;

  setLoginError(reasonMessage);
  if (!disabled) {
    els.adminKeyInput.focus();
  }
}

function showDashboardView() {
  state.authenticated = true;
  els.bootView.hidden = true;
  els.loginView.hidden = true;
  els.dashboardView.hidden = false;
  setLoginError("");
  els.autoCheckUpdates.checked = readStorage(STORAGE_KEY_AUTO_UPDATE_CHECK) === "true";
  void checkForUpdates();
}

function handleUnauthorized() {
  closeHtmlDialog();
  state.tools = [];
  state.themes = [];
  setThemeUploadStatus("");
  showLoginView("管理员会话已过期，请重新输入 Admin Key 登录。");
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

function getFilteredTools() {
  const q = state.search.trim().toLowerCase();
  return state.tools.filter((tool) => {
    const isDraft = tool.draft === true;
    if (state.filter === "published" && isDraft) return false;
    if (state.filter === "draft" && !isDraft) return false;

    if (!q) return true;
    const tags = Array.isArray(tool.tags) ? tool.tags.join(" ") : "";
    const haystack = [tool.name, tool.id, tool.description, tool.category, tool.version, tags]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return haystack.includes(q);
  });
}

function renderStatsAndTabs() {
  const total = state.tools.length;
  let draftCount = 0;
  for (const tool of state.tools) {
    if (tool.draft === true) draftCount += 1;
  }
  const publishedCount = total - draftCount;

  els.statTotal.textContent = String(total);
  els.statPublished.textContent = String(publishedCount);
  els.statDraft.textContent = String(draftCount);

  els.tabCountAll.textContent = String(total);
  els.tabCountPublished.textContent = String(publishedCount);
  els.tabCountDraft.textContent = String(draftCount);

  for (const tab of els.filterTabs) {
    const active = tab.dataset.filter === state.filter;
    tab.classList.toggle("active", active);
    tab.setAttribute("aria-selected", active ? "true" : "false");
  }
}

function renderToolsTable() {
  renderStatsAndTabs();
  const visibleTools = getFilteredTools();

  if (visibleTools.length === 0) {
    els.toolsTableWrap.hidden = true;
    els.toolsEmpty.hidden = false;
    els.toolsEmpty.textContent =
      state.tools.length === 0
        ? "尚未安装任何工具。可在上方收录 .zip 工具包或单文件 HTML。"
        : "当前筛选条件下没有匹配的工具。";
    els.toolsTbody.replaceChildren();
    return;
  }

  els.toolsTableWrap.hidden = false;
  els.toolsEmpty.hidden = true;

  const fragment = document.createDocumentFragment();

  for (const tool of visibleTools) {
    const isDraft = tool.draft === true;
    const tr = document.createElement("tr");

    // 1. Tool cell (icon + name + desc)
    const tdTool = document.createElement("td");
    tdTool.dataset.label = "";
    const toolCell = document.createElement("div");
    toolCell.className = "tool-cell";

    const iconWrap = document.createElement("span");
    iconWrap.className = "tool-icon";
    renderIconInto(iconWrap, tool);

    const textWrap = document.createElement("div");
    textWrap.className = "tool-cell-text";

    const nameEl = document.createElement("span");
    nameEl.className = "tool-cell-name";
    nameEl.textContent = tool.name || tool.id;

    textWrap.appendChild(nameEl);

    if (tool.description) {
      const descEl = document.createElement("span");
      descEl.className = "tool-cell-desc";
      descEl.title = tool.description;
      descEl.textContent = tool.description;
      textWrap.appendChild(descEl);
    }

    toolCell.append(iconWrap, textWrap);
    tdTool.appendChild(toolCell);

    // 2. ID
    const tdId = document.createElement("td");
    tdId.dataset.label = "ID";
    const idCode = document.createElement("code");
    idCode.className = "mono-text";
    idCode.textContent = tool.id;
    tdId.appendChild(idCode);

    // 3. Version
    const tdVer = document.createElement("td");
    tdVer.dataset.label = "版本";
    const verSpan = document.createElement("span");
    verSpan.className = "mono-text";
    verSpan.textContent = tool.version ? `v${tool.version}` : "—";
    tdVer.appendChild(verSpan);

    // 4. Category
    const tdCat = document.createElement("td");
    tdCat.dataset.label = "分类";
    tdCat.textContent = tool.category || "未分类";

    // 5. Status pill
    const tdStatus = document.createElement("td");
    tdStatus.dataset.label = "状态";
    const pill = document.createElement("span");
    pill.className = isDraft ? "status-pill status-pill-draft" : "status-pill status-pill-published";
    const dot = document.createElement("span");
    dot.className = "status-dot";
    dot.setAttribute("aria-hidden", "true");
    const statusText = document.createElement("span");
    statusText.textContent = isDraft ? "草稿" : "已发布";
    pill.append(dot, statusText);
    tdStatus.appendChild(pill);

    // 6. Actions
    const tdActions = document.createElement("td");
    tdActions.dataset.label = "";
    const actionsWrap = document.createElement("div");
    actionsWrap.className = "row-actions";

    const toggleDraftBtn = document.createElement("button");
    toggleDraftBtn.type = "button";
    toggleDraftBtn.className = isDraft ? "btn btn-primary btn-xs" : "btn btn-xs";
    toggleDraftBtn.textContent = isDraft ? "发布上线" : "设为草稿";
    toggleDraftBtn.addEventListener("click", () => {
      toggleToolDraft(tool, toggleDraftBtn);
    });

    actionsWrap.appendChild(toggleDraftBtn);

    if (isSafeToolUrl(tool.entryUrl)) {
      const previewLink = document.createElement("a");
      previewLink.className = "btn btn-xs";
      previewLink.href = tool.entryUrl;
      previewLink.target = "_blank";
      previewLink.rel = "noopener noreferrer";
      previewLink.textContent = "预览";
      actionsWrap.appendChild(previewLink);
    }

    const deleteBtn = document.createElement("button");
    deleteBtn.type = "button";
    deleteBtn.className = "btn btn-danger btn-xs";
    deleteBtn.textContent = "删除";
    deleteBtn.addEventListener("click", () => {
      requestDeleteTool(tool);
    });

    actionsWrap.appendChild(deleteBtn);
    tdActions.appendChild(actionsWrap);

    tr.append(tdTool, tdId, tdVer, tdCat, tdStatus, tdActions);
    fragment.appendChild(tr);
  }

  els.toolsTbody.replaceChildren(fragment);
}

async function fetchAdminTools() {
  try {
    const res = await apiRequest("/api/admin/tools", { method: "GET" });
    if (res.status === 401) {
      handleUnauthorized();
      return;
    }
    if (!res.ok) {
      const body = await readJsonSafe(res);
      throw new Error(parseErrorPayload(body, `加载工具列表失败 (HTTP ${res.status})`));
    }
    const data = await res.json();
    state.tools = Array.isArray(data) ? data : [];
    renderToolsTable();
  } catch (err) {
    showToast(err.message || "加载工具列表失败", "error", 5000);
  }
}

async function fetchAdminSettings() {
  try {
    const res = await apiRequest("/api/admin/settings", { method: "GET" });
    if (res.status === 401) {
      handleUnauthorized();
      return;
    }
    if (!res.ok) {
      const body = await readJsonSafe(res);
      throw new Error(parseErrorPayload(body, `加载站点设置失败 (HTTP ${res.status})`));
    }
    const data = await res.json();
    if (!data || typeof data !== "object") return;

    els.settingSiteTitle.value = typeof data.siteTitle === "string" ? data.siteTitle : "ToolStow";
    els.settingSiteDesc.value =
      typeof data.siteDescription === "string" ? data.siteDescription : "";
  } catch (err) {
    setSettingsFeedback(err.message || "加载站点设置失败", "error");
  }
}

async function saveAdminSettings(event) {
  event.preventDefault();

  const siteTitle = els.settingSiteTitle.value.trim();
  const siteDescription = els.settingSiteDesc.value.trim();

  if (!siteTitle) {
    setSettingsFeedback("站点标题不能为空", "error");
    els.settingSiteTitle.focus();
    return;
  }

  els.saveSettingsBtn.disabled = true;
  setSettingsFeedback("正在保存...", "info");

  try {
    const res = await apiRequest("/api/admin/settings", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        siteTitle,
        siteDescription,
      }),
    });

    if (res.status === 401) {
      handleUnauthorized();
      return;
    }

    const body = await readJsonSafe(res);
    if (!res.ok) {
      throw new Error(parseErrorPayload(body, `保存失败 (HTTP ${res.status})`));
    }

    if (body && typeof body === "object") {
      els.settingSiteTitle.value = body.siteTitle || siteTitle;
      els.settingSiteDesc.value =
        typeof body.siteDescription === "string" ? body.siteDescription : siteDescription;
    }

    const now = new Date();
    const hh = String(now.getHours()).padStart(2, "0");
    const mm = String(now.getMinutes()).padStart(2, "0");
    const ss = String(now.getSeconds()).padStart(2, "0");
    setSettingsFeedback(`已保存 (${hh}:${mm}:${ss})`, "success");
    showToast("站点设置已更新", "info");
  } catch (err) {
    setSettingsFeedback(err.message || "保存站点设置失败", "error");
  } finally {
    els.saveSettingsBtn.disabled = false;
  }
}

function openConfirmDialog({ title, message, confirmLabel, confirmKind = "danger", onConfirm, onCancel }) {
  if (typeof els.confirmDialog.showModal === "function") {
    state.pendingConfirmAction = { onConfirm, onCancel };
    els.confirmTitle.textContent = title;
    els.confirmMessage.textContent = message;
    els.confirmOkBtn.textContent = confirmLabel;
    els.confirmOkBtn.className =
      confirmKind === "primary" ? "btn btn-primary btn-sm" : "btn btn-danger btn-sm";
    els.confirmDialog.showModal();
  } else {
    const confirmed = window.confirm(`${title}\n\n${message}`);
    if (confirmed) {
      if (typeof onConfirm === "function") onConfirm();
    } else if (typeof onCancel === "function") {
      onCancel();
    }
  }
}

function formatOverwritePrompt(body, fileName) {
  const existing = body && body.existing && typeof body.existing === "object" ? body.existing : null;
  const incoming = body && body.incoming && typeof body.incoming === "object" ? body.incoming : null;
  const toolId = (incoming && incoming.id) || (existing && existing.id) || fileName;
  const existingName = (existing && existing.name) || toolId;
  const existingVer = existing && existing.version ? `v${existing.version}` : "未知版本";
  const existingStatus = existing ? (existing.draft ? "草稿" : "已发布") : "已安装";

  if (incoming) {
    const incomingName = incoming.name || toolId;
    const incomingVer = incoming.version ? `v${incoming.version}` : "未知版本";
    const incomingStatus = incoming.draft ? "草稿" : "已发布";
    return `工具「${existingName}」(${toolId}) 已存在（当前 ${existingVer} · ${existingStatus}）。确定要用新上传的「${incomingName}」(${incomingVer} · ${incomingStatus}) 覆盖替换现有目录吗？`;
  }

  return `工具 ID「${toolId}」已存在（当前 ${existingVer} · ${existingStatus}）。确定要覆盖替换现有工具目录吗？`;
}

async function uploadZipFile(file, overwrite = false) {
  if (!file) return;
  if (!file.name.toLowerCase().endsWith(".zip")) {
    setUploadStatus("仅支持上传 .zip 格式的工具压缩包。", "error");
    return;
  }

  setUploadStatus(
    overwrite ? `正在覆盖更新 ${file.name}...` : `正在上传校验 ${file.name}...`,
    "info"
  );
  els.uploadPickBtn.disabled = true;

  const formData = new FormData();
  formData.append("file", file, file.name);

  try {
    const endpoint = overwrite
      ? "/api/admin/tools/upload?overwrite=true"
      : "/api/admin/tools/upload?overwrite=false";
    const res = await apiRequest(endpoint, {
      method: "POST",
      body: formData,
    });

    if (res.status === 401) {
      handleUnauthorized();
      return;
    }

    const body = await readJsonSafe(res);

    if (res.status === 409 && !overwrite) {
      const promptMsg = formatOverwritePrompt(body, file.name);
      const conflictId =
        (body && body.incoming && body.incoming.id) ||
        (body && body.existing && body.existing.id) ||
        file.name;
      setUploadStatus(`检测到同名工具「${conflictId}」已存在，等待确认是否覆盖...`, "info");
      openConfirmDialog({
        title: "工具已存在，确认覆盖？",
        message: promptMsg,
        confirmLabel: "确认覆盖",
        confirmKind: "primary",
        onConfirm: () => {
          uploadZipFile(file, true);
        },
        onCancel: () => {
          setUploadStatus(`已取消覆盖：${conflictId}`, "info");
        },
      });
      return;
    }

    if (!res.ok) {
      throw new Error(parseErrorPayload(body, `上传失败 (HTTP ${res.status})`));
    }

    const tool = body && body.id ? body : body && body.entry ? body.entry : null;
    const actionLabel = res.status === 201 ? "已新增安装" : "已覆盖更新";
    const draftSuffix = tool && tool.draft ? "（草稿状态）" : "";
    const detail = tool ? `${tool.name} (${tool.id}) v${tool.version}${draftSuffix}` : file.name;
    const msg = `${actionLabel}：${detail}`;

    setUploadStatus(msg, "success");
    showToast(msg, "info");
    await fetchAdminTools();
  } catch (err) {
    setUploadStatus(err.message || "上传失败", "error");
  } finally {
    els.uploadPickBtn.disabled = false;
    els.uploadFileInput.value = "";
  }
}

async function toggleToolDraft(tool, buttonEl) {
  const nextDraft = !(tool.draft === true);
  buttonEl.disabled = true;

  try {
    const res = await apiRequest(`/api/admin/tools/${encodeURIComponent(tool.id)}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ draft: nextDraft }),
    });

    if (res.status === 401) {
      handleUnauthorized();
      return;
    }

    const body = await readJsonSafe(res);
    if (!res.ok) {
      throw new Error(parseErrorPayload(body, `更新状态失败 (HTTP ${res.status})`));
    }

    showToast(
      nextDraft ? `已设为草稿：${tool.name} (${tool.id})` : `已发布上线：${tool.name} (${tool.id})`,
      "info"
    );
    await fetchAdminTools();
  } catch (err) {
    showToast(err.message || "更新状态失败", "error", 5000);
    buttonEl.disabled = false;
  }
}

function requestDeleteTool(tool) {
  openConfirmDialog({
    title: "确认删除工具",
    message: `确定要删除工具「${tool.name}」(${tool.id}) 吗？该操作将移除磁盘上的工具目录且无法撤销。`,
    confirmLabel: "确认删除",
    confirmKind: "danger",
    onConfirm: () => {
      executeDeleteTool(tool);
    },
  });
}

async function executeDeleteTool(tool) {
  try {
    const res = await apiRequest(`/api/admin/tools/${encodeURIComponent(tool.id)}`, {
      method: "DELETE",
    });

    if (res.status === 401) {
      handleUnauthorized();
      return;
    }

    if (!res.ok && res.status !== 204) {
      const body = await readJsonSafe(res);
      throw new Error(parseErrorPayload(body, `删除失败 (HTTP ${res.status})`));
    }

    showToast(`已删除工具：${tool.name} (${tool.id})`, "info");
    await fetchAdminTools();
  } catch (err) {
    showToast(err.message || "删除失败", "error", 5000);
  }
}

function getThemeModeLabel(theme) {
  if (theme.builtin === true) return "内置默认";
  if (theme.hasCustomShell && theme.hasUiCss) return "完整主题 + 皮肤";
  if (theme.hasCustomShell) return "完整主题";
  if (theme.hasUiCss) return "纯皮肤覆盖";
  return "自定义主题";
}

function getThemePreviewTag(theme) {
  if (theme.builtin === true) return "BUILTIN";
  if (theme.hasCustomShell && theme.hasUiCss) return "SHELL + UI.CSS";
  if (theme.hasCustomShell) return "CUSTOM SHELL";
  if (theme.hasUiCss) return "UI.CSS SKIN";
  return "THEME";
}

function renderThemeFallbackPreview(previewBox, theme) {
  const wrap = document.createElement("div");
  wrap.className = "theme-preview-fallback";

  const shortBadge = document.createElement("span");
  shortBadge.className = "theme-preview-short";
  shortBadge.textContent = theme.short || "theme";

  const tag = document.createElement("span");
  tag.className = "theme-preview-tag";
  tag.textContent = getThemePreviewTag(theme);

  wrap.append(shortBadge, tag);
  previewBox.replaceChildren(wrap);
}

function renderThemePreview(previewBox, theme) {
  if (isSafeThemePreviewUrl(theme.previewUrl)) {
    const img = document.createElement("img");
    img.src = theme.previewUrl;
    img.alt = `${theme.name || theme.short || "主题"} 预览`;
    img.loading = "lazy";
    img.addEventListener("error", () => {
      renderThemeFallbackPreview(previewBox, theme);
    });
    previewBox.replaceChildren(img);
    return;
  }
  renderThemeFallbackPreview(previewBox, theme);
}

function renderThemesGrid() {
  const count = state.themes.length;
  const activeTheme = state.themes.find((t) => t && t.active === true);
  els.themesCountBadge.textContent = activeTheme
    ? `${count} 个主题 · 当前 ${activeTheme.short}`
    : `${count} 个主题`;

  if (count === 0) {
    els.themesGrid.hidden = true;
    els.themesEmpty.hidden = false;
    els.themesGrid.replaceChildren();
    return;
  }

  els.themesGrid.hidden = false;
  els.themesEmpty.hidden = true;

  const fragment = document.createDocumentFragment();

  for (const theme of state.themes) {
    if (!theme || typeof theme !== "object") continue;

    const isActive = theme.active === true;
    const isBuiltin = theme.builtin === true;
    const displayName = theme.name || theme.short || "未命名主题";

    const card = document.createElement("article");
    card.className = isActive ? "theme-card theme-card-active" : "theme-card";

    // Top 16:9 monochrome preview box
    const previewBox = document.createElement("div");
    previewBox.className = "theme-preview";
    renderThemePreview(previewBox, theme);

    // Card body
    const body = document.createElement("div");
    body.className = "theme-card-body";

    const head = document.createElement("div");
    head.className = "theme-card-head";

    const titleRow = document.createElement("div");
    titleRow.className = "theme-card-title-row";

    const titleEl = document.createElement("h3");
    titleEl.className = "theme-card-title";
    titleEl.textContent = displayName;
    titleRow.appendChild(titleEl);

    if (isActive) {
      const activePill = document.createElement("span");
      activePill.className = "theme-active-pill";
      const dot = document.createElement("span");
      dot.className = "theme-active-dot";
      dot.setAttribute("aria-hidden", "true");
      const activeText = document.createElement("span");
      activeText.textContent = "当前使用";
      activePill.append(dot, activeText);
      titleRow.appendChild(activePill);
    }

    const badgesRow = document.createElement("div");
    badgesRow.className = "theme-badges";
    const modeBadge = document.createElement("span");
    modeBadge.className = "mono-badge";
    modeBadge.textContent = getThemeModeLabel(theme);
    badgesRow.appendChild(modeBadge);

    head.append(titleRow, badgesRow);
    body.appendChild(head);

    // Metadata line: short, version, author, safe external link
    const metaRow = document.createElement("div");
    metaRow.className = "theme-meta";

    const shortItem = document.createElement("span");
    shortItem.className = "theme-meta-item";
    const shortCode = document.createElement("code");
    shortCode.className = "mono-text";
    shortCode.textContent = theme.short || "";
    shortItem.appendChild(shortCode);
    metaRow.appendChild(shortItem);

    if (theme.version) {
      const verItem = document.createElement("span");
      verItem.className = "theme-meta-item mono-text";
      verItem.textContent = `v${theme.version}`;
      metaRow.appendChild(verItem);
    }

    if (theme.author) {
      const authorItem = document.createElement("span");
      authorItem.className = "theme-meta-item";
      authorItem.textContent = theme.author;
      metaRow.appendChild(authorItem);
    }

    if (isSafeExternalUrl(theme.url)) {
      const extLink = document.createElement("a");
      extLink.className = "theme-meta-link";
      extLink.href = theme.url.trim();
      extLink.target = "_blank";
      extLink.rel = "noopener noreferrer";
      extLink.textContent = "主页 ↗";
      metaRow.appendChild(extLink);
    }

    body.appendChild(metaRow);

    if (theme.description) {
      const descEl = document.createElement("p");
      descEl.className = "theme-card-desc";
      descEl.title = theme.description;
      descEl.textContent = theme.description;
      body.appendChild(descEl);
    }

    // Actions row
    const actionsRow = document.createElement("div");
    actionsRow.className = "theme-card-actions";

    if (isActive) {
      const enabledBtn = document.createElement("button");
      enabledBtn.type = "button";
      enabledBtn.className = "btn btn-xs";
      enabledBtn.disabled = true;
      enabledBtn.setAttribute("aria-disabled", "true");
      enabledBtn.textContent = "已启用";
      actionsRow.appendChild(enabledBtn);
    } else {
      const activateBtn = document.createElement("button");
      activateBtn.type = "button";
      activateBtn.className = "btn btn-primary btn-xs";
      activateBtn.textContent = "启用主题";
      activateBtn.addEventListener("click", () => {
        activateTheme(theme, activateBtn);
      });
      actionsRow.appendChild(activateBtn);
    }

    if (!isBuiltin) {
      const deleteBtn = document.createElement("button");
      deleteBtn.type = "button";
      deleteBtn.className = "btn btn-danger btn-xs";
      deleteBtn.textContent = "删除";
      deleteBtn.addEventListener("click", () => {
        requestDeleteTheme(theme);
      });
      actionsRow.appendChild(deleteBtn);
    }

    body.appendChild(actionsRow);
    card.append(previewBox, body);
    fragment.appendChild(card);
  }

  els.themesGrid.replaceChildren(fragment);
}

async function fetchAdminThemes() {
  try {
    const res = await apiRequest("/api/admin/themes", { method: "GET" });
    if (res.status === 401) {
      handleUnauthorized();
      return;
    }
    if (!res.ok) {
      const body = await readJsonSafe(res);
      throw new Error(parseErrorPayload(body, `加载主题列表失败 (HTTP ${res.status})`));
    }
    const data = await res.json();
    state.themes = Array.isArray(data) ? data : [];
    renderThemesGrid();
  } catch (err) {
    showToast(err.message || "加载主题列表失败", "error", 5000);
  }
}

function formatThemeOverwritePrompt(body, fileName) {
  const existing = body && body.existing && typeof body.existing === "object" ? body.existing : null;
  const incoming = body && body.incoming && typeof body.incoming === "object" ? body.incoming : null;
  const short = (incoming && incoming.short) || (existing && existing.short) || fileName;
  const existingName = (existing && existing.name) || short;
  const existingVer = existing && existing.version ? `v${existing.version}` : "未知版本";

  if (incoming) {
    const incomingName = incoming.name || short;
    const incomingVer = incoming.version ? `v${incoming.version}` : "未知版本";
    return `主题「${existingName}」(${short}) 已存在（当前 ${existingVer}）。确定要用新上传的「${incomingName}」(${incomingVer}) 覆盖替换现有主题吗？`;
  }

  return `主题「${existingName}」(${short}) 已存在（当前 ${existingVer}）。确定要覆盖替换现有主题目录吗？`;
}

async function uploadThemeZipFile(file, overwrite = false) {
  if (!file) return;
  if (!file.name.toLowerCase().endsWith(".zip")) {
    setThemeUploadStatus("仅支持上传 .zip 格式的主题压缩包。", "error");
    return;
  }

  setThemeUploadStatus(
    overwrite ? `正在覆盖更新主题 ${file.name}...` : `正在上传校验主题 ${file.name}...`,
    "info"
  );
  els.themeUploadPickBtn.disabled = true;

  const formData = new FormData();
  formData.append("file", file, file.name);

  try {
    const endpoint = overwrite
      ? "/api/admin/themes/upload?overwrite=true"
      : "/api/admin/themes/upload?overwrite=false";
    const res = await apiRequest(endpoint, {
      method: "POST",
      body: formData,
    });

    if (res.status === 401) {
      handleUnauthorized();
      return;
    }

    const body = await readJsonSafe(res);

    if (res.status === 409 && !overwrite && (!body || !body.code || body.code === "THEME_EXISTS")) {
      const promptMsg = formatThemeOverwritePrompt(body, file.name);
      const conflictShort =
        (body && body.incoming && body.incoming.short) ||
        (body && body.existing && body.existing.short) ||
        file.name;
      setThemeUploadStatus(`检测到同名主题「${conflictShort}」已存在，等待确认是否覆盖...`, "info");
      openConfirmDialog({
        title: "主题已存在，确认覆盖？",
        message: promptMsg,
        confirmLabel: "确认覆盖",
        confirmKind: "primary",
        onConfirm: () => {
          uploadThemeZipFile(file, true);
        },
        onCancel: () => {
          setThemeUploadStatus(`已取消覆盖主题：${conflictShort}`, "info");
        },
      });
      return;
    }

    if (!res.ok) {
      throw new Error(parseErrorPayload(body, `主题上传失败 (HTTP ${res.status})`));
    }

    const theme =
      body && body.short
        ? body
        : body && body.theme
          ? body.theme
          : body && body.entry
            ? body.entry
            : null;
    const actionLabel = res.status === 201 ? "已安装主题" : "已覆盖更新主题";
    const verSuffix = theme && theme.version ? ` v${theme.version}` : "";
    const detail = theme ? `${theme.name || theme.short} (${theme.short})${verSuffix}` : file.name;
    const msg = `${actionLabel}：${detail}`;

    setThemeUploadStatus(msg, "success");
    showToast(msg, "info");
    await fetchAdminThemes();
    if (res.status === 200 && theme && state.themes.some((t) => t.short === theme.short && t.active)) {
      reloadThemePalette();
    }
  } catch (err) {
    setThemeUploadStatus(err.message || "主题上传失败", "error");
  } finally {
    els.themeUploadPickBtn.disabled = false;
    els.themeUploadFileInput.value = "";
  }
}

async function activateTheme(theme, buttonEl) {
  buttonEl.disabled = true;
  const originalLabel = buttonEl.textContent;
  buttonEl.textContent = "切换中...";

  try {
    const res = await apiRequest("/api/admin/themes/active", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ short: theme.short }),
    });

    if (res.status === 401) {
      handleUnauthorized();
      return;
    }

    const body = await readJsonSafe(res);
    if (!res.ok) {
      throw new Error(parseErrorPayload(body, `切换主题失败 (HTTP ${res.status})`));
    }

    showToast(`已切换主题：${theme.name || theme.short}`, "info");
    await fetchAdminThemes();
    reloadThemePalette();
  } catch (err) {
    showToast(err.message || "切换主题失败", "error", 5000);
    buttonEl.disabled = false;
    buttonEl.textContent = originalLabel;
  }
}

function requestDeleteTheme(theme) {
  const displayName = theme.name || theme.short;
  const message = theme.active
    ? `确定要删除当前正在使用的主题「${displayName}」(${theme.short}) 吗？删除后公开站点将自动切回内置默认主题 (default)，且该操作无法撤销。`
    : `确定要删除主题「${displayName}」(${theme.short}) 吗？该操作将移除磁盘上的主题目录且无法撤销。`;

  openConfirmDialog({
    title: "确认删除主题",
    message,
    confirmLabel: "确认删除",
    confirmKind: "danger",
    onConfirm: () => {
      executeDeleteTheme(theme);
    },
  });
}

async function executeDeleteTheme(theme) {
  try {
    const res = await apiRequest(`/api/admin/themes/${encodeURIComponent(theme.short)}`, {
      method: "DELETE",
    });

    if (res.status === 401) {
      handleUnauthorized();
      return;
    }

    if (!res.ok && res.status !== 204) {
      const body = await readJsonSafe(res);
      throw new Error(parseErrorPayload(body, `删除主题失败 (HTTP ${res.status})`));
    }

    const displayName = theme.name || theme.short;
    const msg = theme.active
      ? `已删除主题：${displayName} (${theme.short})，已自动切回默认主题`
      : `已删除主题：${displayName} (${theme.short})`;
    showToast(msg, "info");
    await fetchAdminThemes();
    if (theme.active) {
      reloadThemePalette();
    }
  } catch (err) {
    showToast(err.message || "删除主题失败", "error", 5000);
  }
}

async function handleLoginSubmit(event) {
  event.preventDefault();
  setLoginError("");

  const key = els.adminKeyInput.value;
  if (!key) {
    setLoginError("请输入 Admin Key。");
    els.adminKeyInput.focus();
    return;
  }

  els.loginSubmitBtn.disabled = true;
  els.loginSubmitBtn.textContent = "验证中...";

  try {
    const res = await apiRequest("/api/admin/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ key }),
    });

    const body = await readJsonSafe(res);

    if (!res.ok) {
      if (res.status === 401) {
        setLoginError(parseErrorPayload(body, "Admin Key 无效，请检查后重试。"));
      } else if (res.status === 429) {
        setLoginError(parseErrorPayload(body, "失败尝试次数过多，请稍后再试。"));
      } else if (res.status === 404) {
        state.adminEnabled = false;
        showLoginView(parseErrorPayload(body, "管理后台未启用。"));
      } else {
        setLoginError(parseErrorPayload(body, `登录失败 (HTTP ${res.status})`));
      }
      els.adminKeyInput.select();
      return;
    }

    els.adminKeyInput.value = "";
    showDashboardView();
    await loadDashboardData();
  } catch {
    setLoginError("无法连接到 ToolStow 服务，请检查网络或服务状态。");
  } finally {
    els.loginSubmitBtn.disabled = state.adminEnabled === false;
    els.loginSubmitBtn.textContent = "登录后台";
  }
}

async function handleLogout() {
  els.logoutBtn.disabled = true;
  try {
    await apiRequest("/api/admin/logout", { method: "POST" });
  } catch {
    // Still switch back to login screen
  } finally {
    els.logoutBtn.disabled = false;
    state.tools = [];
    state.themes = [];
    setUploadStatus("");
    setThemeUploadStatus("");
    setSettingsFeedback("");
    showLoginView();
  }
}

async function loadDashboardData() {
  await Promise.all([fetchAdminTools(), fetchAdminThemes(), fetchAdminSettings()]);
}

async function checkSession() {
  try {
    const res = await apiRequest("/api/admin/session", { method: "GET" });
    const body = await readJsonSafe(res);

    if (res.ok && body && typeof body === "object") {
      state.adminEnabled = body.adminEnabled !== false;
      if (body.authenticated === true) {
        showDashboardView();
        await loadDashboardData();
        return;
      }
      showLoginView();
      return;
    }

    showLoginView(parseErrorPayload(body, `无法检查会话状态 (HTTP ${res.status})`));
  } catch {
    showLoginView("无法连接到 ToolStow 服务。");
  }
}

function initCurlExample() {
  const origin = window.location.origin || "http://127.0.0.1:8080";
  els.curlExample.textContent = [
    "# 上传 .zip 工具包",
    "curl -X POST \\",
    '  -H "Authorization: Bearer $TOOLSTOW_API_KEY" \\',
    '  -F "file=@my-tool.zip" \\',
    `  ${origin}/api/admin/tools/upload`,
    "",
    "# 收录单文件 HTML 工具（默认 overwrite=false；覆盖加 ?overwrite=true）",
    "curl -X POST \\",
    '  -H "Authorization: Bearer $TOOLSTOW_API_KEY" \\',
    '  -H "Content-Type: application/json" \\',
    `  -d '{"id":"hello","name":"Hello","html":"<h1>Hello</h1>","draft":true}' \\`,
    `  ${origin}/api/admin/tools/import/html`,
  ].join("\n");
}

/* ───────────── Top-level tabs ───────────── */

function setActiveTab(name, { focus = false, updateHash = true } = {}) {
  const next = TAB_NAMES.includes(name) ? name : "tools";
  state.activeTab = next;

  for (const tab of els.mainTabs) {
    const active = tab.dataset.tab === next;
    tab.classList.toggle("active", active);
    tab.setAttribute("aria-selected", active ? "true" : "false");
    tab.tabIndex = active ? 0 : -1;
    if (active && focus) tab.focus();
  }
  for (const panel of els.tabPanels) {
    panel.hidden = panel.id !== `panel-${next}`;
  }

  if (updateHash) {
    try {
      const base = `${window.location.pathname}${window.location.search}`;
      history.replaceState(null, "", next === "tools" ? base : `${base}#${next}`);
    } catch {
      // Ignore history errors.
    }
  }
}

function handleTabKeydown(event) {
  const idx = TAB_NAMES.indexOf(state.activeTab);
  let nextIdx = -1;
  if (event.key === "ArrowRight") nextIdx = (idx + 1) % TAB_NAMES.length;
  else if (event.key === "ArrowLeft") nextIdx = (idx - 1 + TAB_NAMES.length) % TAB_NAMES.length;
  else if (event.key === "Home") nextIdx = 0;
  else if (event.key === "End") nextIdx = TAB_NAMES.length - 1;
  if (nextIdx < 0) return;
  event.preventDefault();
  setActiveTab(TAB_NAMES[nextIdx], { focus: true });
}

/* ───────────── Unified ingestion (.zip / .html / paste) ───────────── */

function ingestFile(file) {
  if (!file) return;
  const lower = file.name.toLowerCase();
  if (lower.endsWith(".zip")) {
    uploadZipFile(file);
    return;
  }
  if (lower.endsWith(".html") || lower.endsWith(".htm")) {
    openHtmlFromFile(file);
    return;
  }
  els.uploadFileInput.value = "";
  setUploadStatus("仅支持 .zip 工具包或 .html / .htm 单文件。", "error");
}

async function openHtmlFromFile(file) {
  els.uploadFileInput.value = "";
  if (file.size > MAX_HTML_BYTES) {
    setUploadStatus(`HTML 文件过大（上限 ${Math.round(MAX_HTML_BYTES / 1024 / 1024)} MB）。`, "error");
    return;
  }
  try {
    const text = await file.text();
    if (!text.trim()) {
      setUploadStatus("HTML 文件内容为空。", "error");
      return;
    }
    setUploadStatus("");
    openHtmlDialog({ html: text, fileName: file.name, pasteMode: false });
  } catch {
    setUploadStatus("读取 HTML 文件失败，请重试。", "error");
  }
}

function slugifyAscii(text) {
  return String(text || "")
    .normalize("NFKD")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function clampSlug(slug, max = 48) {
  return slug.slice(0, max).replace(/-+$/, "");
}

function shortHash(text) {
  // FNV-1a 32-bit, base36, padded to a stable 6 chars.
  let hash = 0x811c9dc5;
  const input = String(text || "");
  for (let i = 0; i < input.length; i += 1) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(36).padStart(6, "0").slice(0, 6);
}

function deriveToolId({ title, fileName, html }) {
  const fromTitle = clampSlug(slugifyAscii(title));
  if (fromTitle) return fromTitle;
  const stem = String(fileName || "").replace(/\.[^.]+$/, "");
  const fromFile = clampSlug(slugifyAscii(stem));
  if (fromFile) return fromFile;
  return `html-tool-${shortHash(html || title || fileName)}`;
}

function parseHtmlMetadata(html) {
  let title = "";
  let description = "";
  try {
    const doc = new DOMParser().parseFromString(html, "text/html");
    title = (doc.title || "").replace(/\s+/g, " ").trim();
    for (const meta of doc.querySelectorAll("meta")) {
      if ((meta.getAttribute("name") || "").trim().toLowerCase() === "description") {
        description = (meta.getAttribute("content") || "").replace(/\s+/g, " ").trim();
        break;
      }
    }
  } catch {
    // Ignore parse failures; the user can fill the fields manually.
  }
  return { title: title.slice(0, 80), description: description.slice(0, 240) };
}

function htmlReferencesSdk(html) {
  return typeof html === "string" && html.includes(SDK_CSS_PATH);
}

function setHtmlError(message) {
  els.htmlError.textContent = message || "";
  els.htmlError.hidden = !message;
}

function setHtmlSubmitting(submitting) {
  if (state.html) state.html.submitting = submitting;
  els.htmlSubmitBtn.disabled = submitting;
  els.htmlSubmitBtn.textContent = submitting ? "收录中..." : "收录工具";
}

function updateHtmlIdHint() {
  const value = els.htmlId.value.trim();
  let message = "仅限小写字母、数字与连字符，例如 my-tool";
  let kind = "info";
  if (value) {
    if (TOOL_ID_PATTERN.test(value)) {
      message = `✓ 格式有效 · 访问路径 /t/${value}/`;
      kind = "success";
    } else {
      message = "✗ 格式无效：需匹配 ^[a-z0-9]+(?:-[a-z0-9]+)*$（小写字母/数字，以单个连字符分隔）";
      kind = "error";
    }
  } else {
    message = "请填写 ID（小写字母、数字与连字符）";
    kind = "error";
  }
  els.htmlIdHint.textContent = message;
  els.htmlIdHint.dataset.kind = kind;
}

function refreshHtmlPreview() {
  els.htmlPreviewFrame.srcdoc = state.html ? state.html.html : "";
}

function schedulePreviewRefresh() {
  if (!state.html) return;
  if (state.html.previewTimer) window.clearTimeout(state.html.previewTimer);
  state.html.previewTimer = window.setTimeout(refreshHtmlPreview, 250);
}

function populateCategoryOptions() {
  const seen = new Set();
  const names = [];
  const add = (name) => {
    const trimmed = typeof name === "string" ? name.trim() : "";
    const key = trimmed.toLowerCase();
    if (!trimmed || seen.has(key)) return;
    seen.add(key);
    names.push(trimmed);
  };
  for (const tool of state.tools) add(tool.category);
  for (const name of STANDARD_CATEGORIES) add(name);

  const fragment = document.createDocumentFragment();
  for (const name of names) {
    const opt = document.createElement("option");
    opt.value = name;
    fragment.appendChild(opt);
  }
  els.htmlCategoryOptions.replaceChildren(fragment);
}

// Re-derive auto-filled fields from the current HTML unless the user already edited them.
function applyHtmlMetadataDefaults() {
  const session = state.html;
  if (!session) return;
  const meta = parseHtmlMetadata(session.html);

  if (!session.nameTouched) {
    els.htmlName.value = meta.title;
  }
  if (!session.descTouched) {
    els.htmlDesc.value = meta.description;
  }
  if (!session.idTouched) {
    els.htmlId.value = deriveToolId({
      title: els.htmlName.value.trim() || meta.title,
      fileName: session.fileName,
      html: session.html,
    });
  }
  if (!session.injectTouched) {
    els.htmlInjectSdk.checked = !htmlReferencesSdk(session.html);
  }
  updateHtmlIdHint();
}

function openHtmlDialog({ html = "", fileName = "", pasteMode = false } = {}) {
  if (typeof els.htmlDialog.showModal !== "function") {
    setUploadStatus("当前浏览器不支持 <dialog>，请改用 .zip 上传或 API。", "error");
    return;
  }

  if (state.html && state.html.previewTimer) window.clearTimeout(state.html.previewTimer);
  state.html = {
    html,
    fileName,
    pasteMode,
    nameTouched: false,
    descTouched: false,
    idTouched: false,
    injectTouched: false,
    submitting: false,
    previewTimer: null,
  };

  els.htmlSourceField.hidden = !pasteMode;
  els.htmlSource.value = html;
  els.htmlCategory.value = "";
  els.htmlIcon.value = DEFAULT_TOOL_ICON;
  els.htmlDraft.checked = false;
  setHtmlError("");
  setHtmlSubmitting(false);
  populateCategoryOptions();
  applyHtmlMetadataDefaults();
  refreshHtmlPreview();

  if (!els.htmlDialog.open) els.htmlDialog.showModal();
  (pasteMode ? els.htmlSource : els.htmlName).focus();
}

function closeHtmlDialog() {
  if (els.htmlDialog.open) {
    els.htmlDialog.close();
  } else {
    resetHtmlSession();
  }
}

function resetHtmlSession() {
  if (state.html && state.html.previewTimer) window.clearTimeout(state.html.previewTimer);
  state.html = null;
  // Stop any running preview scripts.
  els.htmlPreviewFrame.srcdoc = "";
  setHtmlSubmitting(false);
}

function readHtmlPayload() {
  const session = state.html;
  const payload = {
    id: els.htmlId.value.trim(),
    name: els.htmlName.value.trim(),
    html: session ? session.html : "",
    icon: els.htmlIcon.value.trim() || DEFAULT_TOOL_ICON,
    draft: els.htmlDraft.checked,
    injectSdk: els.htmlInjectSdk.checked,
  };
  const description = els.htmlDesc.value.trim();
  const category = els.htmlCategory.value.trim();
  if (description) payload.description = description;
  if (category) payload.category = category;
  return payload;
}

function validateHtmlPayload(payload) {
  if (!payload.html || !payload.html.trim()) return "HTML 内容不能为空。";
  if (!payload.name) return "请填写工具名称。";
  if (!payload.id) return "请填写工具 ID。";
  if (!TOOL_ID_PATTERN.test(payload.id)) {
    return "ID 格式无效：仅限小写字母、数字与连字符（例如 my-tool）。";
  }
  return "";
}

async function submitHtmlIngest(overwrite = false) {
  const session = state.html;
  if (!session || session.submitting) return;

  const payload = readHtmlPayload();
  const validationError = validateHtmlPayload(payload);
  if (validationError) {
    setHtmlError(validationError);
    return;
  }

  setHtmlError("");
  setHtmlSubmitting(true);

  try {
    const endpoint = overwrite
      ? "/api/admin/tools/import/html?overwrite=true"
      : "/api/admin/tools/import/html?overwrite=false";
    const res = await apiRequest(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });

    if (res.status === 401) {
      handleUnauthorized();
      return;
    }

    const body = await readJsonSafe(res);

    if (res.status === 409 && !overwrite && (!body || !body.code || body.code === "TOOL_EXISTS")) {
      openConfirmDialog({
        title: "工具已存在，确认覆盖？",
        message: formatOverwritePrompt(body, payload.id),
        confirmLabel: "确认覆盖",
        confirmKind: "primary",
        onConfirm: () => {
          submitHtmlIngest(true);
        },
        onCancel: () => {
          setHtmlError(`已取消覆盖：${payload.id}（可修改 ID 后重试）`);
        },
      });
      return;
    }

    if (!res.ok) {
      throw new Error(parseErrorPayload(body, `收录失败 (HTTP ${res.status})`));
    }

    const tool = body && body.id ? body : body && body.entry ? body.entry : null;
    const actionLabel = res.status === 201 ? "已收录" : "已覆盖更新";
    const info = tool || payload;
    const draftSuffix = info.draft ? "（草稿状态）" : "";
    const msg = `${actionLabel}：${info.name || payload.name} (${info.id || payload.id})${draftSuffix}`;

    closeHtmlDialog();
    setUploadStatus(msg, "success");
    showToast(msg, "info");
    await fetchAdminTools();
  } catch (err) {
    setHtmlError(err.message || "收录失败");
  } finally {
    setHtmlSubmitting(false);
  }
}

function bindHtmlDialogEvents() {
  els.pasteHtmlBtn.addEventListener("click", () => {
    openHtmlDialog({ html: "", fileName: "", pasteMode: true });
  });

  els.htmlForm.addEventListener("submit", (e) => {
    e.preventDefault();
    submitHtmlIngest(false);
  });

  els.htmlCloseBtn.addEventListener("click", closeHtmlDialog);
  els.htmlCancelBtn.addEventListener("click", closeHtmlDialog);
  els.htmlDialog.addEventListener("close", resetHtmlSession);

  els.htmlSource.addEventListener("input", () => {
    if (!state.html) return;
    state.html.html = els.htmlSource.value;
    applyHtmlMetadataDefaults();
    schedulePreviewRefresh();
  });

  els.htmlName.addEventListener("input", () => {
    if (!state.html) return;
    state.html.nameTouched = true;
    if (!state.html.idTouched) {
      els.htmlId.value = deriveToolId({
        title: els.htmlName.value.trim(),
        fileName: state.html.fileName,
        html: state.html.html,
      });
      updateHtmlIdHint();
    }
  });

  els.htmlId.addEventListener("input", () => {
    if (state.html) state.html.idTouched = true;
    updateHtmlIdHint();
  });

  els.htmlDesc.addEventListener("input", () => {
    if (state.html) state.html.descTouched = true;
  });

  els.htmlInjectSdk.addEventListener("change", () => {
    if (state.html) state.html.injectTouched = true;
  });
}

function bindEvents() {
  els.loginThemeToggle.addEventListener("click", toggleTheme);
  els.dashThemeToggle.addEventListener("click", toggleTheme);

  els.toggleKeyVisibility.addEventListener("click", () => {
    const isPassword = els.adminKeyInput.type === "password";
    els.adminKeyInput.type = isPassword ? "text" : "password";
    els.toggleKeyVisibility.textContent = isPassword ? "隐藏" : "显示";
    els.toggleKeyVisibility.setAttribute("aria-pressed", isPassword ? "true" : "false");
  });

  els.loginForm.addEventListener("submit", handleLoginSubmit);
  els.logoutBtn.addEventListener("click", handleLogout);
  els.checkUpdatesBtn.addEventListener("click", () => checkForUpdates({ manual: true }));
  els.autoCheckUpdates.addEventListener("change", () => {
    writeStorage(STORAGE_KEY_AUTO_UPDATE_CHECK, String(els.autoCheckUpdates.checked));
    if (els.autoCheckUpdates.checked) void checkForUpdates();
  });

  // Top-level tabs
  for (const tab of els.mainTabs) {
    tab.addEventListener("click", () => setActiveTab(tab.dataset.tab));
    tab.addEventListener("keydown", handleTabKeydown);
  }

  bindHtmlDialogEvents();

  // Upload picker & dropzone
  const openPicker = () => els.uploadFileInput.click();
  els.uploadPickBtn.addEventListener("click", openPicker);
  els.uploadDropzone.addEventListener("click", openPicker);
  els.uploadDropzone.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      openPicker();
    }
  });

  els.uploadFileInput.addEventListener("change", (e) => {
    const file = e.target.files && e.target.files[0];
    if (file) ingestFile(file);
  });

  const hasFiles = (e) =>
    e.dataTransfer && Array.from(e.dataTransfer.types || []).includes("Files");

  els.uploadDropzone.addEventListener("dragenter", (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    state.dragDepth += 1;
    els.uploadDropzone.classList.add("drag-over");
  });

  els.uploadDropzone.addEventListener("dragover", (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
  });

  els.uploadDropzone.addEventListener("dragleave", (e) => {
    if (!hasFiles(e)) return;
    state.dragDepth = Math.max(0, state.dragDepth - 1);
    if (state.dragDepth === 0) {
      els.uploadDropzone.classList.remove("drag-over");
    }
  });

  els.uploadDropzone.addEventListener("drop", (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    state.dragDepth = 0;
    els.uploadDropzone.classList.remove("drag-over");
    const file = e.dataTransfer.files && e.dataTransfer.files[0];
    if (file) ingestFile(file);
  });

  // Tools filter & search
  for (const tab of els.filterTabs) {
    tab.addEventListener("click", () => {
      state.filter = tab.dataset.filter || "all";
      renderToolsTable();
    });
  }

  els.toolsSearchInput.addEventListener("input", (e) => {
    state.search = e.target.value;
    renderToolsTable();
  });

  els.refreshToolsBtn.addEventListener("click", fetchAdminTools);

  // Theme upload picker, dropzone & refresh
  const openThemePicker = () => els.themeUploadFileInput.click();
  els.themeUploadPickBtn.addEventListener("click", openThemePicker);
  els.themeUploadDropzone.addEventListener("click", openThemePicker);
  els.themeUploadDropzone.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      openThemePicker();
    }
  });

  els.themeUploadFileInput.addEventListener("change", (e) => {
    const file = e.target.files && e.target.files[0];
    if (file) uploadThemeZipFile(file);
  });

  els.themeUploadDropzone.addEventListener("dragenter", (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    state.themeDragDepth += 1;
    els.themeUploadDropzone.classList.add("drag-over");
  });

  els.themeUploadDropzone.addEventListener("dragover", (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
  });

  els.themeUploadDropzone.addEventListener("dragleave", (e) => {
    if (!hasFiles(e)) return;
    state.themeDragDepth = Math.max(0, state.themeDragDepth - 1);
    if (state.themeDragDepth === 0) {
      els.themeUploadDropzone.classList.remove("drag-over");
    }
  });

  els.themeUploadDropzone.addEventListener("drop", (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    state.themeDragDepth = 0;
    els.themeUploadDropzone.classList.remove("drag-over");
    const file = e.dataTransfer.files && e.dataTransfer.files[0];
    if (file) uploadThemeZipFile(file);
  });

  els.refreshThemesBtn.addEventListener("click", fetchAdminThemes);

  // Settings controls
  els.settingsForm.addEventListener("submit", saveAdminSettings);

  // Copy cURL snippet
  els.copyCurlBtn.addEventListener("click", async () => {
    const text = els.curlExample.textContent || "";
    try {
      await navigator.clipboard.writeText(text);
      els.copyCurlBtn.textContent = "已复制";
      window.setTimeout(() => {
        els.copyCurlBtn.textContent = "复制";
      }, 1600);
    } catch {
      showToast("复制失败，请手动选中复制", "error");
    }
  });

  // Confirmation Dialog (Delete / Overwrite)
  els.confirmCancelBtn.addEventListener("click", () => {
    const pending = state.pendingConfirmAction;
    state.pendingConfirmAction = null;
    els.confirmDialog.close();
    if (pending && typeof pending.onCancel === "function") {
      pending.onCancel();
    }
  });

  els.confirmDialog.addEventListener("cancel", () => {
    const pending = state.pendingConfirmAction;
    state.pendingConfirmAction = null;
    if (pending && typeof pending.onCancel === "function") {
      pending.onCancel();
    }
  });

  els.confirmOkBtn.addEventListener("click", async () => {
    const pending = state.pendingConfirmAction;
    state.pendingConfirmAction = null;
    els.confirmDialog.close();
    if (pending && typeof pending.onConfirm === "function") {
      await pending.onConfirm();
    }
  });
}

async function bootstrap() {
  initTheme();
  initCurlExample();
  bindEvents();
  setActiveTab(window.location.hash.replace(/^#/, ""), { updateHash: false });
  await checkSession();
}

bootstrap();
