const DEFAULT_SITE_TITLE = "ToolStow";

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

let statusTimer = null;
let searchTimer = null;

function showStatus(message, kind = "info", autoHideMs = 4500) {
  if (statusTimer) {
    window.clearTimeout(statusTimer);
    statusTimer = null;
  }
  els.statusBanner.textContent = message;
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
  defaultMode: "light",
  fallbackAccentLight: "#2563eb",
  fallbackAccentDark: "#3b82f6",
  titleSeparator: " · ",
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

function renderCategories(state) {
  els.statTotal.textContent = String(state.allTools.length);
  els.statShown.textContent = String(state.filteredTools.length);
  els.statCats.textContent = String(state.categories.length);

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
      const next = value === "" ? "" : state.filters.category === value ? "" : value;
      host.setFilters({ category: next });
    });
    return btn;
  };

  fragment.appendChild(makeTab("全部", "", state.allTools.length));
  for (const cat of state.categories) {
    fragment.appendChild(makeTab(cat.name, cat.name, cat.count));
  }

  els.categoryTabs.replaceChildren(fragment);
}

function renderTags(state) {
  const fragment = document.createDocumentFragment();

  if (state.tags.length === 0) {
    const empty = document.createElement("span");
    empty.className = "ops-tag-empty";
    empty.textContent = "暂无标签";
    fragment.appendChild(empty);
  } else {
    for (const tag of state.tags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "ops-tag-pill" + (state.filters.tag === tag ? " active" : "");
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
      window.ToolStowHost.renderIcon(iconWrap, tool, "🛠️");

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
          const pill = document.createElement("span");
          pill.className = "ops-tag-pill";
          pill.textContent = `#${tag}`;
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
        host.selectTool(tool.id, { updateHash: true, forceReload: false });
      });
      actionsWrap.appendChild(openBtn);

      const standaloneUrl = host.getStandaloneUrl(tool);
      if (standaloneUrl) {
        const extLink = document.createElement("a");
        extLink.className = "ops-btn ops-btn-xs";
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

      tr.append(tdTool, tdId, tdCat, tdTags, tdVer, tdActions);
      tr.addEventListener("click", () => {
        host.selectTool(tool.id, { updateHash: true, forceReload: false });
      });

      fragment.appendChild(tr);
    }
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
  window.ToolStowHost.renderIcon(els.activeToolIcon, tool, "🛠️");
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
    label.className = "ops-tag-bar-label";
    label.textContent = "按标签返回目录";
    tagFrag.appendChild(label);
    for (const tag of tool.tags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "ops-tag-pill";
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
  els.siteDescPill.textContent = state.site.siteDescription || "";
  els.adminLink.hidden = state.site.isAdmin !== true;
  els.themeToggle.textContent = state.theme.mode === "dark" ? "亮色" : "暗色";

  if (els.searchInput.value !== state.filters.q) {
    els.searchInput.value = state.filters.q;
  }

  renderCategories(state);
  renderTags(state);
  renderTable(state);
  renderQuickSwitcher(state);

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

  els.themeToggle.addEventListener("click", () => {
    host.toggleThemeMode();
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

bindEvents();
host.bootstrap();
