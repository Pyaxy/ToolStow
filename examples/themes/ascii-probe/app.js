const DEFAULT_SITE_TITLE = "ToolStow";

const els = {
  shell: document.querySelector(".tui-shell"),
  brandLink: document.getElementById("brand-link"),
  brandTitle: document.getElementById("brand-title"),
  sidebarToggle: document.getElementById("sidebar-toggle"),
  toolSidebarBtn: document.getElementById("tool-sidebar-btn"),
  sidebar: document.getElementById("sidebar"),
  sidebarBackdrop: document.getElementById("sidebar-backdrop"),
  searchInput: document.getElementById("search-input"),
  mobileSearchToggle: document.getElementById("mobile-search-toggle"),
  themeToggle: document.getElementById("theme-toggle"),
  adminLink: document.getElementById("admin-link"),
  statusBanner: document.getElementById("status-banner"),
  refreshListBtn: document.getElementById("refresh-list-btn"),
  clearFiltersBtn: document.getElementById("clear-filters-btn"),
  categoryList: document.getElementById("category-list"),
  tagList: document.getElementById("tag-list"),
  toolsBox: document.getElementById("tools-box"),
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
  toolDetailsToggle: document.getElementById("tool-details-toggle"),
  tuiDetailsPanel: document.getElementById("tui-details-panel"),
  detailsToolName: document.getElementById("details-tool-name"),
  detailsToolId: document.getElementById("details-tool-id"),
  detailsToolVersion: document.getElementById("details-tool-version"),
  detailsToolThemeBadge: document.getElementById("details-tool-theme-badge"),
  detailsToolDesc: document.getElementById("details-tool-desc"),
  detailsSwitcherRow: document.getElementById("details-switcher-row"),
  detailsQuickSwitcher: document.getElementById("details-quick-switcher"),
  detailsToolTags: document.getElementById("details-tool-tags"),
  reloadToolBtn: document.getElementById("reload-tool-btn"),
  standaloneLink: document.getElementById("standalone-link"),
  closeToolBtn: document.getElementById("close-tool-btn"),
  iframeHost: document.getElementById("iframe-host"),
  footerStatus: document.getElementById("footer-status"),
  footerToolsStat: document.getElementById("footer-tools-stat"),
};

let statusTimer = null;
let searchTimer = null;
let lastSidebarTrigger = null;
let lastSelectedToolId = null;

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

function setMobileSearchOpen(open, { focus = false, restoreFocus = false } = {}) {
  if (!els.shell) return;
  const isOpen = Boolean(open);
  if (isOpen) {
    setDetailsOpen(false);
    els.shell.dataset.searchOpen = "true";
  } else {
    delete els.shell.dataset.searchOpen;
  }
  if (els.mobileSearchToggle) {
    els.mobileSearchToggle.setAttribute("aria-expanded", String(isOpen));
  }
  if (isOpen && focus) {
    els.searchInput.focus();
    els.searchInput.select();
  } else if (!isOpen && restoreFocus && els.mobileSearchToggle && els.mobileSearchToggle.offsetParent !== null) {
    els.mobileSearchToggle.focus();
  }
}

function setDetailsOpen(open, { restoreFocus = false } = {}) {
  const isOpen = Boolean(open);
  if (isOpen) {
    setMobileSearchOpen(false);
  }
  els.tuiDetailsPanel.hidden = !isOpen;
  els.toolDetailsToggle.setAttribute("aria-expanded", String(isOpen));
  if (!isOpen && restoreFocus && !els.toolDetailsToggle.hidden) {
    els.toolDetailsToggle.focus();
  }
}

function openMobileSidebar(trigger) {
  setDetailsOpen(false);
  lastSidebarTrigger = trigger || els.sidebarToggle;
  els.sidebar.classList.add("open");
  els.sidebarBackdrop.hidden = false;
  els.sidebarToggle.setAttribute("aria-expanded", "true");
  els.toolSidebarBtn.setAttribute("aria-expanded", "true");
}

function closeMobileSidebar({ restoreFocus = false } = {}) {
  const shouldFocus =
    restoreFocus ||
    (els.sidebar.classList.contains("open") && els.sidebar.contains(document.activeElement));
  els.sidebar.classList.remove("open");
  els.sidebarBackdrop.hidden = true;
  els.sidebarToggle.setAttribute("aria-expanded", "false");
  els.toolSidebarBtn.setAttribute("aria-expanded", "false");
  if (shouldFocus) {
    const target =
      lastSidebarTrigger && lastSidebarTrigger.offsetParent !== null
        ? lastSidebarTrigger
        : els.toolSidebarBtn.offsetParent !== null
          ? els.toolSidebarBtn
          : els.sidebarToggle;
    target.focus();
  }
}

const host = window.ToolStowHost.create({
  iframeHost: els.iframeHost,
  defaultMode: "dark",
  fallbackAccentLight: "#047857",
  fallbackAccentDark: "#10b981",
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
    els.detailsToolThemeBadge.hidden = false;
  },
});

function renderCategories(state) {
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
      const next = value === "" ? "" : state.filters.category === value ? "" : value;
      host.setFilters({ category: next });
    });

    li.appendChild(btn);
    return li;
  };

  fragment.appendChild(makeItem("ALL", "", state.allTools.length));
  for (const cat of state.categories) {
    fragment.appendChild(makeItem(cat.name, cat.name, cat.count));
  }

  els.categoryList.replaceChildren(fragment);
}

function renderTags(state) {
  const fragment = document.createDocumentFragment();

  if (state.tags.length === 0) {
    const empty = document.createElement("span");
    empty.className = "tui-list-empty";
    empty.textContent = "(none)";
    fragment.appendChild(empty);
  } else {
    for (const tag of state.tags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "tui-tag-chip" + (state.filters.tag === tag ? " active" : "");
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

function renderToolList(state) {
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
      window.ToolStowHost.renderIcon(iconWrap, tool, "#");

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
        host.selectTool(tool.id, { updateHash: true, forceReload: false });
        closeMobileSidebar();
      });

      li.appendChild(btn);
      fragment.appendChild(li);
    }
  }

  els.toolList.replaceChildren(fragment);
}

function buildHeroCommand(filters) {
  const parts = ["toolstow --list"];
  if (filters.category) parts.push(`--cat="${filters.category}"`);
  if (filters.tag) parts.push(`--tag="${filters.tag}"`);
  if (filters.q.trim()) parts.push(`--grep="${filters.q.trim()}"`);
  if (parts.length === 1) parts.push("--all");
  return parts.join(" ");
}

function renderEmptyGrid(state) {
  const fragment = document.createDocumentFragment();
  const list = state.filteredTools;

  if (list.length === 0) {
    const box = document.createElement("div");
    box.className = "tui-card-empty";
    const msg = document.createElement("span");
    if (state.loading) {
      msg.textContent = "[SYNC] 正在加载工具列表...";
      box.appendChild(msg);
    } else if (state.error && state.allTools.length === 0) {
      msg.textContent = "[ERROR] 加载工具列表失败，请检查服务后重试。";
      const retryBtn = document.createElement("button");
      retryBtn.type = "button";
      retryBtn.className = "tui-btn";
      retryBtn.textContent = "[RETRY]";
      retryBtn.addEventListener("click", () => {
        host.refresh();
      });
      box.append(msg, retryBtn);
    } else {
      msg.textContent =
        state.allTools.length === 0
          ? "[EMPTY] 暂无已发布工具，请管理员在后台上传发布。"
          : "[NO MATCH] 当前过滤条件下没有匹配的工具。";
      box.appendChild(msg);
    }
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
      window.ToolStowHost.renderIcon(iconWrap, tool, "#");

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
        host.selectTool(tool.id, { updateHash: true, forceReload: false });
      });

      fragment.appendChild(card);
    }
  }

  els.emptyGrid.replaceChildren(fragment);
}

function updateEmptyState(state) {
  const title = state.site.siteTitle || DEFAULT_SITE_TITLE;
  els.emptyTitle.textContent = title;
  els.heroCmdText.textContent = buildHeroCommand(state.filters);
  els.emptyCount.textContent = `${state.filteredTools.length} READY`;

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

  renderEmptyGrid(state);
}

function buildTagReturnFragment(tags, { includeLabel = false } = {}) {
  const tagFrag = document.createDocumentFragment();
  if (Array.isArray(tags) && tags.length > 0) {
    if (includeLabel) {
      const label = document.createElement("span");
      label.className = "tui-details-label";
      label.textContent = "TAGS";
      tagFrag.appendChild(label);
    }
    for (const tag of tags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "tui-tag-chip";
      btn.textContent = `#${tag}`;
      btn.setAttribute("aria-label", `返回目录并查看 #${tag} 工具`);
      btn.addEventListener("click", () => {
        setDetailsOpen(false);
        host.setFilters({ q: "", category: "", tag });
        host.clearSelection();
      });
      tagFrag.appendChild(btn);
    }
  }
  return tagFrag;
}

function renderDetailsQuickSwitcher(state) {
  const hasMultiple = state.allTools.length > 1;
  const fragment = document.createDocumentFragment();
  for (const tool of state.allTools) {
    const opt = document.createElement("option");
    opt.value = tool.id;
    opt.textContent = tool.name;
    opt.selected = tool.id === state.selectedId;
    fragment.appendChild(opt);
  }
  els.detailsQuickSwitcher.replaceChildren(fragment);
  els.detailsSwitcherRow.hidden = !hasMultiple;
}

function renderActiveToolHeader(tool, state) {
  if (!tool) return;
  window.ToolStowHost.renderIcon(els.activeToolIcon, tool, "#");
  const versionText = tool.version ? `v${tool.version}` : "";
  const themeReady = tool.themeAware === true || state.activeIframeReady;

  els.activeToolName.textContent = tool.name;
  els.activeToolId.textContent = tool.id;
  els.activeToolVersion.textContent = versionText;
  els.activeToolVersion.hidden = !tool.version;
  els.activeToolThemeBadge.hidden = !themeReady;
  els.activeToolDesc.textContent = tool.description || "";
  els.activeToolDesc.hidden = !tool.description;

  els.detailsToolName.textContent = tool.name;
  els.detailsToolId.textContent = tool.id;
  els.detailsToolVersion.textContent = versionText;
  els.detailsToolVersion.hidden = !tool.version;
  els.detailsToolThemeBadge.hidden = !themeReady;
  els.detailsToolDesc.textContent = tool.description || "";
  els.detailsToolDesc.hidden = !tool.description;

  const standaloneUrl = host.getStandaloneUrl(tool);
  if (standaloneUrl) {
    els.standaloneLink.href = standaloneUrl;
    els.standaloneLink.hidden = false;
  } else {
    els.standaloneLink.removeAttribute("href");
    els.standaloneLink.hidden = true;
  }

  els.activeToolTags.replaceChildren(buildTagReturnFragment(tool.tags));
  els.detailsToolTags.replaceChildren(buildTagReturnFragment(tool.tags, { includeLabel: true }));
}

function renderState(state) {
  els.brandTitle.textContent = state.site.siteTitle || DEFAULT_SITE_TITLE;
  els.adminLink.hidden = state.site.isAdmin !== true;
  els.themeToggle.textContent = state.theme.mode === "dark" ? "MODE:LIGHT" : "MODE:DARK";

  if (els.searchInput.value !== state.filters.q) {
    els.searchInput.value = state.filters.q;
  }

  renderCategories(state);
  renderTags(state);
  renderDetailsQuickSwitcher(state);
  els.toolsBox.hidden = !state.selectedTool;
  if (state.selectedTool) {
    renderToolList(state);
  }

  els.footerToolsStat.textContent = `TOOLS:${state.filteredTools.length}/${state.allTools.length}`;
  els.footerStatus.textContent = state.selectedTool ? `PROC:${state.selectedTool.id}` : "SYS:READY";

  const showingTool = Boolean(state.selectedTool);
  if (els.shell) {
    els.shell.dataset.view = showingTool ? "tool" : "directory";
  }

  if (showingTool) {
    if (lastSelectedToolId !== state.selectedTool.id) {
      setDetailsOpen(false);
      setMobileSearchOpen(false);
    }
    lastSelectedToolId = state.selectedTool.id;
    els.emptyState.hidden = true;
    els.toolView.hidden = false;
    renderActiveToolHeader(state.selectedTool, state);
  } else {
    lastSelectedToolId = null;
    setDetailsOpen(false);
    setMobileSearchOpen(false);
    els.toolView.hidden = true;
    els.emptyState.hidden = false;
    updateEmptyState(state);
  }
}

function bindEvents() {
  els.brandLink.addEventListener("click", (e) => {
    e.preventDefault();
    setDetailsOpen(false);
    setMobileSearchOpen(false);
    host.clearSelection();
    closeMobileSidebar();
  });

  const handleSidebarToggle = (trigger) => {
    if (els.sidebar.classList.contains("open")) {
      closeMobileSidebar();
    } else {
      openMobileSidebar(trigger);
    }
  };

  els.sidebarToggle.addEventListener("click", () => handleSidebarToggle(els.sidebarToggle));
  els.toolSidebarBtn.addEventListener("click", () => handleSidebarToggle(els.toolSidebarBtn));

  els.sidebarBackdrop.addEventListener("click", () => closeMobileSidebar());

  els.mobileBackBtn.addEventListener("click", () => {
    setDetailsOpen(false);
    setMobileSearchOpen(false);
    host.clearSelection();
    closeMobileSidebar();
  });

  els.closeToolBtn.addEventListener("click", () => {
    setDetailsOpen(false);
    setMobileSearchOpen(false);
    host.clearSelection();
  });

  if (els.mobileSearchToggle) {
    els.mobileSearchToggle.addEventListener("click", () => {
      const next = els.shell?.dataset.searchOpen !== "true";
      setMobileSearchOpen(next, { focus: next });
    });
  }

  els.toolDetailsToggle.addEventListener("click", () => {
    if (els.sidebar.classList.contains("open")) {
      closeMobileSidebar();
    }
    setDetailsOpen(els.tuiDetailsPanel.hidden);
  });

  els.detailsQuickSwitcher.addEventListener("change", (e) => {
    const targetId = e.target.value;
    setDetailsOpen(false);
    if (targetId && targetId !== host.getState().selectedId) {
      host.selectTool(targetId, { updateHash: true, forceReload: false });
    }
  });

  document.addEventListener("pointerdown", (e) => {
    if (els.tuiDetailsPanel.hidden) return;
    if (els.tuiDetailsPanel.contains(e.target) || els.toolDetailsToggle.contains(e.target)) {
      return;
    }
    setDetailsOpen(false);
  });

  window.addEventListener("blur", () => {
    if (!els.tuiDetailsPanel.hidden && document.activeElement?.tagName === "IFRAME") {
      setDetailsOpen(false);
    }
  });

  els.themeToggle.addEventListener("click", () => {
    host.toggleThemeMode();
  });

  els.searchInput.addEventListener("input", (e) => {
    const query = e.target.value;
    if (
      (window.innerWidth <= 780 || window.innerHeight <= 500) &&
      host.getState().selectedId &&
      !els.sidebar.classList.contains("open")
    ) {
      openMobileSidebar(els.toolSidebarBtn);
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
      } else if (els.shell?.dataset.view === "tool" && els.shell?.dataset.searchOpen === "true") {
        e.preventDefault();
        setMobileSearchOpen(false, { restoreFocus: true });
      } else {
        els.searchInput.blur();
      }
    }
  });

  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      if (!els.tuiDetailsPanel.hidden) {
        e.preventDefault();
        setDetailsOpen(false, { restoreFocus: true });
        return;
      }
      if (els.sidebar.classList.contains("open")) {
        e.preventDefault();
        closeMobileSidebar({ restoreFocus: true });
        return;
      }
    }
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
    host.clearFilters();
  });

  els.reloadToolBtn.addEventListener("click", () => {
    host.reloadTool();
  });
}

bindEvents();
host.bootstrap();
