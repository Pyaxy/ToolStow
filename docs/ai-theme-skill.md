# ToolStow AI Theme Development Skill

Use this specification whenever designing, implementing, or packaging a custom public frontend theme for **ToolStow** (v0.2.2+).

ToolStow themes are self-contained static `.zip` packages installed via `/admin` (into `<data-dir>/themes/<short>/`). All visual styling — colors, accent, typography, radii, light/dark palettes, and control appearance — is owned **entirely by the theme** (there is no global accent picker in ToolStow).

---

## 1. Choose a Theme Mode

ToolStow supports **two theme modes** under the exact same `theme.json` schema:

| Mode | Required Files | When to Use |
| :--- | :--- | :--- |
| **Skin-Only Theme** *(fastest)* | `theme.json` + `ui.css` (+ optional `preview.svg`) | You want to keep ToolStow's built-in responsive sidebar/gallery shell (`frontend/`), and reskin **both the host shell and every plugin** purely through CSS variables and `.ts-*` overrides. Zero HTML/JS required! |
| **Full Custom Shell Theme** | `theme.json` + `index.html` (or `dist/index.html`) + `ui.css` (+ local CSS/JS & optional `preview.svg`) | You want a completely custom public layout (e.g., terminal TUI, compact table dashboard, card mosaic, React/Vite static build in `dist/`) while still reskinning plugins via `ui.css`. |

> **Validation rule**: A valid theme archive MUST contain at least one of `ui.css`, `index.html`, or `dist/index.html`.

---

## 2. Directory & ZIP Structure

### Skin-Only Theme Layout

```text
my-skin/
├── theme.json      # Required metadata
├── ui.css          # Automatically appended to GET /ui/v1/toolstow.css
└── preview.svg     # Optional 16:9 static preview (NO <script>!)
```

### Full Custom Shell Theme Layout

```text
my-theme/
├── theme.json      # Required metadata
├── ui.css          # Recommended: reskins plugins + shared /ui/v1/toolstow.css controls
├── preview.svg     # Optional 16:9 static preview (or preview.png)
├── index.html      # Public shell entry (OR place built SPA under dist/index.html)
├── style.css       # Shell-specific layout styles
└── app.js          # Shell logic talking to /api/me & /api/tools
```

> If `dist/index.html` exists, ToolStow serves static public assets from `dist/`; otherwise if `index.html` exists at the theme root, it serves from the theme root. Always reference local assets with relative or root paths (e.g. `./style.css`, `./app.js`).

### Packaging as `.zip`

Both flat ZIPs (`theme.json` at archive root) and single-wrapper-folder ZIPs (`my-theme/theme.json`) are supported (`__MACOSX/` and `.DS_Store` are automatically ignored):

```bash
# Flat ZIP (recommended)
(cd my-theme && zip -r ../my-theme.zip .)
```

- Limits: max upload 50 MB (configurable), max 200 MB uncompressed, max 1,000 files; no symlinks, no `..` traversal, no duplicate normalized paths.

---

## 3. `theme.json` Schema

```json
{
  "short": "nord-desk",
  "name": "Nord Desk",
  "version": "1.0.0",
  "description": "Cool Arctic slate palette with crisp cards for host and plugins",
  "author": "Your Name",
  "url": "https://github.com/yourname/toolstow-theme-nord-desk",
  "preview": "preview.svg"
}
```

| Field | Type | Required | Default | Validation & Semantics |
| :--- | :--- | :--- | :--- | :--- |
| `short` | `string` | **Yes** | — | Unique theme ID matching `^[a-z0-9]+(?:-[a-z0-9]+)*$` (1–64 chars). **`"default"` is reserved** for the built-in monochrome theme and will be rejected. |
| `name` | `string` | **Yes** | — | Display title shown in `/admin` (non-empty after trimming). |
| `version` | `string` | No | `"0.1.0"` | Display version string. |
| `description` | `string` | No | `""` | Short summary shown in `/admin`. |
| `author` | `string` | No | `""` | Author label shown in `/admin`. |
| `url` | `string` | No | `""` | Optional homepage/repo link (`http://` or `https://` only). |
| `preview` | `string` | No | `null` | Safe relative path to an existing regular image file (e.g. `"preview.svg"` or `"preview.png"`). Served by `/admin` under `Content-Security-Policy: sandbox;` and `X-Content-Type-Options: nosniff`. For `.svg`, **never include `<script>` or inline `on*` event attributes**. |

---

## 4. Styling Host & Plugins Together (`ui.css`)

ToolStow serves a locked shared design stylesheet at **`/ui/v1/toolstow.css`**. Whenever your theme is active and includes `ui.css`, ToolStow automatically appends your `ui.css` to the end of `/ui/v1/toolstow.css` (`Cache-Control: no-cache`).

That means defining your Light and Dark tokens in `ui.css` automatically reskins **every installed plugin** (and the built-in shell in skin-only mode) without touching any plugin code!

### Canonical `--ts-*` Design Tokens

Always define **both** `:root` (light mode) and `:root[data-theme="dark"]` (dark mode):

```css
/* ui.css */
:root {
  --ts-canvas: #faf7f2;
  --ts-surface: #f4efe6;
  --ts-surface-hover: #ebe4d8;
  --ts-border: #e5dccf;
  --ts-border-strong: #cfc3b2;
  --ts-ink: #1c1917;
  --ts-ink-secondary: #44403c;
  --ts-ink-muted: #78716c;
  --ts-inverted-bg: #1c1917;
  --ts-inverted-bg-hover: #292524;
  --ts-inverted-ink: #faf7f2;
  --ts-accent: #b45309;
  --ts-danger: #dc2626;
  --ts-danger-bg: rgba(220, 38, 38, 0.08);
  --ts-danger-border: rgba(220, 38, 38, 0.35);
  --ts-success: #15803d;
  --ts-code-bg: #1c1917;
  --ts-code-ink: #faf7f2;
  --ts-radius-sm: 8px;
  --ts-radius-md: 10px;
  --ts-font-sans: Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  --ts-font-mono: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

:root[data-theme="dark"] {
  --ts-canvas: #1c1917;
  --ts-surface: #262220;
  --ts-surface-hover: #302b28;
  --ts-border: #3a3430;
  --ts-border-strong: #524a44;
  --ts-ink: #f5f2ec;
  --ts-ink-secondary: #d6d0c6;
  --ts-ink-muted: #a8a096;
  --ts-inverted-bg: #f5f2ec;
  --ts-inverted-bg-hover: #e5dfd5;
  --ts-inverted-ink: #1c1917;
  --ts-accent: #f59e0b;
  --ts-danger: #f87171;
  --ts-code-bg: #12100e;
  --ts-code-ink: #f5f2ec;
}
```

You may also optionally override shared plugin primitives (`.ts-card`, `.ts-btn`, `.ts-btn-primary`, `.ts-input`, `.ts-tab`, `.ts-code-row`, etc.) inside `ui.css` to customize control feel across all plugins.

> **Important**: Never write inline `document.documentElement.style.setProperty("--ts-accent", ...)` in your shell JS — let `ui.css` own `--ts-accent` for light and dark modes, and read the active value via `getComputedStyle(document.documentElement).getPropertyValue("--ts-accent")` when syncing to plugin iframes.

---

## 5. Public API Contract (For Full Custom Shell Themes)

ToolStow locks `/api/*`, `/admin*`, `/t/*`, and `/ui/*` on the backend. A custom shell only needs **two read-only same-origin endpoints**:

### 5.1 `GET /api/me`

Fetch with `{ credentials: "same-origin", cache: "no-store" }`:

```json
{
  "siteTitle": "ToolStow",
  "siteDescription": "Personal Micro-App Toolbox",
  "isAdmin": false,
  "adminEnabled": true,
  "activeTheme": "nord-desk",
  "themeSource": "custom"
}
```

- Render `siteTitle` (and `document.title`) and `siteDescription`.
- **Conditional Admin Link (Mandatory)**: Show a link to `/admin` (e.g. `管理 / 设置`) **if and ONLY if `isAdmin === true`**. Keep it strictly hidden (`hidden` with `[hidden] { display: none !important; }`) for anonymous visitors!
- **Read-only public shell**: Never render upload or delete controls on `/`.

### 5.2 `GET /api/tools`

Optional query parameters: `?q=<keyword>&category=<category>&tag=<tag>` (or fetch once with no params and filter client-side). Returns an array of published (`draft: false`) tools:

```json
[
  {
    "id": "docker-cheatsheet",
    "name": "Docker 常用命令速查",
    "version": "1.0.0",
    "description": "Docker 容器、镜像、Compose、日志排查与空间清理常用命令",
    "category": "运维",
    "tags": ["docker", "devops", "cli"],
    "icon": "🐳",
    "iconIsPath": false,
    "iconUrl": null,
    "entry": "index.html",
    "entryUrl": "/t/docker-cheatsheet/index.html",
    "runtime": "static",
    "draft": false,
    "themeAware": true,
    "sandbox": ["allow-scripts", "allow-forms"]
  }
]
```

---

## 6. Hosting Plugins & Syncing Theme to Iframes (Mandatory for Full Shells)

Every full custom shell MUST follow these rules when loading a plugin:

1. **Load `/ui/v1/toolstow.css` FIRST in `<head>`** before your own `style.css`, so the shell inherits base tokens + your `ui.css` automatically.
2. **Validate URLs & Sandbox Strictly**:
   - Only load `tool.entryUrl` if it starts with `"/t/"` and not `"//"`.
   - Only render `tool.iconUrl` (when `tool.iconIsPath === true`) as `<img src="...">` if it starts with `"/t/"` and not `"//"`; otherwise render `tool.icon` via `textContent`.
   - Filter `tool.sandbox` through the strict allowlist (`allow-scripts`, `allow-downloads`, `allow-forms`, `allow-modals`, `allow-popups`, `allow-popups-to-escape-sandbox`), preserving `[]` when `Array.isArray(tool.sandbox)` and **NEVER adding `allow-same-origin` or `allow-top-navigation*`**.
3. **Anti-FOUC Initial Iframe URL**:
   - Append `?theme=<mode>&accent=<encodedComputedAccent>` when setting `iframe.src` (preserving existing query/hash on `entryUrl`), so `/ui/v1/theme.js` applies dark/light mode synchronously on frame 1 without a white flash.
4. **Live `postMessage` Handshake**:
   - Listen for `{ type: "toolstow:ready" }` from `state.activeIframe.contentWindow` and mark the active iframe ready.
   - Send `toolstow:theme` on iframe `load`, on `toolstow:ready`, and whenever the user toggles Light/Dark mode (sync whenever `tool.themeAware === true` **OR** `activeIframeReady === true`):
     ```js
     const SYNCED_TOKENS = [
       "--ts-canvas", "--ts-surface", "--ts-surface-hover", "--ts-border", "--ts-border-strong",
       "--ts-ink", "--ts-ink-secondary", "--ts-ink-muted",
       "--ts-inverted-bg", "--ts-inverted-bg-hover", "--ts-inverted-ink",
       "--ts-accent", "--ts-danger", "--ts-danger-bg", "--ts-danger-border",
       "--ts-success", "--ts-code-bg", "--ts-code-ink",
       "--ts-radius-sm", "--ts-radius-md",
       "--ts-space-1", "--ts-space-2", "--ts-space-3", "--ts-space-4", "--ts-space-5",
       "--ts-font-sans", "--ts-font-mono",
     ];

     function syncThemeToIframe(iframe, mode) {
       if (!iframe || !iframe.contentWindow) return;
       const cs = getComputedStyle(document.documentElement);
       const tokens = {};
       for (const name of SYNCED_TOKENS) {
         const v = cs.getPropertyValue(name).trim();
         if (v) tokens[name] = v;
       }
       const accent = tokens["--ts-accent"] || (mode === "dark" ? "#fafafa" : "#18181b");
       iframe.contentWindow.postMessage(
         { type: "toolstow:theme", payload: { mode, accent, tokens } },
         "*"
       );
     }
     ```
5. **Security & UX Essentials**:
   - **Zero `innerHTML` / `outerHTML` / `insertAdjacentHTML`**: render all tool/site strings with `textContent` and safe DOM builders.
   - Support Light/Dark mode toggle (persisted in host `localStorage` under `"toolstow:mode"`), `Cmd/Ctrl+K` search focus, `#tool=<id>` URL hash navigation, Reload tool, Open Standalone (`target="_blank" rel="noopener noreferrer"`), and responsive mobile layout.

---

## 7. Reference Implementations & Verification Checklist

Inspect the 3 bundled themes in `examples/themes/` as working references:
- **Skin-only**: `examples/themes/paper-warm/` (`theme.json`, `ui.css`, `preview.svg`)
- **Full TUI shell**: `examples/themes/ascii-probe/`
- **Full table ops shell**: `examples/themes/serverstatus/`

Before packaging your theme `.zip`, verify:
- [ ] `theme.json` has valid `short` (`^[a-z0-9]+(?:-[a-z0-9]+)*$`, not `"default"`) and non-empty `name`.
- [ ] Contains `ui.css` (with both `:root` and `:root[data-theme="dark"]`) and/or `index.html` (`dist/index.html`).
- [ ] `preview.svg` (if included) is 16:9 and contains zero `<script>` or `on*` event attributes.
- [ ] Full shell loads `/ui/v1/toolstow.css` first, shows `/admin` strictly when `me.isAdmin === true`, has zero upload/delete UI, zero `innerHTML`, and zero `allow-same-origin`.
- [ ] Upload via `/admin` → **主题管理** → activate → verify `/` + Light/Dark toggle + opening a plugin (e.g. `docker-cheatsheet` / `dynamic-favicon`) inherits the theme cleanly.
