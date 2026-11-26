const DEFAULT_SITE_TITLE = "ToolStow";

const els = {
  brandLink: document.getElementById("brand-link"),
  brandTitle: document.getElementById("brand-title"),
  searchInput: document.getElementById("search-input"),
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
  reloadToolBtn: document.getElementById("reload-tool-btn"),
  standaloneLink: document.getElementById("standalone-link"),
  activeToolTags: document.getElementById("active-tool-tags"),
  iframeHost: document.getElementById("iframe-host"),
  footerStatus: document.getElementById("footer-status"),
  footerCount: document.getElementById("footer-count"),
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

function renderQuickSwitcher(state) {
  const fragment = document.createDocumentFragment();
  for (const tool of state.allTools) {
    const opt = document.createElement("option");
    opt.value = tool.id;
    opt.textContent = tool.name;
    opt.selected = tool.id === state.selectedId;
    fragment.appendChild(opt);
  }
  els.quickSwitcher.replaceChildren(fragment);
  els.quickSwitcher.hidden = state.allTools.length <= 1;
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
        host.selectTool(tool.id, { updateHash: true, forceReload: false });
      });

      fragment.appendChild(card);
    }
  }

  els.toolGrid.replaceChildren(fragment);
}

function renderActiveToolHeader(tool, state) {
  if (!tool) return;
  window.ToolStowHost.renderIcon(els.activeToolIcon, tool, "✎");
  els.activeToolName.textContent = tool.name;
  els.activeToolId.textContent = tool.id;
  els.activeToolVersion.textContent = tool.version ? `v${tool.version}` : "";
  els.activeToolVersion.hidden = !tool.version;
  els.activeToolThemeBadge.hidden = !(tool.themeAware === true || state.activeIframeReady);
  els.activeToolDesc.textContent = tool.description || "";
  els.activeToolDesc.hidden = !tool.description;

  const standaloneUrl = host.getStandaloneUrl(tool);
  if (standaloneUrl) {
    els.standaloneLink.href = standaloneUrl;
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
      btn.className = "rk-tag-pill" + (state.filters.tag === tag ? " active" : "");
      btn.textContent = `#${tag}`;
      btn.addEventListener("click", () => {
        const nextTag = state.filters.tag === tag ? "" : tag;
        host.clearSelection();
        host.setFilters({ tag: nextTag });
      });
      tagFrag.appendChild(btn);
    }
  }
  els.activeToolTags.replaceChildren(tagFrag);
}

function renderState(state) {
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

  if (state.selectedTool) {
    els.notebookView.hidden = true;
    els.toolView.hidden = false;
    renderActiveToolHeader(state.selectedTool, state);
  } else {
    els.toolView.hidden = true;
    els.notebookView.hidden = false;
  }
}

function bindEvents() {
  els.brandLink.addEventListener("click", (e) => {
    e.preventDefault();
    host.clearSelection();
  });

  els.backToNotebookBtn.addEventListener("click", () => {
    host.clearSelection();
  });

  els.quickSwitcher.addEventListener("change", (e) => {
    const targetId = e.target.value;
    if (targetId && targetId !== host.getState().selectedId) {
      host.selectTool(targetId, { updateHash: true, forceReload: false });
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
