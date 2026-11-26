---
name: toolstow-tool
description: Build, validate, package, and publish self-contained static web micro-tools for ToolStow (v0.2+), including manifest.json schema, /ui/v1/ SDK integration, sandbox safety checks, clean ZIP packaging, and Bearer API upload.
---

# ToolStow AI Tool Development & Publishing Skill

Use this skill whenever generating, validating, packaging, or publishing a self-contained static web micro-tool for **ToolStow** (v0.2+).

> **Companion scripts** live right inside this skill directory under `scripts/` (zero external dependencies, Python 3.9+ stdlib only):
> - `scripts/validate.py <tool_dir>` — validate `manifest.json`, sandbox/XSS hygiene, `/ui/v1/*` SDK usage, archive limits, and JS syntax (`node --check` when available).
> - `scripts/pack.py <tool_dir> [-o out.zip]` — run full validation and build a clean flat `.zip` (no `.DS_Store`, `__MACOSX`, `.git`, or symlinks).
> - `scripts/publish.py <tool_dir> [--overwrite]` — validate, pack in memory, and upload via `Authorization: Bearer $TOOLSTOW_API_KEY` (defaults to `?overwrite=false` to prevent accidental overwrites).

---

## 1. Quick Workflow (Always Run Scripts Before Delivery)

1. Create a tool directory named after your `manifest.id` (e.g. `json-pretty/`) containing `manifest.json`, `index.html`, and any local relative assets.
2. **Validate**:
   ```bash
   python3 skills/toolstow-tool/scripts/validate.py ./json-pretty
   ```
3. **Package** (for manual `/admin` upload or release artifacts):
   ```bash
   python3 skills/toolstow-tool/scripts/pack.py ./json-pretty -o ./json-pretty.zip
   ```
4. **Publish directly via Bearer API** (requires `TOOLSTOW_URL` and `TOOLSTOW_API_KEY`):
   ```bash
   export TOOLSTOW_URL="http://127.0.0.1:8080"
   export TOOLSTOW_API_KEY="<your-api-key>"

   # Safe create-only upload (returns 409 TOOL_EXISTS if ID already exists)
   python3 skills/toolstow-tool/scripts/publish.py ./json-pretty

   # Intentional overwrite of an existing tool
   python3 skills/toolstow-tool/scripts/publish.py ./json-pretty --overwrite
   ```

*(If this skill folder is installed standalone outside the ToolStow repo, replace `skills/toolstow-tool/scripts/...` with the path to this skill's `scripts/` directory.)*

---

## 2. Core Principles

- **Zero-build & self-contained**: Output plain HTML, CSS, and vanilla JS (a single `index.html` is ideal for small utilities; split local relative assets only when needed). Avoid external CDN dependencies so the tool works offline and inside strict sandboxes.
- **Opaque-origin safe**: Every tool runs inside a dual sandbox (`<iframe sandbox="...">` + `Content-Security-Policy: sandbox ...`) with an opaque origin (`Origin: null`). Never assume same-origin browser storage is available.
- **Shared UI SDK (`/ui/v1/`)**: Every native ToolStow plugin MUST load `/ui/v1/toolstow.css` and `/ui/v1/theme.js` in `<head>` to inherit the host's design system, automatic Light/Dark & accent sync, zero-FOUC first paint, and any active theme's `ui.css` site-wide reskin automatically.
- **No duplicate top headers inside iframes**: The ToolStow shell already renders the active tool's icon, title, and description in its top bar. Start directly with functional controls; wrap any standalone header in `.ts-standalone-only`.
- **Draft-friendly publishing**: Tools support a Hugo-style `"draft": true` flag so AI agents can publish work-in-progress tools for admin preview before making them public.

---

## 3. Directory & ZIP Packaging

On disk, each tool lives inside the host's data directory at `<data-dir>/tools/{manifest.id}/` (default `./data/tools/{manifest.id}/`):

```text
my-tool/
├── manifest.json   # Required
├── index.html      # Default entry file
└── assets/         # Optional relative assets (CSS, JS, SVG, PNG, etc.)
```

### ZIP & Single-HTML Ingestion Formats (All Supported)

Use `scripts/pack.py` to generate a verified, flat `.zip` automatically:

```bash
python3 skills/toolstow-tool/scripts/pack.py ./my-tool -o ./my-tool.zip
```

ToolStow accepts:
1. **Flat ZIP** (`manifest.json` at archive root, produced by `pack.py`).
2. **Single-root-folder ZIP** (`my-tool/manifest.json`, with `__MACOSX/` and `.DS_Store` automatically ignored).
3. **Single-file `.html` / `.htm` drag-and-drop or Paste HTML in `/admin`** (and `POST /api/admin/tools/html`): Drop any standalone `.html` file directly into `/admin` -> **工具管理 -> 收录工具** (or click **粘贴 HTML**). ToolStow automatically extracts `<title>` and `<meta name="description">`, generates a valid `id` slug and `manifest.json`, previews the tool in a live sandboxed iframe, and optionally injects `<link rel="stylesheet" href="/ui/v1/toolstow.css">` + `<script src="/ui/v1/theme.js"></script>` before saving.

### Packaging Limits & Security Rules

- **Max upload size**: 50 MB default (configurable on host via `--max-upload-mb` / `TOOLSTOW_MAX_UPLOAD_MB`).
- **Max uncompressed size**: 200 MB total.
- **Max file count**: 1,000 files per archive.
- **Forbidden entries**: No symlinks, no `..` path traversal, no absolute paths (`/`), no Windows drive prefixes or backslashes (`\`), no duplicate normalized paths. Dot-prefixed entries at the top level (e.g. `.DS_Store`) are ignored/reserved.

---

## 4. `manifest.json` Schema

Minimal valid manifest (only `id` and `name` are required):

```json
{
  "id": "json-pretty",
  "name": "JSON 格式化工具"
}
```

Full example:

```json
{
  "id": "dynamic-favicon",
  "name": "动态强调色 Favicon 生成器",
  "version": "1.0.0",
  "description": "随主站主题实时同步强调色并导出 SVG / PNG 图标",
  "category": "设计",
  "tags": ["icon", "favicon", "svg", "png"],
  "icon": "🎨",
  "entry": "index.html",
  "runtime": "static",
  "draft": false,
  "themeAware": true,
  "sandbox": ["allow-scripts", "allow-downloads", "allow-forms"]
}
```

### Field Reference

| Field | Type | Required | Default | Validation & Semantics |
| :--- | :--- | :--- | :--- | :--- |
| `id` | `string` | **Yes** | — | Globally unique ID matching `^[a-z0-9]+(?:-[a-z0-9]+)*$` (1–64 chars). When loaded from disk, must match the directory name. |
| `name` | `string` | **Yes** | — | Display title (non-empty after trimming). |
| `version` | `string` | No | `"0.1.0"` | Display version string. |
| `description` | `string` | No | `""` | Short summary shown in the sidebar/header and indexed by search. |
| `category` | `string` | No | `"未分类"` | Sidebar grouping label (trimmed; empty string falls back to `"未分类"`). Recommended standard categories to avoid fragmentation: `"开发"`, `"运维"`, `"设计"`, `"文本"`, `"时间日期"`, `"转换"`, `"效率"`, `"实用工具"` (custom names are still allowed). |
| `tags` | `string[]` | No | `[]` | Search and filter tags (deduplicated; empty strings ignored). |
| `icon` | `string` | No | `"🛠️"` | Emoji string, or a safe relative image path inside the tool directory (e.g. `"assets/icon.svg"`). Must exist if path-like. |
| `entry` | `string` | No | `"index.html"` | Safe relative path to an existing regular HTML file (no leading `/`, `..`, `\`, or URL scheme). |
| `runtime` | `"static"` | No | `"static"` | Only `"static"` is currently supported. Declaring `"script"` is rejected. |
| `draft` | `boolean` | No | `false` | Hugo-style visibility flag. When `true`, the tool is hidden from public `GET /api/tools` and returns `404` on `/t/{id}/*` for anonymous visitors; visible only to an authenticated Admin session or when the host runs with `--include-drafts`. |
| `themeAware` | `boolean` | No | `false` | Set `true` for plugins using `/ui/v1/theme.js` (the host also syncs automatically once `toolstow:ready` is received). |
| `sandbox` | `string[]` | No | `["allow-scripts", "allow-downloads", "allow-forms"]` | Requested iframe sandbox tokens, filtered by the host allowlist. For pure calculation tools without file export, prefer least-privilege `["allow-scripts", "allow-forms"]` (`window.ToolStow.copy()` requires no extra sandbox token). |

### Sandbox Token Allowlist

Only these tokens are accepted by the host:

- `allow-scripts`
- `allow-downloads`
- `allow-forms`
- `allow-modals`
- `allow-popups`
- `allow-popups-to-escape-sandbox`

`allow-same-origin` and `allow-top-navigation*` are **always stripped** by the host (and blocked by the response header `Content-Security-Policy: sandbox ...`).

---

## 5. Sandbox Limitations & Defensive Patterns

Because tools run in an opaque origin (`Origin: null`):

1. **No unguarded `localStorage` / `sessionStorage` / `document.cookie`**
   - Direct calls like `localStorage.getItem(...)` throw `SecurityError` in sandboxed iframes and will break script initialization.
   - Keep state in memory, URL hash, or file export/import (Blob download / `<input type="file">`). Never call `localStorage` without a `try/catch` (better: omit it completely).
2. **Use `window.ToolStow.copy()` for clipboard**
   - `navigator.clipboard.writeText()` may reject inside a sandboxed iframe or non-HTTPS origin. Use `await window.ToolStow.copy(text, fallbackEl)` from `/ui/v1/theme.js`, which handles the 3-tier sandbox-safe fallback (`navigator.clipboard` -> temporary `<textarea>` + `execCommand("copy")` -> optional DOM Range selection) automatically.
3. **File downloads**
   - Include `"allow-downloads"` in `manifest.json`'s `sandbox` array, and export files via `Blob` + `URL.createObjectURL()` + `<a download="filename">`.
4. **No calls to ToolStow host admin APIs**
   - All management endpoints under `/api/admin/*` require authentication and reject `Origin: null` and cross-site requests. Tools must not attempt to mutate host state or embed secrets.
5. **Safe DOM rendering**
   - Prefer `textContent` and DOM element builders over `innerHTML` to avoid XSS when rendering user input.

---

## 6. Shared UI SDK (`/ui/v1/toolstow.css` + `/ui/v1/theme.js`)

ToolStow serves a locked shared UI SDK at `/ui/v1/toolstow.css` and `/ui/v1/theme.js` (also shared by the host shell and `/admin`). When the active theme provides `ui.css`, ToolStow automatically appends it to `/ui/v1/toolstow.css`, reskinning the host and every plugin in unison.

### Mandatory `<head>` Includes

Every native ToolStow plugin MUST include both lines in `<head>` (do **NOT** copy-paste `:root` color palettes or manual `postMessage` theme listeners):

```html
<link rel="stylesheet" href="/ui/v1/toolstow.css">
<script src="/ui/v1/theme.js"></script>
```

> Load `<script src="/ui/v1/theme.js"></script>` synchronously in `<head>` (without `defer`/`async`) so it sets `data-embed` and `data-theme` from the initial query parameters (`?theme=...&accent=...`) or OS preference before first paint, eliminating dark-mode white flash (FOUC).

### 4-Layer Decoupled UI Architecture & `.ts-workbench` Semantic Slots

Follow ToolStow's 4-layer separation so plugins feel native to every theme and allow themes to rearrange tool layout slots via `ui.css` without breaking tool functionality:

1. **Layer 1 — Shared design tokens & controls (`/ui/v1/toolstow.css`)**:
   - Use `.ts-*` primitives for cards, buttons, inputs, selects, textareas, tabs, badges, code blocks, empty states, and toasts.
   - Reference `var(--ts-*)` tokens for any custom border, surface, text, radius, spacing, or font styling.
   - **NEVER** hardcode UI chrome hex/rgb colors, decorative background gradients, heavy drop shadows, or custom external web fonts.
2. **Layer 2 — Semantic workbench slots (`.ts-workbench` + `.ts-wb-*`)**:
   - Structure your tool root with `<div class="ts-workbench">` (optional preferences: `data-width="narrow"`, `data-aside="left"`, or `data-aside="right"`) and direct semantic slot children:
     - `<header class="ts-standalone-only ts-standalone-header">` — standalone page title (auto-hidden when embedded in an iframe).
     - `<header class="ts-wb-toolbar">` — search bar, primary action buttons, status counters, keyboard hints.
     - `<nav class="ts-wb-nav">` — category tabs (`.ts-tabs`) or section navigation.
     - `<aside class="ts-wb-aside">` — optional configuration/inspector sidebar panel.
     - `<main class="ts-wb-main">` — primary workspace output, cards, or preview stage.
   - Inside `.ts-wb-main`, wrap grouped card sections in `.ts-collection-group` and multi-card grids in `.ts-collection` (setting a preferred minimum card width via `--ts-collection-min: 380px` on a custom class rather than hardcoding `grid-template-columns`).
   - **Why semantic slots matter**: Tools declare *what* each slot is and its default preference (`data-aside="left|right"`, `--ts-collection-min`), while active themes can rearrange those slots via `ui.css` (for example, `ascii-probe` moves `.ts-wb-nav` to a left vertical directory rail, `.ts-wb-toolbar` to a bottom `grep>` command bar, and switches `.ts-collection` into a 1-column terminal stream; `rakugaki` turns `.ts-wb-nav` into right-edge vertical notebook index tabs). Do **NOT** hardcode `grid-template-areas` or `position: fixed` on `.ts-wb-*` in your tool's `<style>`.
3. **Layer 3 — Scoped functional component styles (minimal `<style>`)**:
   - Plugins MAY add a small `<style>` block for tool-specific inner component layout (e.g. `.io-area`, `.preview-stage`, `.color-picker`) using `var(--ts-space-*)` and `var(--ts-*)` tokens.
4. **Layer 4 — Domain content visuals (FREE domain colors)**:
   - Canvas/SVG artwork, color pickers/swatches, image previews, charts, syntax highlighting, and diagram payloads may freely use domain-specific colors.

### No Duplicate Top Headers Inside Iframes

- **FORBIDDEN inside the ToolStow shell**: Do NOT render a persistent top banner/hero/title `<h1>` repeating the tool name already shown in the host shell's top bar. Start directly with functional controls (`.ts-wb-toolbar`, `.ts-wb-nav`, `.ts-wb-aside`, `.ts-wb-main`).
- **Standalone header pattern**: If you want a page title when the user opens the tool directly in a new tab at `/t/{id}/index.html`, wrap it in `.ts-standalone-only` paired with `.ts-standalone-header` as the first child of `.ts-workbench`:
  ```html
  <header class="ts-standalone-only ts-standalone-header">
    <h1>🛠️ 工具名称</h1>
  </header>
  ```
  `/ui/v1/theme.js` automatically sets `data-embed="true"` inside an iframe and `toolstow.css` hides `.ts-standalone-only`.

### Minimal Copy-Paste Starter Template

```html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>示例微工具</title>
  <link rel="stylesheet" href="/ui/v1/toolstow.css">
  <script src="/ui/v1/theme.js"></script>
  <style>
    /* Layer 3: minimal tool-specific inner component styles only */
    .io-area {
      min-height: 220px;
      font-family: var(--ts-font-mono);
    }
  </style>
</head>
<body>
  <div class="ts-workbench" data-aside="left">
    <header class="ts-standalone-only ts-standalone-header">
      <h1>🛠️ 示例微工具</h1>
    </header>

    <header class="ts-wb-toolbar">
      <div class="ts-row">
        <button id="run-btn" class="ts-btn ts-btn-primary" type="button">转换</button>
        <button id="copy-btn" class="ts-btn" type="button">复制结果</button>
      </div>
      <span id="meta-badge" class="ts-badge ts-badge-mono">0 字符</span>
    </header>

    <aside class="ts-wb-aside ts-card" aria-label="输入面板">
      <div class="ts-card-header">
        <h2 class="ts-section-title">输入</h2>
      </div>
      <textarea id="input" class="ts-textarea io-area" placeholder="在此粘贴内容..."></textarea>
    </aside>

    <main class="ts-wb-main ts-card" aria-label="输出面板">
      <div class="ts-card-header">
        <h2 class="ts-section-title">输出</h2>
      </div>
      <pre id="output" class="ts-code io-area"></pre>
    </main>
  </div>

  <script>
    const inputEl = document.getElementById("input");
    const outputEl = document.getElementById("output");
    const metaBadge = document.getElementById("meta-badge");

    function update() {
      const value = inputEl.value.trim();
      outputEl.textContent = value;
      metaBadge.textContent = `${value.length} 字符`;
    }

    document.getElementById("run-btn").addEventListener("click", update);
    document.getElementById("copy-btn").addEventListener("click", async () => {
      const ok = await window.ToolStow.copy(outputEl.textContent, outputEl);
      window.ToolStow.toast(ok ? "已复制到剪贴板" : "复制受限，已选中文本请按 Cmd/Ctrl+C");
    });

    // Optional: react to host theme / accent changes for domain visuals (Canvas/SVG)
    window.ToolStow.onThemeChange(({ mode, accent }) => {
      void mode;
      void accent;
    });
  </script>
</body>
</html>
```

### CSS Tokens Cheat Sheet (`var(--ts-*)`)

- Base `html, body` uses `font-size: 14px; line-height: 1.5;` and automatically sets `color-scheme: light` / `dark` (so native `<input type="date">`, `<select>`, and scrollbars match the active theme).
- There are **no fixed font-size tokens** — use `px` or `rem` freely for typography hierarchy (e.g. large metric readouts).
- For soft accent-tinted surfaces without hardcoding colors, use `color-mix(in srgb, var(--ts-accent) 12%, transparent)`.

| Category | Tokens |
| :--- | :--- |
| **Surfaces & Borders** | `--ts-canvas`, `--ts-surface`, `--ts-surface-hover`, `--ts-border`, `--ts-border-strong` |
| **Text / Ink** | `--ts-ink`, `--ts-ink-secondary`, `--ts-ink-muted` |
| **Inverted & Semantic** | `--ts-inverted-bg`, `--ts-inverted-bg-hover`, `--ts-inverted-ink`, `--ts-accent`, `--ts-danger`, `--ts-danger-bg`, `--ts-danger-border`, `--ts-success` |
| **Code Blocks** | `--ts-code-bg`, `--ts-code-ink` |
| **Radius & Spacing** | `--ts-radius-sm` (`6px`), `--ts-radius-md` (`8px`), `--ts-space-1`..`--ts-space-5` (`4px`, `8px`, `12px`, `16px`, `20px`) |
| **Typography** | `--ts-font-sans`, `--ts-font-mono` |

### `.ts-*` Component Primitives Cheat Sheet

| Class | Layout / Behavior & Expected Usage |
| :--- | :--- |
| `.ts-workbench` | **Recommended semantic root grid container** (`max-width: var(--ts-wb-max-width, 1120px)`, `padding: 20px`, `gap: 16px`). Supports `data-width="narrow"` (`760px`) and `data-aside="left\|right"` (2-column responsive aside+main layout on `>= 761px` when a direct `.ts-wb-aside` child is present). |
| `.ts-wb-toolbar` | Direct child slot of `.ts-workbench` (`grid-column: 1 / -1`, space-between wrapping flex row) for search inputs, action buttons, and status badges. |
| `.ts-wb-nav` | Direct child slot of `.ts-workbench` (`grid-column: 1 / -1`, wrapping flex row) for `.ts-tabs` or category/view switchers. Themes can dock `.ts-wb-nav` to the left, right, or top via `ui.css`. |
| `.ts-wb-aside` | Direct child slot of `.ts-workbench` (`minmax(280px, 350px)` sidebar column on `>= 761px`, flex column `gap: 12px`) for form parameters or secondary controls. |
| `.ts-wb-main` | Direct child slot of `.ts-workbench` (primary content column, flex column `gap: 16px`). |
| `.ts-collection-group` / `.ts-collection` | Inside `.ts-wb-main`: `.ts-collection-group` stacks a section heading + collection (`gap: 8px`); `.ts-collection` renders a responsive card grid (`repeat(auto-fill, minmax(min(100%, var(--ts-collection-min, 340px)), 1fr))`) or 1-column list when `data-density="list"` or overridden by a terminal theme's `ui.css`. |
| `.ts-page` / `.ts-page-narrow` | Legacy centered flex column (`max-width: 1080px` / `760px`, `padding: 20px`, `gap: 16px`; prefer `.ts-workbench` for new tools). |
| `.ts-toolbar` | Flex row (`justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 8px`). |
| `.ts-row` / `.ts-stack` | `.ts-row`: wrapping horizontal flex (`gap: 8px`); `.ts-stack`: vertical flex (`gap: 12px`). |
| `.ts-split` | Responsive 2-column grid (`repeat(2, minmax(0, 1fr))`, `gap: 16px`, `align-items: start`); collapses to 1 column at `<= 720px`. |
| `.ts-grid` | Auto-fill card grid (`repeat(auto-fill, minmax(240px, 1fr))`, `gap: 12px`). |
| `.ts-card` / `.ts-card-header` | `.ts-card`: surface container (`padding: 16px`, `1px` border, `8px` radius, already flex column `gap: 12px`). `.ts-card-header`: space-between flex row inside a card. |
| `.ts-section-title`, `.ts-label`, `.ts-help`, `.ts-muted`, `.ts-empty` | Typography & dashed empty-state box (`13px`/`12px` sizes, muted text colors). |
| `.ts-standalone-only` / `.ts-standalone-header` | Hidden automatically when embedded (`:root[data-embed="true"]`); renders a bordered `18px` header when opened standalone. |
| `.ts-btn` (+ variants) | `34px` height button (`28px` with `.ts-btn-sm`). Variants: `.ts-btn-primary` (inverted), `.ts-btn-accent`, `.ts-btn-ghost`, `.ts-btn-danger`. |
| `.ts-input`, `.ts-textarea`, `.ts-select` | Full-width (`width: 100%`), `34px` height (`textarea` `min-height: 96px`), inherits `color-scheme` for `<input type="date">` and `<select>`. When placing an input beside buttons inside `.ts-row`, override with `width: auto; flex: 1; min-width: 0` or use CSS Grid so it does not wrap buttons to a new line. |
| `.ts-tabs` / `.ts-tab` | Shrink-to-fit `inline-flex` segmented bar (`34px` total height, matching `.ts-btn`/`.ts-input`): `<div class="ts-tabs"><button class="ts-tab active" type="button">A</button><button class="ts-tab" type="button">B</button></div>`. |
| `.ts-badge`, `.ts-badge-mono`, `.ts-kbd` | Compact inline pills (`11px`) and keyboard hint badges. |
| `.ts-code-row` / `.ts-code` | `.ts-code`: dark code block (`<pre class="ts-code">`). `.ts-code-row`: inline command strip `<div class="ts-code-row"><code>...</code><button class="ts-btn ts-btn-sm">复制</button></div>`. |
| `.ts-toast` | Managed automatically by `window.ToolStow.toast()` (fixed bottom-right, full-width on mobile). |

> There is no built-in `.ts-alert` or colored badge variant; build inline error/status banners using `var(--ts-danger)`, `var(--ts-danger-bg)`, `var(--ts-danger-border)`, or `var(--ts-success)` in Layer 2 CSS.

### `window.ToolStow` JS Helper Cheat Sheet

Provided automatically by `/ui/v1/theme.js` (zero `innerHTML`, zero `localStorage`):

| Method | Signature | Behavior |
| :--- | :--- | :--- |
| `getTheme` | `window.ToolStow.getTheme()` | Returns `{ mode: "light" \| "dark", accent: "#rrggbb", embedded: boolean, tokens: Record<string, string> }`. |
| `onThemeChange` | `window.ToolStow.onThemeChange(fn)` | Subscribes `fn(theme)` to live host `toolstow:theme` updates (and standalone OS theme changes); returns an unsubscribe function. |
| `copy` | `await window.ToolStow.copy(text, fallbackEl?)` | 3-tier sandbox-safe copy (`navigator.clipboard.writeText` -> temp `<textarea>` + `execCommand("copy")` -> optional DOM Range selection on `fallbackEl`). Returns `Promise<boolean>`. |
| `toast` | `window.ToolStow.toast(msg, durationMs?)` | Displays a monochrome `.ts-toast` notification using `textContent` (default `1800` ms). |

> **Local Preview Note**: Because `/ui/v1/toolstow.css` and `/ui/v1/theme.js` are served by the ToolStow host, opening `index.html` directly via `file://` will not load the SDK or inject `window.ToolStow`. To preview locally, copy your tool folder into a running ToolStow instance's `data/tools/<id>/` (hot-reloads automatically) and open `http://127.0.0.1:8080/#tool=<id>` or `/t/<id>/index.html`.

---

## 7. Publishing & Managing Tools via HTTP API

ToolStow separates human administration from machine/AI automation using two distinct keys configured on the server:

- **Always authenticate with `TOOLSTOW_API_KEY`** via the `Authorization: Bearer` header.
- **NEVER** pass the API key in URL query parameters.
- **NEVER** use `TOOLSTOW_ADMIN_KEY` in API scripts or `Authorization: Bearer` headers — `TOOLSTOW_ADMIN_KEY` is strictly for human login at `/admin` and is rejected as a Bearer token. Conversely, `TOOLSTOW_API_KEY` cannot log into `/admin`, modify site settings, or manage themes.
- If `TOOLSTOW_API_KEY` is unset on the host, Bearer authentication is disabled by default (fail-closed: `401` when `TOOLSTOW_ADMIN_KEY` is set, or `404` when both keys are unset). Failed attempts are rate-limited (5 failures / 5 minutes -> `429`).

### Recommended: Publish with `scripts/publish.py`

```bash
export TOOLSTOW_URL="http://127.0.0.1:8080"
export TOOLSTOW_API_KEY="<your-api-key>"

# Validates, packs in memory, and uploads with ?overwrite=false
python3 skills/toolstow-tool/scripts/publish.py ./my-tool

# Overwrite an existing tool with the same ID
python3 skills/toolstow-tool/scripts/publish.py ./my-tool --overwrite

# Load TOOLSTOW_URL / TOOLSTOW_API_KEY from a .env file
python3 skills/toolstow-tool/scripts/publish.py ./my-tool --env-file ./.env
```

### Raw `curl` Reference (List / Draft Toggle / Delete)

```bash
# List all installed tools (including drafts)
curl -sS -H "Authorization: Bearer $TOOLSTOW_API_KEY" \
  "$TOOLSTOW_URL/api/admin/tools"

# Hide tool as draft
curl -i -X PATCH "$TOOLSTOW_URL/api/admin/tools/my-tool" \
  -H "Authorization: Bearer $TOOLSTOW_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"draft": true}'

# Publish tool publicly
curl -i -X PATCH "$TOOLSTOW_URL/api/admin/tools/my-tool" \
  -H "Authorization: Bearer $TOOLSTOW_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"draft": false}'

# Delete a tool (returns 204 No Content)
curl -i -X DELETE "$TOOLSTOW_URL/api/admin/tools/my-tool" \
  -H "Authorization: Bearer $TOOLSTOW_API_KEY"
```

---

## 8. Pre-Flight Checklist for AI Agents

Before delivering or publishing a generated tool:

- [ ] Run `python3 skills/toolstow-tool/scripts/validate.py <tool_dir>` with **0 errors**.
- [ ] `manifest.json` has valid `id`, `name`, `runtime: "static"`, `themeAware: true`, and appropriate `"draft"` flag.
- [ ] Entry HTML loads `<link rel="stylesheet" href="/ui/v1/toolstow.css">` and `<script src="/ui/v1/theme.js"></script>` in `<head>` with no external CDN URLs.
- [ ] UI chrome reuses `.ts-*` classes and `var(--ts-*)` tokens, with no duplicate top `<h1>` inside iframe (standalone headers wrapped in `.ts-standalone-only`).
- [ ] Zero unguarded `localStorage` / `sessionStorage` / `document.cookie`, zero `innerHTML` / `outerHTML` / `insertAdjacentHTML` / `document.write`, zero `allow-same-origin`.
- [ ] Clipboard actions use `window.ToolStow.copy(text, fallbackEl)` and `window.ToolStow.toast(msg)`.
- [ ] Package with `scripts/pack.py` or publish with `scripts/publish.py` (never using `TOOLSTOW_ADMIN_KEY`).
