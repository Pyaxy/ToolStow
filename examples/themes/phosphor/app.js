const DEFAULT_SITE_TITLE = "ToolStow";
const STORAGE_KEY_PHOSPHOR = "toolstow:phosphor:color";

const PHOSPHOR_META = {
  green: { label: "P1 GREEN", footer: "CRT:P1-GREEN" },
  amber: { label: "P3 AMBER", footer: "CRT:P3-AMBER" },
  ice: { label: "P4 ICE", footer: "CRT:P4-ICE" },
};

const els = {
  brandLink: document.getElementById("brand-link"),
  brandTitle: document.getElementById("brand-title"),
  searchInput: document.getElementById("search-input"),
  phosphorSwitch: document.getElementById("phosphor-switch"),
  adminLink: document.getElementById("admin-link"),
  statusBanner: document.getElementById("status-banner"),
  directoryView: document.getElementById("directory-view"),
  siteDescLine: document.getElementById("site-desc-line"),
  clearFiltersBtn: document.getElementById("clear-filters-btn"),
  refreshListBtn: document.getElementById("refresh-list-btn"),
  meterMountFill: document.getElementById("meter-mount-fill"),
  meterCatsFill: document.getElementById("meter-cats-fill"),
  statShown: document.getElementById("stat-shown"),
  statTotal: document.getElementById("stat-total"),
  statCats: document.getElementById("stat-cats"),
  statPhosphor: document.getElementById("stat-phosphor"),
  categoryTabs: document.getElementById("category-tabs"),
  tagList: document.getElementById("tag-list"),
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
  footerStatus: document.getElementById("footer-status"),
  footerToolsStat: document.getElementById("footer-tools-stat"),
  footerPhosphor: document.getElementById("footer-phosphor"),
};

let statusTimer = null;
let searchTimer = null;

function readPhosphorStorage() {
  try {
    const val = window.localStorage.getItem(STORAGE_KEY_PHOSPHOR);
    return val && Object.prototype.hasOwnProperty.call(PHOSPHOR_META, val) ? val : "green";
  } catch {
    return "green";
  }
}

function writePhosphorStorage(color) {
  try {
    window.localStorage.setItem(STORAGE_KEY_PHOSPHOR, color);
  } catch {
    // Ignore storage failures.
  }
}

function applyPhosphorColor(color, syncIframe = true) {
  const safeColor = Object.prototype.hasOwnProperty.call(PHOSPHOR_META, color) ? color : "green";
  document.documentElement.dataset.tsSkin = safeColor;

  const meta = PHOSPHOR_META[safeColor];
  els.statPhosphor.textContent = meta.label;
  els.footerPhosphor.textContent = meta.footer;

  const buttons = els.phosphorSwitch.querySelectorAll("[data-phosphor-choice]");
  for (const btn of buttons) {
    const active = btn.dataset.phosphorChoice === safeColor;
    btn.classList.toggle("active", active);
    btn.setAttribute("aria-pressed", active ? "true" : "false");
  }

  writePhosphorStorage(safeColor);
  if (syncIframe && host) {
    host.syncTheme();
  }
}

function showStatus(message, kind = "info", autoHideMs = 4500) {
  if (statusTimer) {
    window.clearTimeout(statusTimer);
    statusTimer = null;
  }
  els.statusBanner.textContent = `[${kind.toUpperCase()}] ${message}`;
  els.statusBanner.dataset.kind = kind;
  els.statusBanner.hidden = false;

  if (autoHideMs > 0) {
    statusTimer = window.setTimeout(() => {
      els.statusBanner.hidden = true;
    }, autoHideMs);
  }
}

function clearErrorBanner() {
  if (els.statusBanner.dataset.kind === "error") {
    els.statusBanner.hidden = true;
  }
}

const host = window.ToolStowHost.create({
  iframeHost: els.iframeHost,
  lockMode: "dark",
  defaultMode: "dark",
  fallbackAccentLight: "#33ff66",
  fallbackAccentDark: "#33ff66",
  titleSeparator: " :: ",
  onStateChange(state) {
    if (!state.error) {
      clearErrorBanner();
    }
    renderState(state);
  },
  onError(message) {
    showStatus(message, "error", 0);
  },
  onToolReady() {
    els.activeToolThemeBadge.hidden = false;
  },
});

function renderMetersAndCategories(state) {
  const shown = state.filteredTools.length;
  const total = state.allTools.length;
  const cats = state.categories.length;

  els.statShown.textContent = String(shown);
  els.statTotal.textContent = String(total);
  els.statCats.textContent = String(cats);

  const mountPct = total > 0 ? Math.round((shown / total) * 100) : 0;
  els.meterMountFill.style.width = `${mountPct}%`;
  const catPct = Math.min(100, cats * 16);
  els.meterCatsFill.style.width = `${catPct}%`;

  const fragment = document.createDocumentFragment();
  const makeTab = (label, value, count) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.setAttribute("role", "tab");
    const isActive = state.filters.category === value;
    btn.className = "ph-cat-tab" + (isActive ? " active" : "");
    btn.setAttribute("aria-selected", isActive ? "true" : "false");

    const nameSpan = document.createElement("span");
    nameSpan.textContent = label;

    const countSpan = document.createElement("span");
    countSpan.className = "ph-cat-count";
    countSpan.textContent = `[${count}]`;

    btn.append(nameSpan, countSpan);
    btn.addEventListener("click", () => {
      const next = value === "" ? "" : state.filters.category === value ? "" : value;
      host.setFilters({ category: next });
    });
    return btn;
  };

  fragment.appendChild(makeTab("ALL", "", total));
  for (const cat of state.categories) {
    fragment.appendChild(makeTab(cat.name, cat.name, cat.count));
  }

  els.categoryTabs.replaceChildren(fragment);
}

function renderTags(state) {
  const fragment = document.createDocumentFragment();

  if (state.tags.length === 0) {
    const empty = document.createElement("span");
    empty.className = "ph-tag-empty";
    empty.textContent = "(none)";
    fragment.appendChild(empty);
  } else {
    for (const tag of state.tags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "ph-tag-pill" + (state.filters.tag === tag ? " active" : "");
      btn.setAttribute("aria-pressed", String(state.filters.tag === tag));
      btn.textContent = `#${tag}`;
      btn.addEventListener("click", () => {
        host.setFilters({ tag: state.filters.tag === tag ? "" : tag });
      });
      fragment.appendChild(btn);
    }
  }

  els.tagList.replaceChildren(fragment);
  els.clearFiltersBtn.hidden = !host.hasActiveFilters();
}

function renderTable(state) {
  const fragment = document.createDocumentFragment();

  if (state.filteredTools.length === 0) {
    const tr = document.createElement("tr");
    const td = document.createElement("td");
    td.colSpan = 6;
    td.className = "ph-empty-cell";
    td.textContent =
      state.allTools.length === 0
        ? "[EMPTY] 暂无已发布工具，请管理员在后台上传发布。"
        : "[NO MATCH] 当前过滤条件下没有匹配的工具。";
    tr.appendChild(td);
    fragment.appendChild(tr);
  } else {
    state.filteredTools.forEach((tool, idx) => {
      const tr = document.createElement("tr");
      tr.className = "ph-row";

      // Column 1: PID
      const tdPid = document.createElement("td");
      tdPid.className = "col-pid";
      tdPid.textContent = String(idx + 1).padStart(2, "0");

      // Column 2: Icon + Name + ID + Desc
      const tdTool = document.createElement("td");
      tdTool.className = "col-tool";
      const toolCell = document.createElement("div");
      toolCell.className = "ph-tool-cell";

      const iconWrap = document.createElement("span");
      iconWrap.className = "ph-tool-icon";
      window.ToolStowHost.renderIcon(iconWrap, tool, ">_");

      const textWrap = document.createElement("div");
      textWrap.className = "ph-tool-cell-text";

      const nameEl = document.createElement("span");
      nameEl.className = "ph-tool-cell-name";
      nameEl.textContent = `${tool.name} `;

      const idEl = document.createElement("span");
      idEl.className = "ph-tool-cell-id";
      idEl.textContent = `(${tool.id})`;
      nameEl.appendChild(idEl);

      const descEl = document.createElement("span");
      descEl.className = "ph-tool-cell-desc";
      descEl.textContent = tool.description || `exec /t/${tool.id}/`;

      textWrap.append(nameEl, descEl);
      toolCell.append(iconWrap, textWrap);
      tdTool.appendChild(toolCell);

      // Column 3: Group
      const tdCat = document.createElement("td");
      tdCat.className = "col-cat";
      const catBadge = document.createElement("span");
      catBadge.className = "ph-mono-pill";
      catBadge.textContent = tool.category || "未分类";
      tdCat.appendChild(catBadge);

      // Column 4: Tags
      const tdTags = document.createElement("td");
      tdTags.className = "col-tags";
      const tagsWrap = document.createElement("div");
      tagsWrap.className = "ph-row-tags";
      if (Array.isArray(tool.tags) && tool.tags.length > 0) {
        for (const tag of tool.tags.slice(0, 3)) {
          const pill = document.createElement("span");
          pill.className = "ph-tag-pill";
          pill.textContent = `#${tag}`;
          tagsWrap.appendChild(pill);
        }
      } else {
        const dash = document.createElement("span");
        dash.className = "ph-tag-empty";
        dash.textContent = "—";
        tagsWrap.appendChild(dash);
      }
      tdTags.appendChild(tagsWrap);

      // Column 5: Version + Sync
      const tdVer = document.createElement("td");
      tdVer.className = "col-ver";
      const verWrap = document.createElement("div");
      verWrap.className = "ph-ver-cell";
      const verCode = document.createElement("span");
      verCode.className = "ph-mono-pill";
      verCode.textContent = tool.version ? `v${tool.version}` : "static";
      verWrap.appendChild(verCode);
      if (tool.themeAware) {
        const syncPill = document.createElement("span");
        syncPill.className = "ph-sync-pill";
        syncPill.textContent = "SYNC";
        verWrap.appendChild(syncPill);
      }
      tdVer.appendChild(verWrap);

      // Column 6: Actions
      const tdActions = document.createElement("td");
      tdActions.className = "col-actions";
      const actionsWrap = document.createElement("div");
      actionsWrap.className = "ph-row-actions";

      const runBtn = document.createElement("button");
      runBtn.type = "button";
      runBtn.className = "ph-btn ph-btn-xs ph-btn-accent";
      runBtn.textContent = "[RUN]";
      runBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        host.selectTool(tool.id, { updateHash: true, forceReload: false });
      });
      actionsWrap.appendChild(runBtn);

      const standaloneUrl = host.getStandaloneUrl(tool);
      if (standaloneUrl) {
        const extLink = document.createElement("a");
        extLink.className = "ph-btn ph-btn-xs";
        extLink.href = standaloneUrl;
        extLink.target = "_blank";
        extLink.rel = "noopener noreferrer";
        extLink.title = "在新标签页打开";
        extLink.setAttribute("aria-label", `在新标签页打开 ${tool.name}`);
        extLink.textContent = "↗";
        extLink.addEventListener("click", (e) => e.stopPropagation());
        actionsWrap.appendChild(extLink);
      }

      tdActions.appendChild(actionsWrap);

      tr.append(tdPid, tdTool, tdCat, tdTags, tdVer, tdActions);
      tr.addEventListener("click", () => {
        host.selectTool(tool.id, { updateHash: true, forceReload: false });
      });

      fragment.appendChild(tr);
    });
  }

  els.toolTableBody.replaceChildren(fragment);
}

function renderQuickSwitcher(state) {
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

function renderActiveToolHeader(tool, state) {
  if (!tool) return;
  window.ToolStowHost.renderIcon(els.activeToolIcon, tool, ">_");
  els.activeToolName.textContent = tool.name;
  els.activeToolId.textContent = tool.id;
  els.activeToolVersion.textContent = tool.version ? `v${tool.version}` : "";
  els.activeToolVersion.hidden = !tool.version;
  els.activeToolThemeBadge.hidden = !(tool.themeAware === true || state.activeIframeReady);
  els.activeToolDesc.textContent = tool.description || "";
  els.activeToolDesc.hidden = !tool.description;

  if (els.quickSwitcher.value !== tool.id) {
    els.quickSwitcher.value = tool.id;
  }

  const standaloneUrl = host.getStandaloneUrl(tool);
  if (standaloneUrl) {
    els.standaloneLink.href = standaloneUrl;
    els.standaloneLink.hidden = false;
  } else {
    els.standaloneLink.removeAttribute("href");
    els.standaloneLink.hidden = true;
  }

  const tagFrag = document.createDocumentFragment();
  if (Array.isArray(tool.tags) && tool.tags.length > 0) {
    const label = document.createElement("span");
    label.className = "ph-tag-label";
    label.textContent = "按标签返回目录";
    tagFrag.appendChild(label);
    for (const tag of tool.tags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "ph-tag-pill";
      btn.textContent = `#${tag}`;
      btn.setAttribute("aria-label", `返回目录并查看 #${tag} 工具`);
      btn.addEventListener("click", () => {
        host.setFilters({ q: "", category: "", tag });
        host.clearSelection();
      });
      tagFrag.appendChild(btn);
    }
  }
  els.activeToolTags.replaceChildren(tagFrag);
}

function renderState(state) {
  els.brandTitle.textContent = state.site.siteTitle || DEFAULT_SITE_TITLE;
  els.siteDescLine.textContent = state.site.siteDescription || "";
  els.adminLink.hidden = state.site.isAdmin !== true;

  if (els.searchInput.value !== state.filters.q) {
    els.searchInput.value = state.filters.q;
  }

  renderMetersAndCategories(state);
  renderTags(state);
  renderTable(state);
  renderQuickSwitcher(state);

  els.footerToolsStat.textContent = `TOOLS:${String(state.filteredTools.length).padStart(2, "0")}/${String(state.allTools.length).padStart(2, "0")}`;
  els.footerStatus.textContent = state.selectedTool ? `PROC:${state.selectedTool.id}` : "NOC:ONLINE";

  if (state.selectedTool) {
    els.directoryView.hidden = true;
    els.toolView.hidden = false;
    renderActiveToolHeader(state.selectedTool, state);
  } else {
    els.toolView.hidden = true;
    els.directoryView.hidden = false;
  }
}

function bindEvents() {
  els.brandLink.addEventListener("click", (e) => {
    e.preventDefault();
    host.clearSelection();
  });

  els.backToListBtn.addEventListener("click", () => {
    host.clearSelection();
  });

  els.quickSwitcher.addEventListener("change", (e) => {
    const nextId = e.target.value;
    if (nextId && nextId !== host.getState().selectedId) {
      host.selectTool(nextId, { updateHash: true, forceReload: false });
    }
  });

  els.phosphorSwitch.addEventListener("click", (e) => {
    const btn = e.target.closest("[data-phosphor-choice]");
    if (!btn) return;
    applyPhosphorColor(btn.dataset.phosphorChoice, true);
  });

  els.searchInput.addEventListener("input", (e) => {
    const query = e.target.value;
    if (host.getState().selectedId) {
      host.clearSelection();
    }
    if (searchTimer) window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => {
      host.setFilters({ q: query });
    }, 150);
  });

  els.searchInput.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      if (els.searchInput.value) {
        els.searchInput.value = "";
        host.setFilters({ q: "" });
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
    host.refresh({ keepSelection: true });
  });

  els.clearFiltersBtn.addEventListener("click", () => {
    els.searchInput.value = "";
    host.clearFilters();
  });

  els.reloadToolBtn.addEventListener("click", () => {
    host.reloadTool();
  });
}

applyPhosphorColor(readPhosphorStorage(), false);
bindEvents();
host.bootstrap();
