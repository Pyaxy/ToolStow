# ToolStow AI Tool Development & Publishing Skill

Use this specification whenever generating, packaging, or publishing a self-contained static web micro-tool for **ToolStow** (v0.2+).

---

## 1. Core Principles

- **Zero-build & self-contained**: Output plain HTML, CSS, and vanilla JS (a single `index.html` is ideal for small utilities; split local relative assets only when needed). Avoid external CDN dependencies so the tool works offline and inside strict sandboxes.
- **Opaque-origin safe**: Every tool runs inside a dual sandbox (`<iframe sandbox="...">` + `Content-Security-Policy: sandbox ...`) with an opaque origin (`Origin: null`). Never assume same-origin browser storage is available.
- **Shared UI SDK (`/ui/v1/`)**: Every native ToolStow plugin MUST load `/ui/v1/toolstow.css` and `/ui/v1/theme.js` in `<head>` to inherit the host's minimalist monochrome design system, automatic Light/Dark & accent sync, zero-FOUC first paint, and any `data/theme/ui.css` site-wide reskin automatically.
- **No duplicate top headers inside iframes**: The ToolStow shell already renders the active tool's icon, title, and description in its top bar. Start directly with functional controls; wrap any standalone header in `.ts-standalone-only`.
- **Draft-friendly publishing**: Tools support a Hugo-style `"draft": true` flag so AI agents can publish work-in-progress tools for admin preview before making them public.

---

## 2. Directory & ZIP Packaging

On disk, each tool lives inside the host's data directory at `<data-dir>/tools/{manifest.id}/` (default `./data/tools/{manifest.id}/`):

```text
my-tool/
├── manifest.json   # Required
├── index.html      # Default entry file
└── assets/         # Optional relative assets (CSS, JS, SVG, PNG, etc.)
```

### ZIP Package Formats (Both Supported)

1. **Flat ZIP (recommended)**: `manifest.json` and `index.html` sit directly at the archive root:
   ```bash
   (cd my-tool && zip -r ../my-tool.zip .)
   ```
2. **Single-root-folder ZIP**: A single top-level directory contains `manifest.json` (ToolStow automatically ignores `__MACOSX/` and `.DS_Store`, strips the single wrapper folder, and installs into `<data-dir>/tools/{manifest.id}/`):
   ```bash
   zip -r my-tool.zip my-tool/
   ```

### Packaging Limits & Security Rules

- **Max upload size**: 50 MB default (configurable on host via `--max-upload-mb` / `TOOLSTOW_MAX_UPLOAD_MB`).
- **Max uncompressed size**: 200 MB total.
- **Max file count**: 1,000 files per archive.
- **Forbidden entries**: No symlinks, no `..` path traversal, no absolute paths (`/`), no Windows drive prefixes or backslashes (`\`), no duplicate normalized paths. Dot-prefixed entries at the top level (e.g. `.DS_Store`) are ignored/reserved.

---

## 3. `manifest.json` Schema

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
| `category` | `string` | No | `"未分类"` | Sidebar grouping label (trimmed; empty string falls back to `"未分类"`). |
| `tags` | `string[]` | No | `[]` | Search and filter tags (deduplicated; empty strings ignored). |
| `icon` | `string` | No | `"🛠️"` | Emoji string, or a safe relative image path inside the tool directory (e.g. `"assets/icon.svg"`). Must exist if path-like. |
| `entry` | `string` | No | `"index.html"` | Safe relative path to an existing regular HTML file (no leading `/`, `..`, `\`, or URL scheme). |
| `runtime` | `"static"` | No | `"static"` | Only `"static"` is currently supported. Declaring `"script"` is rejected. |
| `draft` | `boolean` | No | `false` | Hugo-style visibility flag. When `true`, the tool is hidden from public `GET /api/tools` and returns `404` on `/t/{id}/*` for anonymous visitors; visible only to an authenticated Admin session or when the host runs with `--include-drafts`. |
| `themeAware` | `boolean` | No | `false` | Set `true` for plugins using `/ui/v1/theme.js` (the host also syncs automatically once `toolstow:ready` is received). |
| `sandbox` | `string[]` | No | `["allow-scripts", "allow-downloads", "allow-forms"]` | Requested iframe sandbox tokens, filtered by the host allowlist. |

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

## 4. Sandbox Limitations & Defensive Patterns

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

## 5. Shared UI SDK (`/ui/v1/toolstow.css` + `/ui/v1/theme.js`)

ToolStow serves a locked shared UI SDK at `/ui/v1/toolstow.css` and `/ui/v1/theme.js` (also shared by the host shell and `/admin`). When the host operator places an optional `data/theme/ui.css`, ToolStow automatically appends it to `/ui/v1/toolstow.css`, reskinning the host and every plugin in unison.

### Mandatory `<head>` Includes

Every native ToolStow plugin MUST include both lines in `<head>` (do **NOT** copy-paste `:root` color palettes or manual `postMessage` theme listeners):

```html
<link rel="stylesheet" href="/ui/v1/toolstow.css">
<script src="/ui/v1/theme.js"></script>
```

> Load `<script src="/ui/v1/theme.js"></script>` synchronously in `<head>` (without `defer`/`async`) so it sets `data-embed` and `data-theme` from the initial query parameters (`?theme=...&accent=...`) or OS preference before first paint, eliminating dark-mode white flash (FOUC).

### 3-Layer UI Design Boundary

Follow this strict 3-layer separation so plugins feel native to ToolStow without constraining tool functionality:

1. **Layer 1 — Shared controls & chrome (MUST reuse SDK)**:
   - Use `.ts-*` primitives for pages, cards, buttons, inputs, selects, textareas, tabs, badges, code blocks, empty states, and toasts.
   - Reference `var(--ts-*)` tokens for any custom border, surface, text, radius, spacing, or font styling.
   - **NEVER** hardcode UI chrome hex/rgb colors, decorative background gradients, heavy drop shadows, or custom external web fonts.
2. **Layer 2 — Functional layout (MAY write minimal scoped CSS)**:
   - Plugins MAY add a small `<style>` block for tool-specific layout structure: CSS Grid/Flex templates, panel widths, responsive media queries, and spacing using `var(--ts-space-*)`.
3. **Layer 3 — Domain content visuals (FREE domain colors)**:
   - Canvas/SVG artwork, color pickers/swatches, image previews, charts, syntax highlighting, and diagram payloads may freely use domain-specific colors.

### No Duplicate Top Headers Inside Iframes

- **FORBIDDEN inside the ToolStow shell**: Do NOT render a persistent top banner/hero/title `<h1>` repeating the tool name already shown in the host shell's top bar. Start directly with functional controls (search bar, toolbar, form inputs, split workspace).
- **Standalone header pattern**: If you want a page title when the user opens the tool directly in a new tab at `/t/{id}/index.html`, wrap it in `.ts-standalone-only` (optionally paired with `.ts-standalone-header`):
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
    /* Layer 2: minimal tool-specific layout only */
    .io-area {
      min-height: 220px;
      font-family: var(--ts-font-mono);
    }
  </style>
</head>
<body>
  <div class="ts-page">
    <header class="ts-standalone-only ts-standalone-header">
      <h1>🛠️ 示例微工具</h1>
    </header>

    <div class="ts-toolbar">
      <button id="run-btn" class="ts-btn ts-btn-primary" type="button">转换</button>
      <button id="copy-btn" class="ts-btn" type="button">复制结果</button>
      <span id="meta-badge" class="ts-badge ts-badge-mono">0 字符</span>
    </div>

    <div class="ts-split">
      <section class="ts-card ts-stack">
        <div class="ts-card-header">
          <h2 class="ts-section-title">输入</h2>
        </div>
        <textarea id="input" class="ts-textarea io-area" placeholder="在此粘贴内容..."></textarea>
      </section>

      <section class="ts-card ts-stack">
        <div class="ts-card-header">
          <h2 class="ts-section-title">输出</h2>
        </div>
        <pre id="output" class="ts-code io-area"></pre>
      </section>
    </div>
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
      // e.g. redraw canvas using mode ("light" | "dark") and accent ("#rrggbb")
      void mode;
      void accent;
    });
  </script>
</body>
</html>
```

### CSS Tokens Cheat Sheet (`var(--ts-*)`)

| Category | Tokens |
| :--- | :--- |
| **Surfaces & Borders** | `--ts-canvas`, `--ts-surface`, `--ts-surface-hover`, `--ts-border`, `--ts-border-strong` |
| **Text / Ink** | `--ts-ink`, `--ts-ink-secondary`, `--ts-ink-muted` |
| **Inverted & Semantic** | `--ts-inverted-bg`, `--ts-inverted-ink`, `--ts-accent`, `--ts-danger`, `--ts-success` |
| **Code Blocks** | `--ts-code-bg`, `--ts-code-ink` |
| **Radius & Spacing** | `--ts-radius-sm` (`6px`), `--ts-radius-md` (`8px`), `--ts-space-1`..`--ts-space-5` (`4px`..`20px`) |
| **Typography** | `--ts-font-sans`, `--ts-font-mono` |

### `.ts-*` Component Primitives Cheat Sheet

| Group | Classes |
| :--- | :--- |
| **Page & Layout** | `.ts-page`, `.ts-page-narrow`, `.ts-toolbar`, `.ts-row`, `.ts-stack`, `.ts-split`, `.ts-grid` |
| **Containers & Text** | `.ts-card`, `.ts-card-header`, `.ts-section-title`, `.ts-label`, `.ts-help`, `.ts-muted`, `.ts-empty` |
| **Standalone Header** | `.ts-standalone-only` (hidden when `data-embed="true"`), `.ts-standalone-header` |
| **Buttons** | `.ts-btn`, `.ts-btn-primary`, `.ts-btn-accent`, `.ts-btn-ghost`, `.ts-btn-danger`, `.ts-btn-sm` |
| **Forms** | `.ts-input`, `.ts-textarea`, `.ts-select` |
| **Tabs, Badges & Code** | `.ts-tabs`, `.ts-tab` (`.active`), `.ts-badge`, `.ts-badge-mono`, `.ts-kbd`, `.ts-code-row`, `.ts-code`, `.ts-toast` |

### `window.ToolStow` JS Helper Cheat Sheet

Provided automatically by `/ui/v1/theme.js` (zero `innerHTML`, zero `localStorage`):

| Method | Signature | Behavior |
| :--- | :--- | :--- |
| `getTheme` | `window.ToolStow.getTheme()` | Returns `{ mode: "light" \| "dark", accent: "#rrggbb", embedded: boolean, tokens: Record<string, string> }`. |
| `onThemeChange` | `window.ToolStow.onThemeChange(fn)` | Subscribes `fn(theme)` to live host `toolstow:theme` updates (and standalone OS theme changes); returns an unsubscribe function. |
| `copy` | `await window.ToolStow.copy(text, fallbackEl?)` | 3-tier sandbox-safe copy (`navigator.clipboard.writeText` -> temp `<textarea>` + `execCommand("copy")` -> optional DOM Range selection on `fallbackEl`). Returns `Promise<boolean>`. |
| `toast` | `window.ToolStow.toast(msg, durationMs?)` | Displays a monochrome `.ts-toast` notification using `textContent` (default `1800` ms). |

---

## 6. Publishing & Managing Tools via HTTP API (`curl`)

ToolStow separates human administration from machine/AI automation using two distinct keys configured on the server:

- **Always authenticate with `TOOLSTOW_API_KEY`** via the `Authorization: Bearer` header:
  ```bash
  export TOOLSTOW_URL="http://127.0.0.1:8080"
  export TOOLSTOW_API_KEY="<your-api-key>"
  ```
- **NEVER** pass the API key in URL query parameters.
- **NEVER** use `TOOLSTOW_ADMIN_KEY` in API scripts or `Authorization: Bearer` headers — `TOOLSTOW_ADMIN_KEY` is strictly for human login at `/admin` and is rejected as a Bearer token. Conversely, `TOOLSTOW_API_KEY` cannot log into `/admin` or modify site settings (`/api/admin/settings`).
- If `TOOLSTOW_API_KEY` is unset on the host, Bearer authentication is disabled by default (fail-closed: returns `401` when `TOOLSTOW_ADMIN_KEY` is set, or `404` when both `TOOLSTOW_ADMIN_KEY` and `TOOLSTOW_API_KEY` are unset). Failed attempts are rate-limited (5 failures / 5 minutes -> `429`).

### List All Installed Tools (Including Drafts)

```bash
curl -sS \
  -H "Authorization: Bearer $TOOLSTOW_API_KEY" \
  "$TOOLSTOW_URL/api/admin/tools"

# Optional filtering by keyword, category, or tag:
curl -sS \
  -H "Authorization: Bearer $TOOLSTOW_API_KEY" \
  "$TOOLSTOW_URL/api/admin/tools?q=docker&category=运维&tag=cli"
```

*(Note: Public unauthenticated `GET /api/tools` returns only published tools with `draft: false`.)*

### Upload or Overwrite a Tool (`.zip`)

Re-uploading a ZIP with the same `manifest.id` atomically replaces the existing tool directory and refreshes the registry immediately (no restart required):

```bash
# Package
zip -r my-tool.zip my-tool/

# Upload (returns HTTP 201 on create, HTTP 200 on overwrite update)
curl -i -X POST "$TOOLSTOW_URL/api/admin/tools/upload" \
  -H "Authorization: Bearer $TOOLSTOW_API_KEY" \
  -F "file=@my-tool.zip"

# Optional safe create-only check (returns HTTP 409 TOOL_EXISTS with existing/incoming metadata if ID exists)
curl -i -X POST "$TOOLSTOW_URL/api/admin/tools/upload?overwrite=false" \
  -H "Authorization: Bearer $TOOLSTOW_API_KEY" \
  -F "file=@my-tool.zip"
```

### Toggle Draft / Published Status

Atomically updates `"draft"` inside `manifest.json` on disk and refreshes the live registry immediately (returns HTTP `200` with the updated tool metadata, or `404` if not found):

```bash
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
```

### Delete a Tool

```bash
# Returns HTTP 204 No Content on success, 404 if not found
curl -i -X DELETE "$TOOLSTOW_URL/api/admin/tools/my-tool" \
  -H "Authorization: Bearer $TOOLSTOW_API_KEY"
```

---

## 7. Pre-Flight Checklist for AI Agents

Before packaging and publishing a generated tool, verify:

- [ ] `manifest.json` is valid JSON, `id` matches `^[a-z0-9]+(?:-[a-z0-9]+)*$`, `runtime` is `"static"`, and `themeAware` is `true`.
- [ ] `"draft"` is set appropriately (`false` to publish immediately, or `true` to stage for admin review first).
- [ ] `entry` loads `<link rel="stylesheet" href="/ui/v1/toolstow.css">` and `<script src="/ui/v1/theme.js"></script>` in `<head>`, with no external CDN URLs.
- [ ] UI controls and chrome reuse `.ts-*` classes and `var(--ts-*)` tokens (no hardcoded UI chrome colors, no duplicate top `<h1>` header inside iframe; standalone headers wrapped in `.ts-standalone-only`).
- [ ] Zero unguarded `localStorage`, `sessionStorage`, `document.cookie`, or raw `innerHTML` usage.
- [ ] Clipboard actions use `window.ToolStow.copy(text, fallbackEl)` and `window.ToolStow.toast(msg)`.
- [ ] If the tool exports files, `"allow-downloads"` is included in `sandbox`.
- [ ] API calls use `/api/admin/tools*` with `-H "Authorization: Bearer $TOOLSTOW_API_KEY"` (never query params, never `TOOLSTOW_ADMIN_KEY`).
