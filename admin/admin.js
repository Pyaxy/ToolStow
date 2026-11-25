const CLIENT_HEADERS = { "X-ToolStow-Client": "1" };
const STORAGE_KEY_MODE = "toolstow:mode";
const FALLBACK_ACCENT = "#18181b";

const state = {
  authenticated: false,
  adminEnabled: true,
  tools: [],
  filter: "all", // "all" | "published" | "draft"
  search: "",
  themeMode: "light",
  pendingConfirmAction: null,
  toastTimer: null,
  dragDepth: 0,
};

const els = {
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

  // Settings
  settingsForm: document.getElementById("settings-form"),
  settingSiteTitle: document.getElementById("setting-site-title"),
  settingSiteDesc: document.getElementById("setting-site-desc"),
  settingAccentPicker: document.getElementById("setting-accent-picker"),
  settingAccentPreview: document.getElementById("setting-accent-preview"),
  settingAccentText: document.getElementById("setting-accent-text"),
  accentPresets: Array.from(document.querySelectorAll(".preset-dot")),
  saveSettingsBtn: document.getElementById("save-settings-btn"),
  settingsFeedback: document.getElementById("settings-feedback"),

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

function initTheme() {
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
  const label = state.themeMode === "dark" ? "亮色" : "暗色";
  els.loginThemeToggle.textContent = label;
  els.dashThemeToggle.textContent = label;
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

function showLoginView(reasonMessage = "") {
  state.authenticated = false;
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
}

function handleUnauthorized() {
  state.tools = [];
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
        ? "尚未安装任何工具。可在上方上传 .zip 工具包。"
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

function syncAccentControls(hex) {
  const normalized = normalizeHexColor(hex);
  if (!normalized) return;
  els.settingAccentPicker.value = normalized;
  els.settingAccentPreview.style.backgroundColor = normalized;
  els.settingAccentText.value = normalized;
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
    syncAccentControls(data.defaultAccent || FALLBACK_ACCENT);
  } catch (err) {
    setSettingsFeedback(err.message || "加载站点设置失败", "error");
  }
}

async function saveAdminSettings(event) {
  event.preventDefault();

  const siteTitle = els.settingSiteTitle.value.trim();
  const siteDescription = els.settingSiteDesc.value.trim();
  const accentInput = els.settingAccentText.value.trim();
  const normalizedAccent = normalizeHexColor(accentInput);

  if (!siteTitle) {
    setSettingsFeedback("站点标题不能为空", "error");
    els.settingSiteTitle.focus();
    return;
  }
  if (!normalizedAccent) {
    setSettingsFeedback("默认强调色需为有效十六进制色值（如 #18181b）", "error");
    els.settingAccentText.focus();
    return;
  }

  syncAccentControls(normalizedAccent);
  els.saveSettingsBtn.disabled = true;
  setSettingsFeedback("正在保存...", "info");

  try {
    const res = await apiRequest("/api/admin/settings", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        siteTitle,
        siteDescription,
        defaultAccent: normalizedAccent,
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
      syncAccentControls(body.defaultAccent || normalizedAccent);
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
    setUploadStatus("");
    setSettingsFeedback("");
    showLoginView();
  }
}

async function loadDashboardData() {
  await Promise.all([fetchAdminTools(), fetchAdminSettings()]);
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

function initPresetSwatches() {
  for (const btn of els.accentPresets) {
    const color = normalizeHexColor(btn.dataset.color);
    if (color) {
      btn.style.backgroundColor = color;
      btn.addEventListener("click", () => {
        syncAccentControls(color);
      });
    }
  }
}

function initCurlExample() {
  const origin = window.location.origin || "http://127.0.0.1:8080";
  els.curlExample.textContent = [
    "curl -X POST \\",
    '  -H "Authorization: Bearer $TOOLSTOW_API_KEY" \\',
    '  -F "file=@my-tool.zip" \\',
    `  ${origin}/api/admin/tools/upload`,
  ].join("\n");
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
    if (file) uploadZipFile(file);
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
    if (file) uploadZipFile(file);
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

  // Settings controls
  els.settingAccentPicker.addEventListener("input", (e) => {
    syncAccentControls(e.target.value);
  });

  els.settingAccentText.addEventListener("input", (e) => {
    const normalized = normalizeHexColor(e.target.value);
    if (normalized) {
      els.settingAccentPicker.value = normalized;
      els.settingAccentPreview.style.backgroundColor = normalized;
    }
  });

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
  initPresetSwatches();
  initCurlExample();
  bindEvents();
  await checkSession();
}

bootstrap();
