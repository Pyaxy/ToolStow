const DEFAULT_SITE_TITLE = "ToolStow";
const STORAGE_KEY_PHOSPHOR = "toolstow:phosphor:color";

const PHOSPHOR_META = {
  green: { label: "P1 GREEN", footer: "CRT:P1-GREEN" },
  amber: { label: "P3 AMBER", footer: "CRT:P3-AMBER" },
  ice: { label: "P4 ICE", footer: "CRT:P4-ICE" },
};

const els = {
  shell: document.querySelector(".ph-shell"),
  brandLink: document.getElementById("brand-link"),
  brandTitle: document.getElementById("brand-title"),
  searchInput: document.getElementById("search-input"),
  mobileSearchToggle: document.getElementById("mobile-search-toggle"),
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
  activeToolVisBadge: document.getElementById("active-tool-vis-badge"),
  activeToolId: document.getElementById("active-tool-id"),
  activeToolVersion: document.getElementById("active-tool-version"),
  activeToolThemeBadge: document.getElementById("active-tool-theme-badge"),
  activeToolDesc: document.getElementById("active-tool-desc"),
  activeToolTags: document.getElementById("active-tool-tags"),
  toolDetailsToggle: document.getElementById("tool-details-toggle"),
  toolDetailsPanel: document.getElementById("tool-details-panel"),
  toolDetailsBackdrop: document.getElementById("tool-details-backdrop"),
  detailsToolVisBadge: document.getElementById("details-tool-vis-badge"),
  detailsToolId: document.getElementById("details-tool-id"),
  detailsToolVersion: document.getElementById("details-tool-version"),
  detailsToolCategory: document.getElementById("details-tool-category"),
  detailsToolThemeBadge: document.getElementById("details-tool-theme-badge"),
  detailsToolDesc: document.getElementById("details-tool-desc"),
  detailsSwitcherRow: document.getElementById("details-switcher-row"),
  detailsQuickSwitcher: document.getElementById("details-quick-switcher"),
  detailsTagsSection: document.getElementById("details-tags-section"),
  detailsToolTags: document.getElementById("details-tool-tags"),
  reloadToolBtn: document.getElementById("reload-tool-btn"),
  standaloneLink: document.getElementById("standalone-link"),
  iframeHost: document.getElementById("iframe-host"),
  footerStatus: document.getElementById("footer-status"),
  footerToolsStat: document.getElementById("footer-tools-stat"),
  footerPhosphor: document.getElementById("footer-phosphor"),
};

let statusTimer = null;
let searchToolId = null;
let lastActiveToolId = null;

function toolVisibility(tool) {
  if (window.ToolStowHost && typeof window.ToolStowHost.normalizeVisibility === "function") {
    return window.ToolStowHost.normalizeVisibility(tool);
  }
  if (tool && (tool.visibility === "private" || tool.visibility === "draft" || tool.visibility === "public")) {
    return tool.visibility;
  }
  return tool && tool.draft === true ? "draft" : "public";
}

function visibilityLabel(vis) {
  if (vis === "private") return "私有";
  if (vis === "draft") return "草稿";
  return "";
}

function createVisibilityBadge(tool) {
  const vis = toolVisibility(tool);
  const label = visibilityLabel(vis);
  if (!label) return null;
  const badge = document.createElement("span");
  badge.className = "ph-vis-badge";
  badge.dataset.visibility = vis;
  badge.textContent = label;
  return badge;
}

function syncVisibilityBadge(el, tool) {
  if (!el) return;
  const vis = toolVisibility(tool);
  const label = visibilityLabel(vis);
  if (label) {
    el.textContent = label;
    el.dataset.visibility = vis;
    el.hidden = false;
  } else {
    el.textContent = "";
    delete el.dataset.visibility;
    el.hidden = true;
  }
}

function isToolDetailsOpen() {
  return Boolean(els.toolDetailsPanel && !els.toolDetailsPanel.hidden);
}

function isMobileSearchOpen() {
  return els.shell?.dataset.searchOpen === "true";
}

function setMobileSearchOpen(open, { focus = false, restoreFocus = false } = {}) {
  if (!els.shell) return;
  const next = Boolean(open);
  if (next) {
    closeToolDetails();
    els.shell.dataset.searchOpen = "true";
  } else {
    delete els.shell.dataset.searchOpen;
  }
  if (els.mobileSearchToggle) {
    els.mobileSearchToggle.setAttribute("aria-expanded", next ? "true" : "false");
  }
  if (next && focus) {
    els.searchInput.focus();
    els.searchInput.select();
  } else if (!next && restoreFocus && els.mobileSearchToggle && els.mobileSearchToggle.offsetParent !== null) {
    els.mobileSearchToggle.focus();
  }
}

function setToolDetailsOpen(open, { returnFocus = false } = {}) {
  if (!els.toolDetailsPanel || !els.toolDetailsToggle) return;
  const next = Boolean(open);
  if (next) {
    setMobileSearchOpen(false);
  }
  els.toolDetailsPanel.hidden = !next;
  if (els.toolDetailsBackdrop) {
    els.toolDetailsBackdrop.hidden = !next;
  }
  els.toolDetailsToggle.setAttribute("aria-expanded", next ? "true" : "false");
  if (!next && returnFocus) {
    els.toolDetailsToggle.focus();
  }
}

function closeToolDetails(options) {
  if (isToolDetailsOpen()) {
    setToolDetailsOpen(false, options);
  }
}

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
    if (els.detailsToolThemeBadge) {
      els.detailsToolThemeBadge.hidden = false;
    }
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

      const nameRow = document.createElement("div");
      nameRow.className = "ph-tool-cell-head";

      const nameEl = document.createElement("span");
      nameEl.className = "ph-tool-cell-name";
      nameEl.textContent = `${tool.name} `;

      const idEl = document.createElement("span");
      idEl.className = "ph-tool-cell-id";
      idEl.textContent = `(${tool.id})`;
      nameEl.appendChild(idEl);
      nameRow.appendChild(nameEl);

      const visBadge = createVisibilityBadge(tool);
      if (visBadge) {
        nameRow.appendChild(visBadge);
      }

      const descEl = document.createElement("span");
      descEl.className = "ph-tool-cell-desc";
      descEl.textContent = tool.description || `exec /t/${tool.id}/`;

      textWrap.append(nameRow, descEl);
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
        openTool(tool.id);
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
        openTool(tool.id);
      });

      fragment.appendChild(tr);
    });
  }

  els.toolTableBody.replaceChildren(fragment);
}

function populateQuickSelect(selectEl, state) {
  if (!selectEl) return;
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
  selectEl.replaceChildren(fragment);
  selectEl.hidden = state.allTools.length === 0;
}

function renderQuickSwitcher(state) {
  populateQuickSelect(els.quickSwitcher, state);
  populateQuickSelect(els.detailsQuickSwitcher, state);
  if (els.detailsSwitcherRow) {
    els.detailsSwitcherRow.hidden = state.allTools.length <= 1;
  }
}

function buildToolTagButtons(tags, includeLabel) {
  const frag = document.createDocumentFragment();
  if (!Array.isArray(tags) || tags.length === 0) return frag;
  if (includeLabel) {
    const label = document.createElement("span");
    label.className = "ph-tag-label";
    label.textContent = "按标签返回目录";
    frag.appendChild(label);
  }
  for (const tag of tags) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "ph-tag-pill";
    btn.textContent = `#${tag}`;
    btn.setAttribute("aria-label", `返回目录并查看 #${tag} 工具`);
    btn.addEventListener("click", () => {
      closeToolDetails();
      host.setFilters({ q: "", category: "", tag });
      host.clearSelection();
    });
    frag.appendChild(btn);
  }
  return frag;
}

function renderActiveToolHeader(tool, state) {
  if (!tool) return;
  window.ToolStowHost.renderIcon(els.activeToolIcon, tool, ">_");
  els.activeToolName.textContent = tool.name;
  syncVisibilityBadge(els.activeToolVisBadge, tool);
  els.activeToolId.textContent = tool.id;
  els.activeToolVersion.textContent = tool.version ? `v${tool.version}` : "";
  els.activeToolVersion.hidden = !tool.version;
  const themeSyncActive = tool.themeAware === true || state.activeIframeReady;
  els.activeToolThemeBadge.hidden = !themeSyncActive;
  els.activeToolDesc.textContent = tool.description || "";
  els.activeToolDesc.hidden = !tool.description;

  syncVisibilityBadge(els.detailsToolVisBadge, tool);
  if (els.detailsToolId) els.detailsToolId.textContent = tool.id;
  if (els.detailsToolVersion) {
    els.detailsToolVersion.textContent = tool.version ? `v${tool.version}` : "";
    els.detailsToolVersion.hidden = !tool.version;
  }
  if (els.detailsToolCategory) {
    els.detailsToolCategory.textContent = tool.category || "";
    els.detailsToolCategory.hidden = !tool.category;
  }
  if (els.detailsToolThemeBadge) {
    els.detailsToolThemeBadge.hidden = !themeSyncActive;
  }
  if (els.detailsToolDesc) {
    els.detailsToolDesc.textContent = tool.description || "";
    els.detailsToolDesc.hidden = !tool.description;
  }

  if (els.quickSwitcher.value !== tool.id) {
    els.quickSwitcher.value = tool.id;
  }
  if (els.detailsQuickSwitcher && els.detailsQuickSwitcher.value !== tool.id) {
    els.detailsQuickSwitcher.value = tool.id;
  }

  const standaloneUrl = host.getStandaloneUrl(tool);
  if (standaloneUrl) {
    els.standaloneLink.href = standaloneUrl;
    els.standaloneLink.hidden = false;
  } else {
    els.standaloneLink.removeAttribute("href");
    els.standaloneLink.hidden = true;
  }

  els.activeToolTags.replaceChildren(buildToolTagButtons(tool.tags, true));
  const hasTags = Array.isArray(tool.tags) && tool.tags.length > 0;
  if (els.detailsToolTags) {
    els.detailsToolTags.replaceChildren(buildToolTagButtons(tool.tags, false));
  }
  if (els.detailsTagsSection) {
    els.detailsTagsSection.hidden = !hasTags;
  }
}

function openTool(id) {
  closeToolDetails();
  setMobileSearchOpen(false);
  searchToolId = null;
  host.selectTool(id);
}

function renderState(state) {
  if (searchToolId !== state.selectedId) searchToolId = null;
  const showingSearchResults = Boolean(searchToolId);
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

  const showingTool = Boolean(state.selectedTool && !showingSearchResults);
  if (els.shell) {
    els.shell.dataset.view = showingTool ? "tool" : "directory";
  }
  if (!showingTool || state.selectedId !== lastActiveToolId) {
    closeToolDetails();
  }
  if (showingTool && state.selectedId !== lastActiveToolId) {
    setMobileSearchOpen(false);
  }
  lastActiveToolId = showingTool ? state.selectedId : null;

  els.directoryView.hidden = showingTool;
  els.toolView.hidden = !showingTool;
  els.clearFiltersBtn.textContent = showingSearchResults ? "返回当前工具" : "[RESET]";
  if (showingTool) renderActiveToolHeader(state.selectedTool, state);
}

function bindEvents() {
  els.brandLink.addEventListener("click", (e) => {
    e.preventDefault();
    closeToolDetails();
    setMobileSearchOpen(false);
    host.clearSelection();
  });

  els.backToListBtn.addEventListener("click", () => {
    closeToolDetails();
    setMobileSearchOpen(false);
    host.clearSelection();
  });

  const handleQuickSwitch = (e) => {
    const nextId = e.target.value;
    if (nextId && nextId !== host.getState().selectedId) {
      openTool(nextId);
    }
  };

  els.quickSwitcher.addEventListener("change", handleQuickSwitch);
  if (els.detailsQuickSwitcher) {
    els.detailsQuickSwitcher.addEventListener("change", handleQuickSwitch);
  }

  if (els.mobileSearchToggle) {
    els.mobileSearchToggle.addEventListener("click", () => {
      const next = !isMobileSearchOpen();
      setMobileSearchOpen(next, { focus: next });
    });
  }

  if (els.toolDetailsToggle) {
    els.toolDetailsToggle.addEventListener("click", () => {
      setToolDetailsOpen(!isToolDetailsOpen());
    });
  }

  if (els.toolDetailsBackdrop) {
    els.toolDetailsBackdrop.addEventListener("click", () => {
      closeToolDetails();
    });
  }

  document.addEventListener("click", (e) => {
    if (!isToolDetailsOpen()) return;
    if (
      els.toolDetailsPanel.contains(e.target) ||
      (els.toolDetailsToggle && els.toolDetailsToggle.contains(e.target))
    ) {
      return;
    }
    closeToolDetails();
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && isToolDetailsOpen()) {
      e.preventDefault();
      closeToolDetails({ returnFocus: true });
    }
  });

  if (typeof window.matchMedia === "function") {
    const compactMq = window.matchMedia("(max-width: 820px), (max-height: 500px)");
    const onCompactChange = (e) => {
      if (!e.matches) {
        closeToolDetails();
      }
    };
    if (typeof compactMq.addEventListener === "function") {
      compactMq.addEventListener("change", onCompactChange);
    }
  }

  els.phosphorSwitch.addEventListener("click", (e) => {
    const btn = e.target.closest("[data-phosphor-choice]");
    if (!btn) return;
    applyPhosphorColor(btn.dataset.phosphorChoice, true);
  });

  els.searchInput.addEventListener("input", (e) => {
    const query = e.target.value;
    searchToolId = query.trim() ? host.getState().selectedId : null;
    host.setFilters({ q: query });
  });

  els.searchInput.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      if (els.searchInput.value) {
        searchToolId = null;
        els.searchInput.value = "";
        host.setFilters({ q: "" });
      } else if (els.shell?.dataset.view === "tool" && isMobileSearchOpen()) {
        e.preventDefault();
        setMobileSearchOpen(false, { restoreFocus: true });
      } else {
        els.searchInput.blur();
      }
    }
  });

  window.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      const searchHidden =
        els.shell?.dataset.view === "tool" &&
        (els.searchInput.offsetParent === null || els.searchInput.getBoundingClientRect().width === 0);
      if (searchHidden) {
        setMobileSearchOpen(true, { focus: true });
      } else {
        els.searchInput.focus();
        els.searchInput.select();
      }
    }
  });

  els.refreshListBtn.addEventListener("click", () => {
    host.refresh({ keepSelection: true });
  });

  els.clearFiltersBtn.addEventListener("click", () => {
    els.searchInput.value = "";
    setMobileSearchOpen(false);
    if (searchToolId) {
      searchToolId = null;
      host.setFilters({ q: "" });
    } else {
      host.clearFilters();
    }
  });

  els.reloadToolBtn.addEventListener("click", () => {
    host.reloadTool();
  });
}

applyPhosphorColor(readPhosphorStorage(), false);
bindEvents();
host.bootstrap();
