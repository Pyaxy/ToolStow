# ToolStow AI Tool Development & Publishing Skill

Use this specification whenever generating, packaging, or publishing a self-contained static web micro-tool for **ToolStow**.

---

## 1. Core Principles

- **Zero-build & self-contained**: Output plain HTML, CSS, and vanilla JS (single `index.html` is ideal for small utilities; split local relative assets only when needed). Avoid external CDN dependencies so the tool works offline and inside strict sandboxes.
- **Opaque-origin safe**: Every tool runs inside a dual sandbox (`<iframe sandbox="...">` + `Content-Security-Policy: sandbox ...`) with an opaque origin (`Origin: null`). Never assume same-origin browser storage is available.
- **Standalone-ready & theme-aware**: Provide default `:root` CSS variables so the tool looks complete when opened directly at `/t/{id}/index.html`, and subscribe to `toolstow:theme` to mirror the host's Light/Dark mode and accent color in real time.

---

## 2. Directory & ZIP Packaging

On disk, each tool lives in `tools/{manifest.id}/`:

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
2. **Single-root-folder ZIP**: A single top-level directory contains `manifest.json` (ToolStow automatically ignores `__MACOSX/` and `.DS_Store`, strips the single wrapper folder, and installs into `tools/{manifest.id}/`):
   ```bash
   zip -r my-tool.zip my-tool/
   ```

### Packaging Limits & Security Rules

- **Max upload size**: 50 MB default (configurable on host via `--max-upload-mb`).
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
| `runtime` | `"static"` | No | `"static"` | Phase 1 only supports `"static"`. Declaring `"script"` is rejected. |
| `themeAware` | `boolean` | No | `false` | Set `true` if the tool listens for `toolstow:theme` messages. |
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
2. **Clipboard API fallback**
   - `navigator.clipboard.writeText()` may reject inside a sandboxed iframe or non-HTTPS origin. Always provide a fallback using a temporary `<textarea>` + `document.execCommand("copy")`, and degrade gracefully if both are blocked.
3. **File downloads**
   - Include `"allow-downloads"` in `manifest.json`'s `sandbox` array, and export files via `Blob` + `URL.createObjectURL()` + `<a download="filename">`.
4. **No calls to ToolStow host write APIs**
   - The host rejects `Origin: null` and cross-site requests on `POST /api/tools/upload` and `DELETE /api/tools/{id}`. Tools must not attempt to mutate host state.
5. **Safe DOM rendering**
   - Prefer `textContent` and DOM element builders over `innerHTML` to avoid XSS when rendering user input.

---

## 5. Theme Synchronization (`toolstow:theme` & `toolstow:ready`)

When `themeAware: true` is declared in `manifest.json`:

1. The host posts `{ type: "toolstow:theme", payload: { mode: "light" | "dark", accent: "#rrggbb" } }` whenever the iframe loads, when the tool posts `{ type: "toolstow:ready" }`, and whenever the user toggles Light/Dark mode or changes the accent color.
2. Because the tool runs in an opaque origin, `window.parent.postMessage({ type: "toolstow:ready" }, "*")` must use `"*"` as `targetOrigin`.

### Copy-Paste Boilerplate

```html
<style>
  :root {
    --accent: #2563eb;
    --bg: #f8fafc;
    --surface: #ffffff;
    --text: #0f172a;
    --muted: #64748b;
    --border: #e2e8f0;
  }

  :root[data-theme="dark"] {
    --bg: #0f172a;
    --surface: #1e293b;
    --text: #f1f5f9;
    --muted: #94a3b8;
    --border: #334155;
  }

  body {
    margin: 0;
    background: var(--bg);
    color: var(--text);
    font-family: system-ui, -apple-system, sans-serif;
  }
</style>

<script>
  function applyToolStowTheme(payload) {
    if (!payload || typeof payload !== "object") return;
    if (payload.mode === "dark" || payload.mode === "light") {
      document.documentElement.dataset.theme = payload.mode;
    }
    if (typeof payload.accent === "string" && /^#[0-9a-fA-F]{3,8}$/.test(payload.accent)) {
      document.documentElement.style.setProperty("--accent", payload.accent);
    }
  }

  window.addEventListener("message", (event) => {
    if (event.data && event.data.type === "toolstow:theme") {
      applyToolStowTheme(event.data.payload);
    }
  });

  // Fallback for standalone viewing (/t/{id}/index.html)
  if (window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches) {
    document.documentElement.dataset.theme = "dark";
  }

  // Handshake so the host immediately sends the current theme
  if (window.parent && window.parent !== window) {
    window.parent.postMessage({ type: "toolstow:ready" }, "*");
  }
</script>
```

---

## 6. Publishing & Updating via HTTP API (`curl`)

ToolStow defaults to `http://127.0.0.1:8080`. Local CLI / Agent requests without an `Origin` header are accepted directly (browser frontend requests additionally send `X-ToolStow-Client: 1`).

### Upload or Overwrite a Tool

Re-uploading a ZIP with the same `manifest.id` atomically replaces the existing tool and refreshes the registry immediately (no restart required):

```bash
# Package
zip -r my-tool.zip my-tool/

# Upload (returns HTTP 201 on create, HTTP 200 on overwrite update)
curl -i -X POST http://127.0.0.1:8080/api/tools/upload \
  -F "file=@my-tool.zip"
```

### List / Filter Installed Tools

```bash
curl -s "http://127.0.0.1:8080/api/tools"
curl -s "http://127.0.0.1:8080/api/tools?q=docker&category=运维&tag=cli"
```

### Delete a Tool

```bash
# Returns HTTP 204 No Content on success, 404 if not found
curl -i -X DELETE http://127.0.0.1:8080/api/tools/my-tool
```

---

## 7. Pre-Flight Checklist for AI Agents

Before packaging and uploading a generated tool, verify:

- [ ] `manifest.json` is valid JSON, `id` matches `^[a-z0-9]+(?:-[a-z0-9]+)*$`, and `runtime` is `"static"`.
- [ ] `entry` exists inside the tool folder (e.g. `index.html`) and uses no external CDN URLs unless strictly required.
- [ ] Zero unguarded `localStorage`, `sessionStorage`, or `document.cookie` calls.
- [ ] If `themeAware: true`, default `:root` variables are defined, `toolstow:theme` is handled, and `toolstow:ready` is posted to `window.parent`.
- [ ] If the tool exports files, `"allow-downloads"` is included in `sandbox`.
- [ ] If the tool copies text, a clipboard fallback is implemented.
