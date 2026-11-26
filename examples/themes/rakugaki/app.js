const DEFAULT_SITE_TITLE = "ToolStow";

const els = {
  shell: document.querySelector(".rk-shell"),
  brandLink: document.getElementById("brand-link"),
  brandTitle: document.getElementById("brand-title"),
  searchInput: document.getElementById("search-input"),
  mobileSearchToggle: document.getElementById("mobile-search-toggle"),
  themeToggle: document.getElementById("theme-toggle"),
  adminLink: document.getElementById("admin-link"),
  statusBanner: document.getElementById("status-banner"),
  notebookView: document.getElementById("notebook-view"),
  heroTitle: document.getElementById("hero-title"),
  toolCountStamp: document.getElementById("tool-count-stamp"),
  heroDesc: document.getElementById("hero-desc"),
  clearFiltersBtn: document.getElementById("clear-filters-btn"),
  refreshListBtn: document.getElementById("refresh-list-btn"),
  categoryTabs: document.getElementById("category-tabs"),
  tagList: document.getElementById("tag-list"),
  toolGrid: document.getElementById("tool-grid"),
  toolView: document.getElementById("tool-view"),
  backToNotebookBtn: document.getElementById("back-to-notebook-btn"),
  quickSwitcher: document.getElementById("quick-switcher"),
  activeToolIcon: document.getElementById("active-tool-icon"),
  activeToolName: document.getElementById("active-tool-name"),
  activeToolId: document.getElementById("active-tool-id"),
  activeToolVersion: document.getElementById("active-tool-version"),
  activeToolThemeBadge: document.getElementById("active-tool-theme-badge"),
  activeToolDesc: document.getElementById("active-tool-desc"),
  toolDetailsToggle: document.getElementById("tool-details-toggle"),
  toolDetailsPanel: document.getElementById("tool-details-panel"),
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
  activeToolTags: document.getElementById("active-tool-tags"),
  iframeHost: document.getElementById("iframe-host"),
  footerStatus: document.getElementById("footer-status"),
  footerCount: document.getElementById("footer-count"),
};

let statusTimer = null;
let searchToolId = null;
let lastSelectedToolId = null;

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
  els.toolDetailsPanel.hidden = !isOpen;
  els.toolDetailsToggle.setAttribute("aria-expanded", String(isOpen));
  if (!isOpen && restoreFocus && !els.toolDetailsToggle.hidden) {
    els.toolDetailsToggle.focus();
  }
}

const host = window.ToolStowHost.create({
  iframeHost: els.iframeHost,
  defaultMode: "light",
  fallbackAccentLight: "#d9381e",
  fallbackAccentDark: "#fbbf24",
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
    els.detailsToolThemeBadge.hidden = false;
  },
});

function renderHeroDescription(state) {
  const desc = state.site.siteDescription;
  els.heroDesc.replaceChildren();
  if (desc) {
    els.heroDesc.appendChild(document.createTextNode(`${desc} · `));
  }
  els.heroDesc.appendChild(document.createTextNode("点击任意手绘便签打开工具，或按 "));
  const kbd = document.createElement("kbd");
  kbd.className = "rk-inline-kbd";
  kbd.textContent = "⌘K";
  els.heroDesc.appendChild(kbd);
  els.heroDesc.appendChild(document.createTextNode(" 快速翻找。"));
}

function renderCategories(state) {
  const fragment = document.createDocumentFragment();
  const makeTab = (label, value, count) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.setAttribute("role", "tab");
    const isActive = state.filters.category === value;
    btn.className = "rk-cat-tab" + (isActive ? " active" : "");
    btn.setAttribute("aria-selected", String(isActive));

    const nameSpan = document.createElement("span");
    nameSpan.textContent = label;

    const countSpan = document.createElement("span");
    countSpan.className = "rk-cat-count";
    countSpan.textContent = String(count);

    btn.append(nameSpan, countSpan);
    btn.addEventListener("click", () => {
      const next = value === "" ? "" : state.filters.category === value ? "" : value;
      host.setFilters({ category: next });
    });
    return btn;
  };

  fragment.appendChild(makeTab("全部笔记", "", state.allTools.length));
  for (const cat of state.categories) {
    fragment.appendChild(makeTab(cat.name, cat.name, cat.count));
  }

  els.categoryTabs.replaceChildren(fragment);
}

function renderTags(state) {
  const fragment = document.createDocumentFragment();

  if (state.tags.length === 0) {
    const empty = document.createElement("span");
    empty.className = "rk-empty-inline";
    empty.textContent = "暂无标签";
    fragment.appendChild(empty);
  } else {
    for (const tag of state.tags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "rk-tag-pill" + (state.filters.tag === tag ? " active" : "");
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

function populateSelectOptions(selectEl, state) {
  const fragment = document.createDocumentFragment();
  for (const tool of state.allTools) {
    const opt = document.createElement("option");
    opt.value = tool.id;
    opt.textContent = tool.name;
    opt.selected = tool.id === state.selectedId;
    fragment.appendChild(opt);
  }
  selectEl.replaceChildren(fragment);
}

function renderQuickSwitcher(state) {
  const hasMultiple = state.allTools.length > 1;
  populateSelectOptions(els.quickSwitcher, state);
  populateSelectOptions(els.detailsQuickSwitcher, state);
  els.quickSwitcher.hidden = !hasMultiple;
  els.detailsSwitcherRow.hidden = !hasMultiple;
}

function renderToolWall(state) {
  const shown = state.filteredTools.length;
  const total = state.allTools.length;
  els.toolCountStamp.textContent = host.hasActiveFilters()
    ? `筛出 ${shown} / ${total} 个`
    : `${total} 个工具`;

  const fragment = document.createDocumentFragment();

  if (shown === 0) {
    const empty = document.createElement("div");
    empty.className = "rk-empty-card";
    empty.textContent =
      total === 0
        ? "笔记本目前还是空白页，请管理员在后台上传发布工具。"
        : "没有翻到匹配的便签，试试换个关键词或清空筛选。";
    fragment.appendChild(empty);
  } else {
    for (const tool of state.filteredTools) {
      const card = document.createElement("button");
      card.type = "button";
      card.className = "rk-note-card";

      const head = document.createElement("div");
      head.className = "rk-card-head";

      const titleWrap = document.createElement("div");
      titleWrap.className = "rk-card-title-wrap";

      const iconWrap = document.createElement("span");
      iconWrap.className = "rk-tool-icon";
      window.ToolStowHost.renderIcon(iconWrap, tool, "✎");

      const titleEl = document.createElement("span");
      titleEl.className = "rk-card-title";
      titleEl.textContent = tool.name;

      titleWrap.append(iconWrap, titleEl);

      const verEl = document.createElement("span");
      verEl.className = "rk-mono-pill";
      verEl.textContent = tool.version ? `v${tool.version}` : "note";

      head.append(titleWrap, verEl);

      const descEl = document.createElement("p");
      descEl.className = "rk-card-desc";
      descEl.textContent = tool.description || `ID: ${tool.id}`;

      card.append(head, descEl);

      if (Array.isArray(tool.tags) && tool.tags.length > 0) {
        const tagWrap = document.createElement("div");
        tagWrap.className = "rk-card-tags";
        for (const tag of tool.tags.slice(0, 3)) {
          const pill = document.createElement("span");
          pill.className = "rk-card-tag";
          pill.textContent = `#${tag}`;
          tagWrap.appendChild(pill);
        }
        card.appendChild(tagWrap);
      }

      const foot = document.createElement("div");
      foot.className = "rk-card-foot";

      const catEl = document.createElement("span");
      catEl.className = "rk-card-cat";
      catEl.textContent = `${tool.category || "未分类"} · ${tool.id}`;

      const openEl = document.createElement("span");
      openEl.className = "rk-card-open";
      openEl.textContent = "翻开 →";

      foot.append(catEl, openEl);
      card.appendChild(foot);

      card.addEventListener("click", () => {
        openTool(tool.id);
      });

      fragment.appendChild(card);
    }
  }

  els.toolGrid.replaceChildren(fragment);
}

function buildTagReturnFragment(tags) {
  const tagFrag = document.createDocumentFragment();
  if (Array.isArray(tags) && tags.length > 0) {
    const label = document.createElement("span");
    label.className = "rk-tags-label";
    label.textContent = "按标签返回目录";
    tagFrag.appendChild(label);
    for (const tag of tags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "rk-tag-pill";
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

function renderActiveToolHeader(tool, state) {
  if (!tool) return;
  window.ToolStowHost.renderIcon(els.activeToolIcon, tool, "✎");
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
  els.detailsToolTags.replaceChildren(buildTagReturnFragment(tool.tags));
}

function openTool(id) {
  setDetailsOpen(false);
  setMobileSearchOpen(false);
  searchToolId = null;
  host.selectTool(id);
}

function renderState(state) {
  if (searchToolId !== state.selectedId) searchToolId = null;
  const showingSearchResults = Boolean(searchToolId);
  const siteTitle = state.site.siteTitle || DEFAULT_SITE_TITLE;
  els.brandTitle.textContent = siteTitle;
  els.heroTitle.textContent = siteTitle;
  els.adminLink.hidden = state.site.isAdmin !== true;
  els.themeToggle.textContent = state.theme.mode === "dark" ? "☀ 网格纸" : "☾ 黑板";
  renderHeroDescription(state);

  if (els.searchInput.value !== state.filters.q) {
    els.searchInput.value = state.filters.q;
  }

  renderCategories(state);
  renderTags(state);
  renderToolWall(state);
  renderQuickSwitcher(state);

  els.footerCount.textContent = `${state.filteredTools.length} / ${state.allTools.length}`;
  els.footerStatus.textContent = state.selectedTool
    ? `正在使用：${state.selectedTool.id}`
    : "笔记本就绪";

  const showingTool = Boolean(state.selectedTool && !showingSearchResults);
  if (els.shell) {
    els.shell.dataset.view = showingTool ? "tool" : "directory";
  }
  els.notebookView.hidden = showingTool;
  els.toolView.hidden = !showingTool;
  els.clearFiltersBtn.textContent = showingSearchResults ? "返回当前工具" : "清空筛选";
  if (showingTool) {
    if (lastSelectedToolId !== state.selectedTool.id) {
      setDetailsOpen(false);
      setMobileSearchOpen(false);
    }
    lastSelectedToolId = state.selectedTool.id;
    renderActiveToolHeader(state.selectedTool, state);
  } else {
    lastSelectedToolId = null;
    setDetailsOpen(false);
  }
}

function bindEvents() {
  els.brandLink.addEventListener("click", (e) => {
    e.preventDefault();
    setDetailsOpen(false);
    setMobileSearchOpen(false);
    host.clearSelection();
  });

  els.backToNotebookBtn.addEventListener("click", () => {
    setDetailsOpen(false);
    setMobileSearchOpen(false);
    host.clearSelection();
  });

  const handleSwitcherChange = (e) => {
    const targetId = e.target.value;
    if (targetId && targetId !== host.getState().selectedId) {
      openTool(targetId);
    } else {
      setDetailsOpen(false);
    }
  };
  els.quickSwitcher.addEventListener("change", handleSwitcherChange);
  els.detailsQuickSwitcher.addEventListener("change", handleSwitcherChange);

  if (els.mobileSearchToggle) {
    els.mobileSearchToggle.addEventListener("click", () => {
      const next = els.shell?.dataset.searchOpen !== "true";
      setMobileSearchOpen(next, { focus: next });
    });
  }

  els.toolDetailsToggle.addEventListener("click", () => {
    setDetailsOpen(els.toolDetailsPanel.hidden);
  });

  document.addEventListener("pointerdown", (e) => {
    if (els.toolDetailsPanel.hidden) return;
    if (els.toolDetailsPanel.contains(e.target) || els.toolDetailsToggle.contains(e.target)) {
      return;
    }
    setDetailsOpen(false);
  });

  window.addEventListener("blur", () => {
    if (!els.toolDetailsPanel.hidden && document.activeElement?.tagName === "IFRAME") {
      setDetailsOpen(false);
    }
  });

  els.themeToggle.addEventListener("click", () => {
    host.toggleThemeMode();
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
      } else if (els.shell?.dataset.view === "tool" && els.shell?.dataset.searchOpen === "true") {
        e.preventDefault();
        setMobileSearchOpen(false, { restoreFocus: true });
      } else {
        els.searchInput.blur();
      }
    }
  });

  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !els.toolDetailsPanel.hidden) {
      e.preventDefault();
      setDetailsOpen(false, { restoreFocus: true });
      return;
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

bindEvents();
host.bootstrap();
