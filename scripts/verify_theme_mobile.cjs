#!/usr/bin/env node
"use strict";

/**
 * Headless mobile & landscape browser regression verifier for ToolStow themes.
 *
 * Zero repo dependencies: loads `playwright-core` via NODE_PATH (e.g.
 * `NODE_PATH=/tmp/pw/node_modules node scripts/verify_theme_mobile.cjs`).
 *
 * Verifies every theme across mobile portrait (`320x568`, `393x641`) and mobile
 * landscape (`844x390`) viewports with a worst-case tool fixture (long Chinese
 * title/description + 9 tags + logged-in `/admin` link) and `.ts-workbench`
 * inner plugin document.
 */

const fs = require("node:fs");
const http = require("node:http");
const net = require("node:net");
const os = require("node:os");
const path = require("node:path");
const { spawn } = require("node:child_process");

const REPO_ROOT = path.resolve(__dirname, "..");
const EXAMPLES_THEMES_DIR = path.join(REPO_ROOT, "examples", "themes");
const ADMIN_KEY = "ci-test-admin-key";

const VIEWPORTS = [
  { name: "320x568", width: 320, height: 568, kind: "portrait", minIframeHeightPx: Math.ceil(568 * 0.45) },
  { name: "393x641", width: 393, height: 641, kind: "portrait", minIframeHeightPx: Math.ceil(641 * 0.45) },
  { name: "844x390", width: 844, height: 390, kind: "landscape", minIframeHeightPx: 140 },
];

const STRESS_TOOL_ID = "vps-scripts-stress";
const STRESS_TOOL_NAME = "VPS 常用脚本与运维命令速查工作台";
const STRESS_TOOL_DESC =
  "聚合服务器初始化配置、流媒体与 IP 解锁检测、三网回程路由追踪、磁盘与网络基准压测、BBR 内核调优及 Docker 维护常用脚本，支持关键词筛选与一键复制。";
const STRESS_TOOL_TAGS = [
  "vps",
  "linux",
  "scripts",
  "streaming",
  "ip-check",
  "benchmark",
  "dd",
  "bbr",
  "docker",
];

const CHROME_CANDIDATES = [
  process.env.CHROME_BIN,
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
].filter(Boolean);

function printUsage() {
  console.log(
    [
      "Usage: NODE_PATH=/tmp/pw/node_modules node scripts/verify_theme_mobile.cjs [options]",
      "",
      "Options:",
      "  --binary <path>           Path to toolstow binary (default: target/debug/toolstow)",
      "  --themes <csv>            Comma-separated theme shorts (default: default + examples/themes/*)",
      "  --screenshots-dir <dir>   Optional directory to save per-theme/viewport screenshots",
      "  --chrome <path>           Optional Chrome/Chromium executable path override",
      "  -h, --help                Show this help message",
    ].join("\n")
  );
}

function parseArgs(argv) {
  const opts = {
    binary: path.join(REPO_ROOT, "target", "debug", "toolstow"),
    themes: null,
    screenshotsDir: null,
    chrome: null,
    help: false,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "-h" || arg === "--help") {
      opts.help = true;
    } else if (arg === "--binary") {
      opts.binary = path.resolve(requireArgValue(argv, ++i, "--binary"));
    } else if (arg.startsWith("--binary=")) {
      opts.binary = path.resolve(arg.slice("--binary=".length));
    } else if (arg === "--themes") {
      opts.themes = requireArgValue(argv, ++i, "--themes");
    } else if (arg.startsWith("--themes=")) {
      opts.themes = arg.slice("--themes=".length);
    } else if (arg === "--screenshots-dir") {
      opts.screenshotsDir = path.resolve(requireArgValue(argv, ++i, "--screenshots-dir"));
    } else if (arg.startsWith("--screenshots-dir=")) {
      opts.screenshotsDir = path.resolve(arg.slice("--screenshots-dir=".length));
    } else if (arg === "--chrome") {
      opts.chrome = requireArgValue(argv, ++i, "--chrome");
    } else if (arg.startsWith("--chrome=")) {
      opts.chrome = arg.slice("--chrome=".length);
    } else {
      throw new Error(`Unknown CLI argument: ${arg}`);
    }
  }

  return opts;
}

function requireArgValue(argv, index, flag) {
  if (index >= argv.length || !argv[index]) {
    throw new Error(`Missing value for ${flag}`);
  }
  return argv[index];
}

function discoverExampleThemes() {
  if (!fs.existsSync(EXAMPLES_THEMES_DIR)) {
    return [];
  }
  const entries = fs.readdirSync(EXAMPLES_THEMES_DIR, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isDirectory() && !entry.name.startsWith("."))
    .map((entry) => entry.name)
    .filter((name) => fs.existsSync(path.join(EXAMPLES_THEMES_DIR, name, "theme.json")))
    .sort();
}

function resolveThemeList(themesFlag, availableExamples) {
  const defaultOrder = ["default", ...availableExamples];
  if (!themesFlag) {
    return defaultOrder;
  }
  const requested = themesFlag
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  if (requested.length === 0) {
    throw new Error("--themes list must not be empty");
  }
  for (const name of requested) {
    if (name !== "default" && !availableExamples.includes(name)) {
      throw new Error(
        `Unknown theme '${name}'. Available: ${defaultOrder.join(", ")}`
      );
    }
  }
  return requested;
}

function detectChromeExecutable(overridePath) {
  if (overridePath) {
    if (!fs.existsSync(overridePath)) {
      throw new Error(`--chrome executable not found: ${overridePath}`);
    }
    return overridePath;
  }
  for (const candidate of CHROME_CANDIDATES) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }
  return undefined;
}

function getFreePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const addr = server.address();
      if (!addr || typeof addr === "string") {
        server.close(() => reject(new Error("Failed to acquire ephemeral TCP port")));
        return;
      }
      const { port } = addr;
      server.close((err) => (err ? reject(err) : resolve(port)));
    });
  });
}

function httpGetStatus(url) {
  return new Promise((resolve, reject) => {
    const req = http.get(url, (res) => {
      res.resume();
      resolve(res.statusCode || 0);
    });
    req.on("error", reject);
    req.setTimeout(1500, () => {
      req.destroy(new Error("Request timeout"));
    });
  });
}

async function waitForHealthz(baseUrl, childProcess, logs, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (childProcess.exitCode !== null) {
      throw new Error(
        `toolstow exited prematurely (code=${childProcess.exitCode}):\n${logs.join("")}`
      );
    }
    try {
      const status = await httpGetStatus(`${baseUrl}/healthz`);
      if (status === 200) return;
    } catch (_) {
      // Server still booting
    }
    await new Promise((r) => setTimeout(r, 150));
  }
  throw new Error(`Timed out waiting for ${baseUrl}/healthz:\n${logs.join("")}`);
}

function buildStressToolHtml() {
  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>${STRESS_TOOL_NAME}</title>
  <link rel="stylesheet" href="/ui/v1/toolstow.css">
  <script src="/ui/v1/theme.js"></script>
  <style>
    .stress-search { flex: 1 1 180px; min-width: 0; }
    .stress-collection { --ts-collection-min: 280px; }
    .stress-cmd-title { font-weight: 600; font-size: 13px; color: var(--ts-ink); }
    .stress-cmd-desc { font-size: 12px; color: var(--ts-ink-muted); margin: 0; overflow-wrap: anywhere; }
    .stress-code { overflow-wrap: anywhere; word-break: break-word; max-width: 100%; }
  </style>
</head>
<body>
  <div class="ts-workbench">
    <header class="ts-standalone-only ts-standalone-header">
      <h1>🖥️ ${STRESS_TOOL_NAME}</h1>
    </header>

    <header class="ts-wb-toolbar">
      <input
        id="filter-input"
        class="ts-input stress-search"
        type="search"
        placeholder="搜索 VPS 脚本、BBR、流媒体解锁、回程路由..."
        aria-label="筛选脚本"
      >
      <span id="count-badge" class="ts-badge ts-badge-mono">6 条</span>
    </header>

    <nav class="ts-wb-nav" aria-label="脚本分类">
      <div id="tabs" class="ts-tabs" role="tablist">
        <button type="button" class="ts-tab active" role="tab" aria-selected="true" data-cat="all">全部</button>
        <button type="button" class="ts-tab" role="tab" aria-selected="false" data-cat="bench">基准压测</button>
        <button type="button" class="ts-tab" role="tab" aria-selected="false" data-cat="net">网络与流媒体</button>
        <button type="button" class="ts-tab" role="tab" aria-selected="false" data-cat="sys">内核与容器</button>
      </div>
    </nav>

    <main class="ts-wb-main" id="main-groups">
      <section class="ts-collection-group" data-group="bench">
        <h2 class="ts-section-title">基准压测与综合检测</h2>
        <div class="ts-collection stress-collection">
          <article class="ts-card">
            <div class="ts-card-header">
              <span class="stress-cmd-title">NodeQuality 综合基准测试</span>
              <span class="ts-badge">bench</span>
            </div>
            <p class="stress-cmd-desc">CPU / 内存 / 磁盘 I/O / 国际互联与三网回程路由一体化测试。</p>
            <pre class="ts-code stress-code">bash &lt;(curl -sL https://run.NodeQuality.com)</pre>
          </article>
          <article class="ts-card">
            <div class="ts-card-header">
              <span class="stress-cmd-title">YABS 国际性能基准</span>
              <span class="ts-badge">bench</span>
            </div>
            <p class="stress-cmd-desc">fio 磁盘随机读写 + iperf3 网络带宽 + Geekbench 跑分。</p>
            <pre class="ts-code stress-code">curl -sL yabs.sh | bash -s -- -i -g</pre>
          </article>
        </div>
      </section>

      <section class="ts-collection-group" data-group="net">
        <h2 class="ts-section-title">流媒体解锁与回程路由</h2>
        <div class="ts-collection stress-collection">
          <article class="ts-card">
            <div class="ts-card-header">
              <span class="stress-cmd-title">RegionRestrictionCheck 流媒体解锁</span>
              <span class="ts-badge">streaming</span>
            </div>
            <p class="stress-cmd-desc">检测 Netflix / YouTube / Disney+ / ChatGPT 地区可用性。</p>
            <pre class="ts-code stress-code">bash &lt;(curl -L -s check.unlock.media)</pre>
          </article>
          <article class="ts-card">
            <div class="ts-card-header">
              <span class="stress-cmd-title">IPQuality 住宅属性与风控检测</span>
              <span class="ts-badge">ip-check</span>
            </div>
            <p class="stress-cmd-desc">检测 ASN 类型、欺诈评分、原生/广播属性及邮局连通性。</p>
            <pre class="ts-code stress-code">bash &lt;(curl -Ls IP.Check.Place)</pre>
          </article>
        </div>
      </section>

      <section class="ts-collection-group" data-group="sys">
        <h2 class="ts-section-title">BBR 调优与 Docker 安装</h2>
        <div class="ts-collection stress-collection">
          <article class="ts-card">
            <div class="ts-card-header">
              <span class="stress-cmd-title">开启 BBR + fq 队列调度</span>
              <span class="ts-badge">bbr</span>
            </div>
            <p class="stress-cmd-desc">写入 sysctl.conf 并立即生效，优化高丢包长肥管道吞吐。</p>
            <pre class="ts-code stress-code">sysctl -w net.core.default_qdisc=fq net.ipv4.tcp_congestion_control=bbr</pre>
          </article>
          <article class="ts-card">
            <div class="ts-card-header">
              <span class="stress-cmd-title">Docker 官方一键安装脚本</span>
              <span class="ts-badge">docker</span>
            </div>
            <p class="stress-cmd-desc">安装 Docker Engine 与 Compose v2 插件并启用 systemd 服务。</p>
            <pre class="ts-code stress-code">curl -fsSL https://get.docker.com | sh &amp;&amp; systemctl enable --now docker</pre>
          </article>
        </div>
      </section>
    </main>
  </div>
  <script>
    (function () {
      var input = document.getElementById("filter-input");
      var badge = document.getElementById("count-badge");
      var tabs = document.getElementById("tabs");
      var groups = Array.from(document.querySelectorAll(".ts-collection-group"));
      var activeCat = "all";

      function applyFilter() {
        var q = (input.value || "").trim().toLowerCase();
        var visible = 0;
        groups.forEach(function (group) {
          var catOk = activeCat === "all" || group.dataset.group === activeCat;
          var cards = Array.from(group.querySelectorAll(".ts-card"));
          var groupVisible = 0;
          cards.forEach(function (card) {
            var text = (card.textContent || "").toLowerCase();
            var show = catOk && (!q || text.indexOf(q) !== -1);
            card.hidden = !show;
            if (show) {
              groupVisible += 1;
              visible += 1;
            }
          });
          group.hidden = groupVisible === 0;
        });
        badge.textContent = visible + " 条";
      }

      input.addEventListener("input", applyFilter);
      tabs.addEventListener("click", function (e) {
        var btn = e.target.closest(".ts-tab");
        if (!btn) return;
        activeCat = btn.dataset.cat || "all";
        Array.from(tabs.querySelectorAll(".ts-tab")).forEach(function (el) {
          var on = el === btn;
          el.classList.toggle("active", on);
          el.setAttribute("aria-selected", on ? "true" : "false");
        });
        applyFilter();
      });
    })();
  </script>
</body>
</html>
`;
}

function buildNormalToolHtml(title, bodyText) {
  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>${title}</title>
  <link rel="stylesheet" href="/ui/v1/toolstow.css">
  <script src="/ui/v1/theme.js"></script>
</head>
<body>
  <div class="ts-workbench">
    <header class="ts-standalone-only ts-standalone-header">
      <h1>${title}</h1>
    </header>
    <main class="ts-wb-main">
      <div class="ts-card">
        <h2 class="ts-section-title">${title}</h2>
        <p class="ts-help">${bodyText}</p>
        <pre class="ts-code">{"ok": true, "fixture": "${title}"}</pre>
      </div>
    </main>
  </div>
</body>
</html>
`;
}

function seedFixtureData(dataDir, exampleThemes) {
  const toolsDir = path.join(dataDir, "tools");
  const themesDir = path.join(dataDir, "themes");
  fs.mkdirSync(toolsDir, { recursive: true });
  fs.mkdirSync(themesDir, { recursive: true });

  const fixtures = [
    {
      manifest: {
        id: STRESS_TOOL_ID,
        name: STRESS_TOOL_NAME,
        version: "1.2.0",
        description: STRESS_TOOL_DESC,
        category: "运维",
        tags: STRESS_TOOL_TAGS,
        icon: "🖥️",
        entry: "index.html",
        runtime: "static",
        themeAware: true,
        sandbox: ["allow-scripts", "allow-forms"],
      },
      html: buildStressToolHtml(),
    },
    {
      manifest: {
        id: "json-tidy-fixture",
        name: "JSON 格式化工具",
        version: "1.0.0",
        description: "快速格式化、压缩并校验 JSON 数据结构。",
        category: "开发",
        tags: ["json", "format"],
        icon: "🧩",
        entry: "index.html",
        runtime: "static",
        themeAware: true,
        sandbox: ["allow-scripts"],
      },
      html: buildNormalToolHtml("JSON 格式化工具", "用于测试目录筛选与快速切换器的常规工具样例。"),
    },
    {
      manifest: {
        id: "cron-helper-fixture",
        name: "Cron 表达式助手",
        version: "1.0.0",
        description: "解析与生成常见定时任务 Cron 表达式。",
        category: "效率",
        tags: ["cron", "schedule"],
        icon: "⏱️",
        entry: "index.html",
        runtime: "static",
        themeAware: true,
        sandbox: ["allow-scripts"],
      },
      html: buildNormalToolHtml("Cron 表达式助手", "用于验证多分类标签与快速切换列表的常规工具样例。"),
    },
  ];

  for (const item of fixtures) {
    const dir = path.join(toolsDir, item.manifest.id);
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(
      path.join(dir, "manifest.json"),
      JSON.stringify(item.manifest, null, 2) + "\n",
      "utf8"
    );
    fs.writeFileSync(path.join(dir, "index.html"), item.html, "utf8");
  }

  for (const short of exampleThemes) {
    const src = path.join(EXAMPLES_THEMES_DIR, short);
    const dst = path.join(themesDir, short);
    fs.cpSync(src, dst, { recursive: true });
  }
}

async function apiJson(page, method, endpoint, body) {
  const result = await page.evaluate(
    async ({ method: m, endpoint: ep, body: b }) => {
      const res = await fetch(ep, {
        method: m,
        credentials: "same-origin",
        headers: {
          "Content-Type": "application/json",
          "X-ToolStow-Client": "verify-theme-mobile",
        },
        body: b === undefined ? undefined : JSON.stringify(b),
      });
      const text = await res.text();
      return { ok: res.ok, status: res.status, text };
    },
    { method, endpoint, body }
  );
  if (!result.ok) {
    throw new Error(`${method} ${endpoint} failed (${result.status}): ${result.text}`);
  }
  return result.text ? JSON.parse(result.text) : null;
}

async function waitForCatalogLoaded(page) {
  await page.waitForFunction(
    ({ toolId, toolName }) => {
      const adminLink = document.querySelector('a[href="/admin"]');
      if (!adminLink || adminLink.hidden) return false;
      const bodyText = document.body ? document.body.innerText : "";
      return bodyText.includes(toolName) || Boolean(document.querySelector(`[data-tool-id="${toolId}"]`));
    },
    { toolId: STRESS_TOOL_ID, toolName: STRESS_TOOL_NAME },
    { timeout: 10000 }
  );
}

async function clickStressToolInCatalog(page) {
  const byAttr = page.locator(`[data-tool-id="${STRESS_TOOL_ID}"]`).first();
  if ((await byAttr.count()) > 0) {
    await byAttr.scrollIntoViewIfNeeded();
    await byAttr.click();
    return;
  }

  const candidates = page.locator(
    "#empty-grid button, #tool-grid button, #tool-table-body tr, .tool-row"
  );
  const total = await candidates.count();
  for (let i = 0; i < total; i += 1) {
    const item = candidates.nth(i);
    const text = (await item.innerText().catch(() => "")) || "";
    if (text.includes(STRESS_TOOL_ID) || text.includes(STRESS_TOOL_NAME)) {
      const innerBtn = item.locator("button").first();
      const target = (await innerBtn.count()) > 0 ? innerBtn : item;
      await target.scrollIntoViewIfNeeded();
      await target.click();
      return;
    }
  }
  throw new Error(`Could not find clickable catalog entry for '${STRESS_TOOL_ID}'`);
}

async function waitForAndCheckIframe(page, viewport) {
  const iframeLoc = page.locator("#iframe-host iframe, iframe").first();
  await iframeLoc.waitFor({ state: "visible", timeout: 10000 });

  const iframeHandle = await iframeLoc.elementHandle();
  if (!iframeHandle) {
    throw new Error("Tool iframe element handle is null");
  }

  const frame = await iframeHandle.contentFrame();
  if (!frame) {
    throw new Error("Tool iframe contentFrame() is null");
  }

  await frame.waitForFunction(
    () => Boolean(document.body && document.querySelector(".ts-workbench") && document.readyState !== "loading"),
    null,
    { timeout: 10000 }
  );

  // Give layout/theme sync one animation frame to settle.
  await page.evaluate(
    () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))
  );

  const hostMetrics = await page.evaluate(() => {
    const vh = window.innerHeight;
    const vw = window.innerWidth;
    const docEl = document.documentElement;
    const scrollWidth = Math.max(docEl.scrollWidth, document.body ? document.body.scrollWidth : 0);
    const iframe = document.querySelector("#iframe-host iframe") || document.querySelector("iframe");
    if (!iframe) {
      return { found: false, vw, vh, scrollWidth };
    }
    const rect = iframe.getBoundingClientRect();
    const visibleHeight = Math.max(0, Math.min(rect.bottom, vh) - Math.max(rect.top, 0));
    const visibleWidth = Math.max(0, Math.min(rect.right, vw) - Math.max(rect.left, 0));
    return {
      found: true,
      vw,
      vh,
      scrollWidth,
      rect: {
        top: rect.top,
        bottom: rect.bottom,
        left: rect.left,
        right: rect.right,
        width: rect.width,
        height: rect.height,
      },
      visibleHeight,
      visibleWidth,
    };
  });

  if (!hostMetrics.found) {
    throw new Error("Iframe disappeared before geometry check");
  }

  const innerMetrics = await frame.evaluate(() => {
    const docEl = document.documentElement;
    return {
      innerWidth: window.innerWidth,
      scrollWidth: Math.max(docEl.scrollWidth, document.body ? document.body.scrollWidth : 0),
    };
  });

  const failures = [];
  if (hostMetrics.scrollWidth > hostMetrics.vw + 1) {
    failures.push(
      `host horizontal overflow (${hostMetrics.scrollWidth}px > ${hostMetrics.vw}px + 1)`
    );
  }
  if (hostMetrics.visibleHeight < viewport.minIframeHeightPx) {
    failures.push(
      `iframe visible height ${hostMetrics.visibleHeight.toFixed(1)}px < required ${viewport.minIframeHeightPx}px`
    );
  }
  const minWidthPx = viewport.width * 0.85;
  if (hostMetrics.visibleWidth < minWidthPx) {
    failures.push(
      `iframe visible width ${hostMetrics.visibleWidth.toFixed(1)}px < required ${minWidthPx.toFixed(1)}px (85% vw)`
    );
  }
  if (innerMetrics.scrollWidth > innerMetrics.innerWidth + 2) {
    failures.push(
      `iframe inner horizontal overflow (${innerMetrics.scrollWidth}px > ${innerMetrics.innerWidth}px + 2)`
    );
  }

  return { hostMetrics, innerMetrics, failures };
}

async function checkAdminLinkVisibleAndInViewport(page, vw, vh) {
  const info = await page.evaluate(() => {
    const el = document.querySelector('a[href="/admin"]');
    if (!el) return { exists: false };
    const cs = getComputedStyle(el);
    const rect = el.getBoundingClientRect();
    return {
      exists: true,
      hidden: el.hidden,
      display: cs.display,
      visibility: cs.visibility,
      rect: {
        top: rect.top,
        bottom: rect.bottom,
        left: rect.left,
        right: rect.right,
        width: rect.width,
        height: rect.height,
      },
    };
  });

  if (!info.exists || info.hidden || info.display === "none" || info.visibility === "hidden") {
    return "admin link (/admin) is not visible after admin login";
  }
  if (info.rect.width <= 0 || info.rect.height <= 0) {
    return "admin link (/admin) has zero bounding box";
  }
  if (info.rect.left < -1 || info.rect.right > vw + 1 || info.rect.top < -1 || info.rect.bottom > vh + 1) {
    return `admin link (/admin) outside viewport (${JSON.stringify(info.rect)} in ${vw}x${vh})`;
  }
  return null;
}

async function findVisibleLocator(page, selectors) {
  for (const sel of selectors) {
    const loc = page.locator(sel);
    const count = await loc.count();
    for (let i = 0; i < count; i += 1) {
      const candidate = loc.nth(i);
      if (await candidate.isVisible().catch(() => false)) {
        return candidate;
      }
    }
  }
  return null;
}

async function verifyDetailsPopoverIfPresent(page, viewport) {
  const toggle = await findVisibleLocator(page, [
    '[data-role="tool-details-toggle"]',
    "#info-btn",
    "#info-toggle",
  ]);
  if (!toggle) {
    return [];
  }

  const ariaControls = await toggle.getAttribute("aria-controls");
  const failures = [];
  await toggle.click();
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

  const afterOpen = await page.evaluate((controlsId) => {
    const vw = window.innerWidth;
    const scrollWidth = Math.max(
      document.documentElement.scrollWidth,
      document.body ? document.body.scrollWidth : 0
    );
    const popSelectors = [
      ...(controlsId ? [`#${CSS.escape(controlsId)}`] : []),
      '[data-role="tool-details-popover"]',
      "#info-popover",
      ".info-popover",
      ".tool-details-popover",
    ];
    let popRect = null;
    for (const sel of popSelectors) {
      const el = document.querySelector(sel);
      if (el && !el.hidden && getComputedStyle(el).display !== "none" && getComputedStyle(el).visibility !== "hidden") {
        const r = el.getBoundingClientRect();
        popRect = { left: r.left, right: r.right, width: r.width, height: r.height };
        break;
      }
    }
    return { vw, scrollWidth, popRect };
  }, ariaControls);

  if (afterOpen.scrollWidth > afterOpen.vw + 1) {
    failures.push(
      `details popover caused horizontal overflow (${afterOpen.scrollWidth}px > ${afterOpen.vw}px + 1)`
    );
  }
  if (afterOpen.popRect && (afterOpen.popRect.left < -1 || afterOpen.popRect.right > viewport.width + 1)) {
    failures.push(
      `details popover clipped outside viewport (${JSON.stringify(afterOpen.popRect)})`
    );
  }

  await page.keyboard.press("Escape");
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

  const stillOpen = await page.evaluate((controlsId) => {
    const popSelectors = [
      ...(controlsId ? [`#${CSS.escape(controlsId)}`] : []),
      '[data-role="tool-details-popover"]',
      "#info-popover",
      ".info-popover",
      ".tool-details-popover",
    ];
    for (const sel of popSelectors) {
      const el = document.querySelector(sel);
      if (el && !el.hidden && getComputedStyle(el).display !== "none" && getComputedStyle(el).visibility !== "hidden") {
        const r = el.getBoundingClientRect();
        if (r.width > 0 && r.height > 0) return true;
      }
    }
    return false;
  }, ariaControls);

  if (stillOpen) {
    failures.push("details popover did not close on Escape");
  }

  return failures;
}

async function maybeCaptureScreenshot(page, screenshotsDir, filename) {
  if (!screenshotsDir) return;
  fs.mkdirSync(screenshotsDir, { recursive: true });
  await page.screenshot({ path: path.join(screenshotsDir, filename), fullPage: false });
}

async function runThemeViewportCheck(page, baseUrl, theme, viewport, screenshotsDir) {
  await page.setViewportSize({ width: viewport.width, height: viewport.height });
  const pageErrors = [];
  const onPageError = (err) => pageErrors.push(err.message || String(err));
  page.on("pageerror", onPageError);

  const failures = [];
  let workspaceMetrics = null;
  let deepLinkMetrics = null;

  try {
    // 1. Catalog load on `/`
    await page.goto("about:blank");
    await page.goto(`${baseUrl}/`, { waitUntil: "networkidle" });
    await waitForCatalogLoaded(page);

    const catalogOverflow = await page.evaluate(() => ({
      vw: window.innerWidth,
      scrollWidth: Math.max(
        document.documentElement.scrollWidth,
        document.body ? document.body.scrollWidth : 0
      ),
    }));
    if (catalogOverflow.scrollWidth > catalogOverflow.vw + 1) {
      failures.push(
        `catalog overflow (${catalogOverflow.scrollWidth}px > ${catalogOverflow.vw}px + 1)`
      );
    }

    const adminErr = await checkAdminLinkVisibleAndInViewport(page, viewport.width, viewport.height);
    if (adminErr) failures.push(adminErr);

    await maybeCaptureScreenshot(page, screenshotsDir, `${theme}__${viewport.name}__catalog.png`);

    // 2. Open stress tool from catalog
    await clickStressToolInCatalog(page);
    const wsCheck = await waitForAndCheckIframe(page, viewport);
    workspaceMetrics = wsCheck.hostMetrics;
    failures.push(...wsCheck.failures.map((f) => `[workspace] ${f}`));

    const popoverFailures = await verifyDetailsPopoverIfPresent(page, viewport);
    failures.push(...popoverFailures.map((f) => `[workspace] ${f}`));

    await maybeCaptureScreenshot(page, screenshotsDir, `${theme}__${viewport.name}__workspace.png`);

    // 3. Return/back button visible, clickable, and restores catalog view
    const backBtn = await findVisibleLocator(page, [
      '[data-role="tool-back"]',
      "#back-btn",
      "#back-to-list-btn",
      "#back-to-notebook-btn",
      "#mobile-back-btn",
      "#close-tool-btn",
    ]);
    if (!backBtn) {
      failures.push("[workspace] no visible return/back button found");
    } else {
      await backBtn.click();
      try {
        await page.waitForFunction(
          ({ toolName }) => {
            const iframe = document.querySelector("#iframe-host iframe") || document.querySelector("iframe");
            const iframeGone = !iframe || iframe.offsetParent === null || Boolean(iframe.closest("[hidden]"));
            const bodyText = document.body ? document.body.innerText : "";
            return iframeGone && bodyText.includes(toolName);
          },
          { toolName: STRESS_TOOL_NAME },
          { timeout: 5000 }
        );
      } catch (_) {
        failures.push("[workspace] clicking back button did not return to catalog view");
      }
    }

    // 4. Direct deep-link open `/#tool=vps-scripts-stress`
    await page.goto("about:blank");
    await page.goto(`${baseUrl}/#tool=${encodeURIComponent(STRESS_TOOL_ID)}`, {
      waitUntil: "networkidle",
    });
    const dlCheck = await waitForAndCheckIframe(page, viewport);
    deepLinkMetrics = dlCheck.hostMetrics;
    failures.push(...dlCheck.failures.map((f) => `[deep-link] ${f}`));

    await maybeCaptureScreenshot(page, screenshotsDir, `${theme}__${viewport.name}__deeplink.png`);
  } finally {
    page.off("pageerror", onPageError);
  }

  for (const errMsg of pageErrors) {
    failures.push(`uncaught pageerror: ${errMsg}`);
  }

  const minSeenHeight = Math.min(
    workspaceMetrics ? workspaceMetrics.visibleHeight : 0,
    deepLinkMetrics ? deepLinkMetrics.visibleHeight : 0
  );
  const vhPct = ((minSeenHeight / viewport.height) * 100).toFixed(1);
  const overflowOk = !failures.some((f) => f.includes("overflow"));

  return {
    theme,
    viewport: viewport.name,
    minIframeHeightPx: minSeenHeight,
    vhPct,
    overflowOk,
    passed: failures.length === 0,
    failures,
  };
}

function formatTable(results) {
  const headers = ["THEME", "VIEWPORT", "IFRAME_VIS_H", "VH_%", "OVERFLOW", "RESULT"];
  const rows = results.map((r) => [
    r.theme,
    r.viewport,
    `${Math.round(r.minIframeHeightPx)}px`,
    `${r.vhPct}%`,
    r.overflowOk ? "OK" : "OVERFLOW",
    r.passed ? "PASS" : "FAIL",
  ]);
  const widths = headers.map((h, idx) =>
    Math.max(h.length, ...rows.map((row) => row[idx].length))
  );
  const pad = (row) => row.map((cell, idx) => cell.padEnd(widths[idx])).join("  ");
  const sep = widths.map((w) => "-".repeat(w)).join("  ");
  return [pad(headers), sep, ...rows.map(pad)].join("\n");
}

async function main() {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (err) {
    console.error(`ERROR: ${err.message}`);
    printUsage();
    return 2;
  }

  if (opts.help) {
    printUsage();
    return 0;
  }

  let playwright;
  try {
    playwright = require("playwright-core");
  } catch (err) {
    console.error(
      [
        "ERROR: Unable to load 'playwright-core'.",
        "Install it outside the repo (e.g. `npm --prefix /tmp/pw install --no-save playwright-core`)",
        "and run with `NODE_PATH=/tmp/pw/node_modules node scripts/verify_theme_mobile.cjs`.",
        `Details: ${err.message}`,
      ].join("\n")
    );
    return 2;
  }

  if (!fs.existsSync(opts.binary)) {
    console.error(`ERROR: toolstow binary not found at: ${opts.binary}`);
    return 2;
  }

  const exampleThemes = discoverExampleThemes();
  let themesToTest;
  try {
    themesToTest = resolveThemeList(opts.themes, exampleThemes);
  } catch (err) {
    console.error(`ERROR: ${err.message}`);
    return 2;
  }

  const chromePath = detectChromeExecutable(opts.chrome);
  const tempDataDir = fs.mkdtempSync(path.join(os.tmpdir(), "toolstow-mobile-verify-"));
  let serverProc = null;
  let browser = null;
  const serverLogs = [];

  try {
    seedFixtureData(tempDataDir, exampleThemes);
    const port = await getFreePort();
    const baseUrl = `http://127.0.0.1:${port}`;

    serverProc = spawn(
      opts.binary,
      ["--host", "127.0.0.1", "--port", String(port), "--data-dir", tempDataDir],
      {
        env: { ...process.env, TOOLSTOW_ADMIN_KEY: ADMIN_KEY },
        stdio: ["ignore", "pipe", "pipe"],
      }
    );
    serverProc.stdout.on("data", (chunk) => serverLogs.push(chunk.toString()));
    serverProc.stderr.on("data", (chunk) => serverLogs.push(chunk.toString()));

    await waitForHealthz(baseUrl, serverProc, serverLogs);

    console.log(
      `Browser: ${chromePath || "playwright-bundled-chromium"} | Binary: ${opts.binary} | Themes: ${themesToTest.join(", ")}`
    );

    browser = await playwright.chromium.launch({
      headless: true,
      executablePath: chromePath,
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    });

    const context = await browser.newContext({
      viewport: { width: 393, height: 641 },
    });
    const page = await context.newPage();

    // Log in as admin once in the browser context so `/admin` link is visible on `/`.
    await page.goto(`${baseUrl}/`, { waitUntil: "domcontentloaded" });
    await apiJson(page, "POST", "/api/admin/login", { key: ADMIN_KEY });
    const me = await apiJson(page, "GET", "/api/me");
    if (!me || me.isAdmin !== true) {
      throw new Error(`Expected isAdmin === true after login, got: ${JSON.stringify(me)}`);
    }

    const results = [];
    for (const theme of themesToTest) {
      await apiJson(page, "PUT", "/api/admin/themes/active", { short: theme });
      for (const viewport of VIEWPORTS) {
        const res = await runThemeViewportCheck(
          page,
          baseUrl,
          theme,
          viewport,
          opts.screenshotsDir
        );
        results.push(res);
      }
    }

    console.log("\n" + formatTable(results) + "\n");

    const failed = results.filter((r) => !r.passed);
    if (failed.length > 0) {
      console.error("Failures:");
      for (const f of failed) {
        console.error(`  [${f.theme} @ ${f.viewport}]`);
        for (const reason of f.failures) {
          console.error(`    - ${reason}`);
        }
      }
      return 1;
    }

    console.log(`All ${results.length} theme/viewport checks passed.`);
    return 0;
  } finally {
    if (browser) {
      await browser.close().catch(() => {});
    }
    if (serverProc && serverProc.exitCode === null) {
      serverProc.kill("SIGTERM");
      await new Promise((resolve) => {
        const timer = setTimeout(() => {
          if (serverProc.exitCode === null) serverProc.kill("SIGKILL");
          resolve();
        }, 3000);
        serverProc.once("exit", () => {
          clearTimeout(timer);
          resolve();
        });
      });
    }
    fs.rmSync(tempDataDir, { recursive: true, force: true });
  }
}

main()
  .then((code) => {
    process.exitCode = code;
  })
  .catch((err) => {
    console.error(`FATAL: ${err.stack || err.message || err}`);
    process.exitCode = 1;
  });
