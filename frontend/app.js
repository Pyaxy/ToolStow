// ToolStow default public shell.
// All data/theme/iframe plumbing lives in window.ToolStowHost (/ui/v1/host.js),
// which talks to /api/me and /api/tools. This file only renders DOM.

const DEFAULT_SITE_TITLE = "ToolStow";
const MAX_COLLAPSED_TAGS = 8;
const UNCATEGORIZED = "未分类";

const els = {
  statusBanner: document.getElementById("status-banner"),
  directoryView: document.getElementById("directory-view"),
  brandLink: document.getElementById("brand-link"),
  brandTitle: document.getElementById("brand-title"),
  searchInput: document.getElementById("search-input"),
  themeToggle: document.getElementById("theme-toggle"),
  adminLink: document.getElementById("admin-link"),
  emptyState: document.getElementById("empty-state"),
  emptyTitle: document.getElementById("empty-title"),
  emptyDesc: document.getElementById("empty-desc"),
  emptyCount: document.getElementById("empty-count"),
  categoryList: document.getElementById("category-list"),
  tagList: document.getElementById("tag-list"),
  clearFiltersBtn: document.getElementById("clear-filters-btn"),
  refreshListBtn: document.getElementById("refresh-list-btn"),
  emptyGrid: document.getElementById("empty-grid"),
  toolView: document.getElementById("tool-view"),
  backBtn: document.getElementById("back-btn"),
  quickSwitcher: document.getElementById("quick-switcher"),
  activeToolIcon: document.getElementById("active-tool-icon"),
  activeToolName: document.getElementById("active-tool-name"),
  infoToggle: document.getElementById("info-toggle"),
  infoPopover: document.getElementById("info-popover"),
  activeToolId: document.getElementById("active-tool-id"),
  activeToolVersion: document.getElementById("active-tool-version"),
  infoVersionRow: document.getElementById("info-version-row"),
  infoThemeRow: document.getElementById("info-theme-row"),
  activeToolDesc: document.getElementById("active-tool-desc"),
  activeToolTags: document.getElementById("active-tool-tags"),
  reloadToolBtn: document.getElementById("reload-tool-btn"),
  standaloneLink: document.getElementById("standalone-link"),
  toolThemeToggle: document.getElementById("tool-theme-toggle"),
  iframeHost: document.getElementById("iframe-host"),
  cmdPaletteTrigger: document.getElementById("cmd-palette-trigger"),
  cmdPalette: document.getElementById("cmd-palette"),
  cmdPaletteBackdrop: document.getElementById("cmd-palette-backdrop"),
  cmdPaletteInput: document.getElementById("cmd-palette-input"),
  cmdPaletteList: document.getElementById("cmd-palette-list"),
};

const ui = {
  statusTimer: null,
  statusFromState: false,
  tagsExpanded: false,
  infoOpen: false,
  lastSelectedId: null,
  switcherSignature: "",
  paletteOpen: false,
  paletteQuery: "",
  paletteActiveIndex: 0,
  paletteResults: [],
  paletteReturnFocus: null,
};

let host = null;

/* ───────────── helpers ───────────── */

function normalizeKey(value) {
  return String(value || "").trim().toLowerCase();
}

function showStatus(message, kind = "info", autoHideMs = 4500) {
  if (ui.statusTimer) {
    window.clearTimeout(ui.statusTimer);
    ui.statusTimer = null;
  }
  ui.statusFromState = false;
  els.statusBanner.textContent = message;
  els.statusBanner.dataset.kind = kind;
  els.statusBanner.hidden = false;
  if (autoHideMs > 0) {
    ui.statusTimer = window.setTimeout(() => {
      els.statusBanner.hidden = true;
    }, autoHideMs);
  }
}

function syncStateError(error) {
  if (error) {
    const message = typeof error === "string" ? error : error.message || "无法连接到 ToolStow 服务";
    showStatus(message, "error", 0);
    ui.statusFromState = true;
  } else if (ui.statusFromState) {
    ui.statusFromState = false;
    els.statusBanner.hidden = true;
  }
}

function makeButton(className, text, onClick) {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = className;
  btn.textContent = text;
  btn.addEventListener("click", onClick);
  return btn;
}

/* ───────────── directory view ───────────── */

function renderCategories(s) {
  const active = normalizeKey(s.filters.category);
  const fragment = document.createDocumentFragment();

  const makePill = (label, count, isActive, onClick) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "pill" + (isActive ? " active" : "");
    btn.setAttribute("aria-pressed", isActive ? "true" : "false");
    const text = document.createElement("span");
    text.textContent = label;
    const num = document.createElement("span");
    num.className = "pill-count";
    num.textContent = String(count);
    btn.append(text, num);
    btn.addEventListener("click", onClick);
    return btn;
  };

  fragment.appendChild(
    makePill("全部", s.allTools.length, active === "", () => host.setFilters({ category: "" }))
  );

  for (const cat of s.categories) {
    const name = cat && typeof cat === "object" ? cat.name : cat;
    const count = cat && typeof cat === "object" ? cat.count : "";
    const isActive = active !== "" && normalizeKey(name) === active;
    fragment.appendChild(
      makePill(name || UNCATEGORIZED, count, isActive, () =>
        host.setFilters({ category: isActive ? "" : name })
      )
    );
  }

  els.categoryList.replaceChildren(fragment);
}

function toggleTagFilter(tag, activeTag) {
  host.setFilters({ tag: normalizeKey(tag) === activeTag ? "" : tag });
}

function renderTags(s) {
  const activeTag = normalizeKey(s.filters.tag);
  const tags = Array.isArray(s.tags) ? s.tags : [];
  const fragment = document.createDocumentFragment();

  let visible = tags;
  let hiddenCount = 0;
  if (!ui.tagsExpanded && tags.length > MAX_COLLAPSED_TAGS) {
    visible = tags.slice(0, MAX_COLLAPSED_TAGS);
    const activeFull = tags.find((t) => normalizeKey(t) === activeTag);
    if (activeFull && !visible.includes(activeFull)) visible.push(activeFull);
    hiddenCount = tags.length - visible.length;
  }

  for (const tag of visible) {
    const isActive = activeTag !== "" && normalizeKey(tag) === activeTag;
    const btn = makeButton("tag-chip" + (isActive ? " active" : ""), `#${tag}`, () =>
      toggleTagFilter(tag, activeTag)
    );
    btn.setAttribute("aria-pressed", isActive ? "true" : "false");
    fragment.appendChild(btn);
  }

  if (tags.length > MAX_COLLAPSED_TAGS) {
    const label = ui.tagsExpanded ? "收起" : `+${hiddenCount} 更多`;
    const more = makeButton("tag-chip tag-chip-more", label, () => {
      ui.tagsExpanded = !ui.tagsExpanded;
      renderTags(host.getState());
    });
    more.setAttribute("aria-expanded", ui.tagsExpanded ? "true" : "false");
    fragment.appendChild(more);
  }

  els.tagList.replaceChildren(fragment);
}

function openTool(id) {
  host.selectTool(id, { updateHash: true, forceReload: false });
}

function renderGrid(s) {
  const fragment = document.createDocumentFragment();
  const list = s.filteredTools;

  if (list.length === 0) {
    const box = document.createElement("div");
    box.className = "tool-card-empty";
    if (s.loading) {
      box.textContent = "正在加载工具列表...";
    } else if (s.allTools.length === 0) {
      box.textContent = "暂无已发布的工具。管理员可在后台收录工具包或单文件 HTML。";
    } else {
      box.textContent = "当前筛选条件下没有匹配的工具。";
    }
    fragment.appendChild(box);
  } else {
    for (const tool of list) {
      const card = document.createElement("button");
      card.type = "button";
      card.className = "tool-card";

      const head = document.createElement("div");
      head.className = "tool-card-head";

      const iconWrap = document.createElement("span");
      iconWrap.className = "tool-icon";
      iconWrap.setAttribute("aria-hidden", "true");
      window.ToolStowHost.renderIcon(iconWrap, tool);

      const nameEl = document.createElement("span");
      nameEl.className = "tool-card-name";
      nameEl.textContent = tool.name || tool.id;

      head.append(iconWrap, nameEl);

      const descEl = document.createElement("p");
      descEl.className = "tool-card-desc";
      descEl.textContent = tool.description || "点击打开此微工具";

      const catEl = document.createElement("span");
      catEl.className = "tool-card-cat";
      catEl.textContent = tool.category || UNCATEGORIZED;

      card.append(head, descEl, catEl);
      card.addEventListener("click", () => openTool(tool.id));
      fragment.appendChild(card);
    }
  }

  els.emptyGrid.replaceChildren(fragment);
}

function renderLandingHeader(s) {
  const title = s.site.siteTitle || DEFAULT_SITE_TITLE;
  els.emptyTitle.textContent = title;

  const total = s.filteredTools.length;
  els.emptyCount.textContent = `${total} ${total === 1 ? "tool" : "tools"}`;

  const desc = s.site.siteDescription;
  els.emptyDesc.replaceChildren();
  if (desc) {
    els.emptyDesc.appendChild(document.createTextNode(`${desc} · `));
  }
  els.emptyDesc.appendChild(document.createTextNode("选择任意工具即可运行，或按 "));
  const kbd = document.createElement("kbd");
  kbd.className = "inline-kbd";
  kbd.textContent = "⌘K";
  els.emptyDesc.appendChild(kbd);
  els.emptyDesc.appendChild(document.createTextNode(" 快速搜索。"));
}

function renderDirectory(s) {
  renderLandingHeader(s);
  renderCategories(s);
  renderTags(s);
  renderGrid(s);
  els.clearFiltersBtn.hidden = !host.hasActiveFilters();

  if (document.activeElement !== els.searchInput && els.searchInput.value !== s.filters.q) {
    els.searchInput.value = s.filters.q;
  }
}

/* ───────────── tool workspace ───────────── */

function setInfoOpen(open, { restoreFocus = false } = {}) {
  ui.infoOpen = open;
  els.infoPopover.hidden = !open;
  els.infoToggle.setAttribute("aria-expanded", open ? "true" : "false");
  if (!open && restoreFocus) els.infoToggle.focus();
}

function renderQuickSwitcher(s) {
  const signature = `${s.selectedId}|${s.allTools.map((t) => `${t.id}:${t.name}`).join(",")}`;
  if (signature === ui.switcherSignature) return;
  ui.switcherSignature = signature;

  const fragment = document.createDocumentFragment();
  for (const tool of s.allTools) {
    const opt = document.createElement("option");
    opt.value = tool.id;
    opt.textContent = tool.name || tool.id;
    fragment.appendChild(opt);
  }
  els.quickSwitcher.replaceChildren(fragment);
  els.quickSwitcher.value = s.selectedId || "";
}

function renderInfoPopover(tool, s) {
  els.activeToolId.textContent = tool.id;

  els.activeToolVersion.textContent = tool.version ? `v${tool.version}` : "";
  els.infoVersionRow.hidden = !tool.version;

  els.infoThemeRow.hidden = !(tool.themeAware === true || s.activeIframeReady === true);

  els.activeToolDesc.textContent = tool.description || "";

  const activeTag = normalizeKey(s.filters.tag);
  const fragment = document.createDocumentFragment();
  if (Array.isArray(tool.tags)) {
    for (const tag of tool.tags) {
      if (!tag) continue;
      fragment.appendChild(
        makeButton("tag-chip" + (normalizeKey(tag) === activeTag ? " active" : ""), `#${tag}`, () => {
          // Jump back to the directory filtered by this tag.
          setInfoOpen(false);
          host.setFilters({ tag });
          host.clearSelection({ updateHash: true });
        })
      );
    }
  }
  els.activeToolTags.replaceChildren(fragment);
}

function renderWorkspace(tool, s) {
  window.ToolStowHost.renderIcon(els.activeToolIcon, tool);
  els.activeToolName.textContent = tool.name || tool.id;
  renderQuickSwitcher(s);
  els.quickSwitcher.value = tool.id;
  renderInfoPopover(tool, s);

  const standaloneUrl = host.getStandaloneUrl(tool);
  if (standaloneUrl) {
    els.standaloneLink.href = standaloneUrl;
    els.standaloneLink.hidden = false;
  } else {
    els.standaloneLink.removeAttribute("href");
    els.standaloneLink.hidden = true;
  }
}

/* ───────────── command palette (active tool search) ───────────── */

function filterPaletteTools(allTools, query) {
  const q = normalizeKey(query);
  if (!q) return allTools;
  return allTools.filter((tool) => {
    if (!tool || typeof tool !== "object") return false;
    const tags = Array.isArray(tool.tags) ? tool.tags : [];
    return (
      normalizeKey(tool.name).includes(q) ||
      normalizeKey(tool.description).includes(q) ||
      normalizeKey(tool.id).includes(q) ||
      normalizeKey(tool.category).includes(q) ||
      tags.some((t) => normalizeKey(t).includes(q))
    );
  });
}

function updatePaletteActiveRow() {
  const options = els.cmdPaletteList.querySelectorAll(".cmd-palette-item");
  let activeOption = null;
  options.forEach((opt, index) => {
    const isActive = index === ui.paletteActiveIndex;
    opt.classList.toggle("is-active", isActive);
    opt.setAttribute("aria-selected", isActive ? "true" : "false");
    if (isActive) activeOption = opt;
  });

  if (activeOption) {
    els.cmdPaletteInput.setAttribute("aria-activedescendant", activeOption.id);
    activeOption.scrollIntoView({ block: "nearest" });
  } else {
    els.cmdPaletteInput.removeAttribute("aria-activedescendant");
  }
}

function selectPaletteTool(tool) {
  if (!tool) return;
  const currentId = host.getState().selectedId;
  if (tool.id === currentId) {
    closePalette({ restoreFocus: true });
    return;
  }
  closePalette({ restoreFocus: false });
  openTool(tool.id);
}

function renderPaletteList(s) {
  const results = filterPaletteTools(s.allTools, ui.paletteQuery);
  ui.paletteResults = results;

  if (results.length === 0) {
    ui.paletteActiveIndex = -1;
    const empty = document.createElement("div");
    empty.className = "cmd-palette-empty";
    empty.textContent = s.allTools.length === 0 ? "暂无可用工具。" : "没有匹配的工具。";
    els.cmdPaletteList.replaceChildren(empty);
    els.cmdPaletteInput.removeAttribute("aria-activedescendant");
    return;
  }

  if (ui.paletteActiveIndex < 0 || ui.paletteActiveIndex >= results.length) {
    ui.paletteActiveIndex = 0;
  }

  const fragment = document.createDocumentFragment();
  results.forEach((tool, index) => {
    const isCurrent = tool.id === s.selectedId;
    const isActive = index === ui.paletteActiveIndex;

    const item = document.createElement("button");
    item.type = "button";
    item.id = `cmd-palette-opt-${index}`;
    item.className = "cmd-palette-item";
    if (isCurrent) item.classList.add("is-current");
    if (isActive) item.classList.add("is-active");
    item.setAttribute("role", "option");
    item.setAttribute("aria-selected", isActive ? "true" : "false");

    const iconWrap = document.createElement("span");
    iconWrap.className = "tool-icon cmd-palette-icon";
    iconWrap.setAttribute("aria-hidden", "true");
    window.ToolStowHost.renderIcon(iconWrap, tool);

    const body = document.createElement("span");
    body.className = "cmd-palette-item-body";

    const titleRow = document.createElement("span");
    titleRow.className = "cmd-palette-item-title-row";

    const nameEl = document.createElement("span");
    nameEl.className = "cmd-palette-item-name";
    nameEl.textContent = tool.name || tool.id;
    titleRow.appendChild(nameEl);

    if (isCurrent) {
      const currentBadge = document.createElement("span");
      currentBadge.className = "mono-badge mono-badge-accent cmd-palette-current-badge";
      currentBadge.textContent = "当前";
      titleRow.appendChild(currentBadge);
    }

    const metaEl = document.createElement("span");
    metaEl.className = "cmd-palette-item-meta";
    const categoryText = tool.category || UNCATEGORIZED;
    metaEl.textContent = tool.description ? `${categoryText} · ${tool.description}` : categoryText;

    body.append(titleRow, metaEl);

    const idEl = document.createElement("code");
    idEl.className = "cmd-palette-item-id";
    idEl.textContent = tool.id;

    item.append(iconWrap, body, idEl);
    item.addEventListener("mouseenter", () => {
      if (ui.paletteActiveIndex !== index) {
        ui.paletteActiveIndex = index;
        updatePaletteActiveRow();
      }
    });
    item.addEventListener("click", () => selectPaletteTool(tool));

    fragment.appendChild(item);
  });

  els.cmdPaletteList.replaceChildren(fragment);
  updatePaletteActiveRow();
}

function openPalette() {
  const s = host.getState();
  if (!s.selectedTool) return;

  if (ui.paletteOpen) {
    els.cmdPaletteInput.focus();
    els.cmdPaletteInput.select();
    return;
  }

  setInfoOpen(false);
  ui.paletteReturnFocus =
    document.activeElement instanceof HTMLElement ? document.activeElement : null;
  ui.paletteOpen = true;
  ui.paletteQuery = "";
  ui.paletteActiveIndex = 0;

  els.cmdPaletteInput.value = "";
  els.cmdPalette.hidden = false;
  els.cmdPaletteTrigger.setAttribute("aria-expanded", "true");
  renderPaletteList(s);
  els.cmdPaletteInput.focus();
}

function closePalette({ restoreFocus = true } = {}) {
  if (!ui.paletteOpen) return;
  ui.paletteOpen = false;
  els.cmdPalette.hidden = true;
  els.cmdPaletteTrigger.setAttribute("aria-expanded", "false");

  const target = ui.paletteReturnFocus;
  ui.paletteReturnFocus = null;
  if (
    restoreFocus &&
    target &&
    target !== document.body &&
    document.contains(target) &&
    typeof target.focus === "function"
  ) {
    target.focus({ preventScroll: true });
  } else if (restoreFocus && document.activeElement === els.cmdPaletteInput) {
    els.cmdPaletteInput.blur();
  }
}

/* ───────────── master render ───────────── */

function render(s) {
  const siteTitle = s.site.siteTitle || DEFAULT_SITE_TITLE;
  els.brandTitle.textContent = siteTitle;
  els.adminLink.hidden = s.site.isAdmin !== true;

  const modeLabel = s.theme.mode === "dark" ? "亮色" : "暗色";
  els.themeToggle.textContent = modeLabel;
  els.toolThemeToggle.textContent = modeLabel;

  syncStateError(s.error);

  const tool = s.selectedTool || null;
  if (tool) {
    if (ui.lastSelectedId !== tool.id) setInfoOpen(false);
    ui.lastSelectedId = tool.id;
    els.directoryView.hidden = true;
    els.toolView.hidden = false;
    renderWorkspace(tool, s);
    if (ui.paletteOpen) renderPaletteList(s);
  } else {
    if (ui.lastSelectedId !== null) {
      ui.lastSelectedId = null;
      ui.switcherSignature = "";
    }
    setInfoOpen(false);
    closePalette({ restoreFocus: false });
    els.toolView.hidden = true;
    els.directoryView.hidden = false;
    renderDirectory(s);
  }
}

/* ───────────── events ───────────── */

function focusSearch() {
  els.searchInput.focus();
  els.searchInput.select();
}

function bindEvents() {
  els.brandLink.addEventListener("click", (e) => {
    e.preventDefault();
    host.clearSelection({ updateHash: true });
  });

  els.backBtn.addEventListener("click", () => {
    host.clearSelection({ updateHash: true });
  });

  els.themeToggle.addEventListener("click", () => host.toggleThemeMode());
  els.toolThemeToggle.addEventListener("click", () => host.toggleThemeMode());

  els.searchInput.addEventListener("input", (e) => {
    host.setFilters({ q: e.target.value });
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
      if (host.getState().selectedTool) {
        if (ui.paletteOpen) {
          closePalette({ restoreFocus: true });
        } else {
          openPalette();
        }
      } else {
        focusSearch();
      }
      return;
    }
    if (e.key === "Escape") {
      if (ui.paletteOpen) {
        e.preventDefault();
        closePalette({ restoreFocus: true });
        return;
      }
      if (ui.infoOpen) {
        setInfoOpen(false, { restoreFocus: true });
      }
    }
  });

  els.cmdPaletteTrigger.addEventListener("click", () => {
    if (ui.paletteOpen) {
      closePalette({ restoreFocus: true });
    } else {
      openPalette();
    }
  });

  els.cmdPaletteBackdrop.addEventListener("click", () => {
    closePalette({ restoreFocus: true });
  });

  els.cmdPaletteInput.addEventListener("input", (e) => {
    ui.paletteQuery = e.target.value;
    ui.paletteActiveIndex = 0;
    renderPaletteList(host.getState());
  });

  els.cmdPalette.addEventListener("keydown", (e) => {
    if (e.isComposing || e.keyCode === 229) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closePalette({ restoreFocus: true });
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (ui.paletteResults.length > 0) {
        ui.paletteActiveIndex = (ui.paletteActiveIndex + 1) % ui.paletteResults.length;
        updatePaletteActiveRow();
      }
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (ui.paletteResults.length > 0) {
        ui.paletteActiveIndex =
          (ui.paletteActiveIndex - 1 + ui.paletteResults.length) % ui.paletteResults.length;
        updatePaletteActiveRow();
      }
      return;
    }
    if (e.key === "Enter" && document.activeElement === els.cmdPaletteInput) {
      e.preventDefault();
      const chosen = ui.paletteResults[ui.paletteActiveIndex];
      if (chosen) selectPaletteTool(chosen);
      return;
    }
    if (e.key === "Tab") {
      const focusables = [
        els.cmdPaletteInput,
        ...Array.from(els.cmdPaletteList.querySelectorAll(".cmd-palette-item")),
      ];
      if (focusables.length === 0) return;
      const idx = focusables.indexOf(document.activeElement);
      const nextIdx = e.shiftKey
        ? (idx <= 0 ? focusables.length - 1 : idx - 1)
        : (idx + 1) % focusables.length;
      e.preventDefault();
      focusables[nextIdx].focus();
    }
  });

  els.refreshListBtn.addEventListener("click", () => {
    host.refresh({ keepSelection: true });
  });

  els.clearFiltersBtn.addEventListener("click", () => {
    els.searchInput.value = "";
    host.clearFilters();
  });

  els.quickSwitcher.addEventListener("change", () => {
    if (els.quickSwitcher.value) openTool(els.quickSwitcher.value);
  });

  els.infoToggle.addEventListener("click", () => setInfoOpen(!ui.infoOpen));

  document.addEventListener("click", (e) => {
    if (!ui.infoOpen) return;
    const target = e.target;
    if (els.infoPopover.contains(target) || els.infoToggle.contains(target)) return;
    setInfoOpen(false);
  });

  els.reloadToolBtn.addEventListener("click", () => host.reloadTool());
}

async function bootstrap() {
  if (!window.ToolStowHost || typeof window.ToolStowHost.create !== "function") {
    showStatus("无法加载 ToolStow Host SDK (/ui/v1/host.js)", "error", 0);
    return;
  }

  host = window.ToolStowHost.create({
    iframeHost: els.iframeHost,
    defaultMode: "auto",
    onError: (message) => showStatus(message || "无法连接到 ToolStow 服务", "error", 6000),
  });

  bindEvents();
  host.subscribe(render);
  await host.bootstrap();
  render(host.getState());
}

bootstrap();
