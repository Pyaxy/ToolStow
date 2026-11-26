---
name: toolstow-theme
description: Official developer guide and companion scripts for designing, live-testing, validating, and packaging custom public frontend themes (Skin-Only ui.css or Full Custom HTML/CSS/JS/SPA shells) for ToolStow (v0.3.0+).
---

# ToolStow Theme Development Guide

Use this skill whenever designing, implementing, live-testing, validating, or packaging a custom public frontend theme for **ToolStow** (v0.3.0+).

## 1. Architecture & Mental Model

ToolStow's single Rust binary **only consumes built static artifacts** — it never compiles theme source code (TypeScript, SCSS, Vite, Tailwind) at runtime. Built-in and third-party themes follow the exact same runtime contract:

- **Total visual ownership**: Colors, accent, typography, radii, light/dark palettes, textures, and control appearance are owned **100% by the theme** (ToolStow has no global accent picker).
- **Strict route isolation**: `/api/*`, `/admin*`, `/t/*`, and `/ui/*` are locked on the backend and can **never** be shadowed by a theme.
- **No server restart on theme edits or switches**: ToolStow resolves the active theme from `<data-dir>/themes/<short>/` dynamically and serves theme assets with `Cache-Control: no-cache`.
- **Two theme modes under one manifest**:

| Mode | Required Files | What ToolStow Serves | Best For |
| :--- | :--- | :--- | :--- |
| **Skin-Only** *(fastest)* | `theme.json` + `ui.css` (+ optional `assets/`, `preview.svg`) | Built-in responsive shell (`frontend/`) on `/` + your `ui.css` appended to `/ui/v1/toolstow.css` | Fast reskinning of **both** the host shell and every plugin using pure CSS tokens & `.ts-*` overrides. Zero HTML/JS. |
| **Full Custom Shell** | `theme.json` + (`index.html` or `dist/index.html`) + `ui.css` (+ local CSS/JS/assets & optional `preview.svg`) | Your static shell on `/` + your `ui.css` appended to `/ui/v1/toolstow.css` | Completely custom public layouts (TUI, street-ad wall, sketchbook, dashboard, React/Vite SPA) that still reskin embedded plugins seamlessly. |

> **Hard validity rule**: Every theme package MUST contain `theme.json` and **at least one** of `ui.css`, `index.html`, or `dist/index.html`.

> **Companion scripts** live in `scripts/` inside this skill directory (Python 3.9+ stdlib only, zero external dependencies):
> - `scripts/validate.py <theme_dir>` — mirrors ToolStow's Rust validator (`theme.json`, archive limits, path & symlink rules) plus SVG XSS safety, `ui.css` light/dark coverage, full-shell contract & DOM hygiene, and JS syntax (`node --check`).
> - `scripts/pack.py <theme_dir> [-o out.zip]` — validates and builds a clean flat `.zip` ready to upload in `/admin` -> **主题管理**.

---

## 2. Directory Layout & Resolution Rules

On disk under `<data-dir>/themes/<short>/`, the **directory name MUST match `theme.json`'s `short` field exactly**.

### Layout A — Skin-Only Theme

```text
my-skin/
├── theme.json        # Required manifest (short === "my-skin")
├── ui.css            # Appended automatically to GET /ui/v1/toolstow.css
├── assets/           # Optional fonts/textures loaded via /ui/v1/theme-assets/assets/...
└── preview.svg       # Optional 16:9 static preview (SVG or PNG; zero <script>!)
```

### Layout B — Full Custom Shell (Vanilla HTML/CSS/JS)

```text
my-theme/
├── theme.json        # Required manifest (short === "my-theme")
├── ui.css            # Recommended: reskins plugins & shared /ui/v1/toolstow.css controls
├── preview.svg       # Optional 16:9 static preview (or preview.png)
├── assets/           # Optional shared textures/fonts via /ui/v1/theme-assets/assets/...
├── index.html        # Public shell entry (served on GET /)
├── style.css         # Shell-specific layout styles
└── app.js            # Shell logic talking to /api/me & /api/tools
```

### Layout C — Full Custom Shell (Built SPA / Vite in `dist/`)

```text
my-spa-theme/
├── theme.json        # Required manifest at theme root (NEVER inside dist/)
├── ui.css            # Required/recommended at theme root (NEVER inside dist/)
├── preview.png       # Optional preview at theme root
├── assets/           # Optional shared plugin textures/fonts for /ui/v1/theme-assets/...
└── dist/
    ├── index.html    # Built entry HTML (takes precedence over root index.html)
    └── assets/       # Built JS/CSS chunks referenced by dist/index.html
```

### Exact Backend Resolution Rules

| Request | Resolution Order | Notes |
| :--- | :--- | :--- |
| `GET /` & static shell assets | 1. `<theme_root>/dist/` if `dist/index.html` exists<br>2. `<theme_root>/` if root `index.html` exists<br>3. Embedded `frontend/` (Skin-Only mode) | **No HTML5 history fallback**: unknown paths return `404`. Always use **hash routing** (`#tool=<id>`), never `history.pushState('/tool/...')`. |
| `GET /ui/v1/toolstow.css` | Embedded base `toolstow.css` + active `<theme_root>/ui.css` appended at the end | `ui.css` ALWAYS resolves from `<theme_root>/ui.css`, even when using `dist/index.html`. |
| `GET /ui/v1/theme-assets/{*path}` | Active `<theme_root>/{*path}` (for both Skin-Only & Full Shell themes) | Always resolves from `<theme_root>/`, NOT `dist/`. Ideal for fonts/SVGs shared by host + sandboxed plugin iframes. |
| Active theme missing or broken | Graceful fallback to built-in `"default"` (or legacy `data/theme/` if present) | Never bricks `/` or `/admin`. |

> **Never bundle `node_modules/`** or build-tool dotfiles into your deliverable theme directory. For Vite/SPA projects, keep source code in a separate project directory and copy or output only `theme.json`, `ui.css`, `preview.*`, `assets/`, and `dist/` into the packable theme directory.

---

## 3. Manifest Schema (`theme.json`)

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

| Field | Type | Required | Default | Rules & Constraints |
| :--- | :--- | :--- | :--- | :--- |
| `short` | `string` | **Yes** | — | Unique theme slug matching `^[a-z0-9]+(?:-[a-z0-9]+)*$` (1–64 chars, lowercase ASCII letters, digits, single hyphens). **`"default"` is reserved** and rejected. Must equal the directory name under `data/themes/`. |
| `name` | `string` | **Yes** | — | Display title shown in `/admin` (non-empty after trimming). |
| `version` | `string` | No | `"0.1.0"` | Display version string (trimmed). |
| `description` | `string` | No | `""` | Short summary shown in `/admin` (trimmed). |
| `author` | `string` | No | `""` | Author label shown in `/admin` (trimmed). |
| `url` | `string` | No | `""` | Optional homepage/repo URL. If non-empty, must start with `http://` or `https://`, include a host, and contain no whitespace or control characters. |
| `preview` | `string` | No | `null` | Safe relative path (no leading/trailing `/`, no `..`, no `\`, no `:`, no symlinks) to an existing regular image file inside the theme root (e.g. `"preview.svg"` or `"preview.png"`). Recommended 16:9 aspect ratio (e.g. `viewBox="0 0 960 540"` for SVG). Served by `/admin` under `Content-Security-Policy: sandbox;` and `X-Content-Type-Options: nosniff`, so SVG previews must be **100% self-contained** (no external web fonts, remote stylesheets, or `http(s)://` images) and **never include `<script>`, `on*` event attributes, or `javascript:` URIs**. |

---

## 4. Quick-Start & Local Live-Development Loop

### Path A — Direct Drop-In Live Dev (Recommended for Vanilla & Skin-Only Themes)

ToolStow reads active theme files directly from `<data-dir>/themes/<short>/` with `Cache-Control: no-cache`, so edits take effect immediately on browser reload with **zero server restart**:

1. **Start a local ToolStow instance** (default `http://127.0.0.1:8080`; use an admin key of at least 16 characters to avoid startup length warnings):
   ```bash
   # Using a built release binary:
   TOOLSTOW_ADMIN_KEY=dev-secret-local-1234 ./target/release/toolstow --port 8080 --data-dir ./data

   # Or from source with Cargo:
   TOOLSTOW_ADMIN_KEY=dev-secret-local-1234 cargo run -- --port 8080 --data-dir ./data
   ```
   *(Optional flags: `--themes-dir <path>` to point at another themes directory; `--include-drafts` to preview draft tools on `/`.)*

2. **Create and edit your theme directly inside `data/themes/<short>/`** (matching `theme.json`'s `short`):
   ```bash
   mkdir -p data/themes/my-theme
   ```

3. **Activate once** (browser `/admin` or headless startup config):
   - **Browser**: Open `http://127.0.0.1:8080/admin`, log in with `dev-secret-local-1234`, open **主题管理**, and activate `my-theme`. Open `http://127.0.0.1:8080/` in another tab.
   - **Headless / scripted local preview**: Write `{"activeTheme": "my-theme"}` to `<data-dir>/settings.json` **before starting ToolStow** (missing `siteTitle`/`siteDescription` fields default automatically). Note that `settings.json` is loaded on startup and updated via `/admin`, not hot-reloaded on manual disk edits while running.

4. **Edit -> Refresh -> Verify**:
   - Edit `ui.css`, `index.html`, `style.css`, `app.js`, or `assets/*` directly in `data/themes/my-theme/` and hard-refresh the browser (`Cmd/Ctrl+Shift+R`).
   - Test both Light & Dark modes, open at least one theme-aware plugin (e.g. `docker-cheatsheet` or `dynamic-favicon`), and verify live mode switching inside the iframe.

### Path B — Vite / SPA Dev Server Proxy (Optional for `dist/` Themes)

When authoring a TypeScript/React/Vue/Svelte shell in a separate source repo, proxy ToolStow's locked backend paths to a running local (or remote) ToolStow instance:

```ts
// vite.config.ts
import { defineConfig } from "vite";

const target = process.env.TOOLSTOW_ORIGIN || "http://127.0.0.1:8080";

export default defineConfig({
  base: "./",
  server: {
    proxy: {
      "/api": { target, changeOrigin: true },
      "/ui": { target, changeOrigin: true },
      "/t": { target, changeOrigin: true },
      "/admin": { target, changeOrigin: true },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
```

Run against your local server:
```bash
TOOLSTOW_ORIGIN=http://127.0.0.1:8080 npm run dev
```

Before packaging, build `npm run build` and assemble a clean deliverable folder containing ONLY `theme.json`, `ui.css`, optional `preview.*`, optional `assets/`, and `dist/`.

> **Vite reminder**: Keep `<link rel="stylesheet" href="/ui/v1/toolstow.css">` FIRST inside `index.html` `<head>` (Vite leaves absolute `/ui/...` stylesheet links untouched at build time).

---

## 5. Styling Host & Plugins Together (`ui.css` & Theme Assets)

ToolStow serves its shared design stylesheet at **`GET /ui/v1/toolstow.css`** (`Cache-Control: no-cache`). Whenever your theme is active and includes a root-level `ui.css`, ToolStow automatically appends your `ui.css` to the end of `/ui/v1/toolstow.css`.

Because both your shell and every ToolStow plugin load `/ui/v1/toolstow.css`, defining your design tokens and `.ts-*` overrides in `ui.css` reskins **every installed plugin automatically** with zero plugin modifications.

### 5.1 Complete `--ts-*` Design Token Catalog

Define `:root` (Light mode, `color-scheme: light`) and `:root[data-theme="dark"]` (Dark mode, `color-scheme: dark`). For **dark-only themes** (such as CRT/terminal themes like `phosphor`), set `color-scheme: dark` and your dark palette on **both** `:root` and `:root[data-theme="dark"]` in `ui.css` so standalone plugins opened at `/t/{id}/` also resolve dark mode and your theme's computed `--ts-accent` automatically:

| Token | Built-in Light Default | Built-in Dark Default | Semantic Role |
| :--- | :--- | :--- | :--- |
| `--ts-canvas` | `#ffffff` | `#09090b` | Page / viewport background |
| `--ts-surface` | `#fafafa` | `#111113` | Card, sidebar, empty-state & badge surface |
| `--ts-surface-hover` | `#f4f4f5` | `#18181b` | Interactive surface hover fill |
| `--ts-border` | `#e5e5e5` | `#27272a` | Default hairline/card/input border |
| `--ts-border-strong` | `#d4d4d8` | `#3f3f46` | Emphasized or hovered border |
| `--ts-ink` | `#09090b` | `#fafafa` | Primary text & focus outline ink |
| `--ts-ink-secondary` | `#3f3f46` | `#d4d4d8` | Secondary labels & body copy |
| `--ts-ink-muted` | `#71717a` | `#a1a1aa` | Help text, placeholders, metadata |
| `--ts-inverted-bg` | `#09090b` | `#fafafa` | Primary button, active tab & toast fill |
| `--ts-inverted-bg-hover` | `#27272a` | `#e4e4e7` | Primary button hover fill |
| `--ts-inverted-ink` | `#ffffff` | `#09090b` | Text/icons rendered on `--ts-inverted-bg` / `--ts-accent` |
| `--ts-accent` | `#18181b` | `#fafafa` | Theme accent (used by `.ts-btn-accent` & synced to plugins; use 3/6-digit hex!) |
| `--ts-danger` | `#dc2626` | `#f87171` | Destructive actions & error text |
| `--ts-danger-bg` | `#fef2f2` | `#261214` | Danger button hover / error banner fill |
| `--ts-danger-border` | `#fecaca` | `#5c1f24` | Error container border |
| `--ts-success` | `#16a34a` | `#22c55e` | Success indicator color |
| `--ts-code-bg` | `#09090b` | `#050507` | Code block (`.ts-code`) background |
| `--ts-code-ink` | `#fafafa` | `#fafafa` | Code block text |
| `--ts-radius-sm` | `6px` | *(inherited)* | Buttons, inputs, tabs, code blocks, toasts |
| `--ts-radius-md` | `8px` | *(inherited)* | Cards and empty-state panels |
| `--ts-space-1` .. `--ts-space-5` | `4px`, `8px`, `12px`, `16px`, `20px` | *(inherited)* | Spacing scale used by `.ts-page`, `.ts-card`, `.ts-stack`, `.ts-row` |
| `--ts-font-sans` | `Inter, -apple-system, ...` | *(inherited)* | Primary UI font stack |
| `--ts-font-mono` | `ui-monospace, SFMono-Regular, ...` | *(inherited)* | Monospace font stack (`code`, `kbd`, `pre`, `.ts-code`, `.ts-badge-mono`) |

> **Notes**:
> - `ui/v1/toolstow.css` also defines one-way legacy aliases (`--canvas: var(--ts-canvas)`, `--accent: var(--ts-accent)`, etc.), so overriding `--ts-*` automatically updates legacy aliases too.
> - Keep `--ts-accent` a valid **6-digit (or 3-digit) `#rrggbb` hex color** in both light and dark modes, because `/ui/v1/theme.js` validates `accent` as hex when reading `?accent=` and `toolstow:theme`.
> - **NEVER** call `document.documentElement.style.setProperty("--ts-accent", ...)` in shell JS! An inline style on `:root` overrides `:root[data-theme="dark"]` and breaks light/dark accent switching. Let `ui.css` own `--ts-accent`, and read the computed value via `getComputedStyle`.
> - **Multi-skin themes (`data-skin`)**: Custom attributes like `data-skin="..."` set on the host `<html>` do **not** exist inside embedded plugin iframes or standalone `/t/{id}/` tabs. Always define cohesive base `:root` / `:root[data-theme="dark"]` defaults in `ui.css` for standalone plugins, and layer skin-specific `--ts-*` overrides under `:root[data-skin="..."]` (and `:root[data-theme="dark"][data-skin="..."]`) so `getComputedStyle(document.documentElement)` pushes the active skin's resolved tokens live to embedded iframes via `toolstow:theme`.

### 5.2 Starter `ui.css` Template

```css
/* ui.css */
:root {
  color-scheme: light;

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
  --ts-danger-bg: #fef2f2;
  --ts-danger-border: #fecaca;
  --ts-success: #15803d;
  --ts-code-bg: #1c1917;
  --ts-code-ink: #faf7f2;

  --ts-radius-sm: 8px;
  --ts-radius-md: 10px;
  --ts-font-sans: Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  --ts-font-mono: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

:root[data-theme="dark"] {
  color-scheme: dark;

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
  --ts-danger-bg: #2e1817;
  --ts-danger-border: #6b2926;
  --ts-success: #4ade80;
  --ts-code-bg: #12100e;
  --ts-code-ink: #f5f2ec;
}
```

### 5.3 Reskinning Shared Plugin Primitives (`.ts-*`)

ToolStow appends the active theme's `ui.css` to `/ui/v1/toolstow.css`, so tools that load the shared stylesheet receive the overrides inside their iframes. Use `ui.css` for design tokens, shared `.ts-*` controls, and semantic workbench slots. Use the shell's `style.css` for the surrounding page. Tools own their actions, state, and tool-specific components (such as a cropper's handles or a chart's interactions). Keep shared controls usable and preserve their purpose and keyboard behavior when changing their appearance or moving a slot.

| Category | Classes | Safe Override Tips |
| :--- | :--- | :--- |
| **Containers** | `.ts-card`, `.ts-empty`, `.ts-standalone-header`, `.ts-section-title` | Customize border width/style, `border-radius`, `box-shadow`, subtle header accents. Avoid adding huge margins or fixed widths/heights. |
| **Buttons** | `.ts-btn`, `.ts-btn-primary`, `.ts-btn-accent`, `.ts-btn-ghost`, `.ts-btn-danger`, `.ts-btn-sm` | Style borders, shadows, hover/active transforms, focus-visible ring. Always maintain `>= 4.5:1` text contrast in both light & dark modes. |
| **Form Controls** | `.ts-input`, `.ts-textarea`, `.ts-select` | Customize border, radius, shadow, and `:focus` accent border/ring. Keep padding & full readability intact. |
| **Tabs & Chips** | `.ts-tabs`, `.ts-tab`, `.ts-tab.active`, `.ts-badge`, `.ts-badge-mono`, `.ts-kbd` | Great places to inject signature theme identity (pills, stamps, sharp terminal boxes, wobbly ink outlines). |
| **Code & Feedback** | `.ts-code`, `.ts-code-row`, `.ts-toast` | Keep `.ts-code` monospace and readable; remember `.ts-code-row > code` sits on `--ts-canvas` (not `--ts-code-bg`). |
| **Workbench Slots** | `.ts-workbench`, `.ts-wb-toolbar`, `.ts-wb-nav`, `.ts-wb-aside`, `.ts-wb-main`, `.ts-collection`, `.ts-collection-group` | Rearrange plugin internal layouts via CSS Grid in `ui.css`! E.g. dock `.ts-wb-nav` as a left directory rail or right-edge vertical notebook tabs, pin `.ts-wb-toolbar` to the bottom as a `grep>` bar, or set `.ts-collection { --ts-collection-min: 100%; grid-template-columns: 1fr; }` for a 1-column terminal readout. Always guard desktop slot rearrangements inside `@media (min-width: 761px)` and handle `:not(:has(> .ts-wb-nav))` / `:has(> .ts-wb-aside)` cleanly. |

> **Base `/ui/v1/toolstow.css` resets & defaults**: Before your `ui.css` and `style.css` run, `/ui/v1/toolstow.css` sets `*, *::before, *::after { box-sizing: border-box; }`, `color-scheme: light` (and `dark` under `:root[data-theme="dark"]`), `html, body` (`margin: 0; padding: 0; font-size: 14px; line-height: 1.5; background: var(--ts-canvas); color: var(--ts-ink);`), `body { min-height: 100dvh; }`, `[hidden] { display: none !important; }`, `.ts-btn` / `.ts-input` / `.ts-select` (`height: 34px; font-size: 13px;`), and `.ts-card` (`padding: 16px; border: 1px solid var(--ts-border); background: var(--ts-surface);`).
>
> **Full-viewport background tip**: Because `/ui/v1/toolstow.css` sets `background: var(--ts-canvas)` on `html` (preventing dark-mode FOUC), CSS does not propagate a `body`-only `background-image` to the viewport canvas. Always apply full-page textures to `html, body` (or ensure `body` keeps `min-height: 100dvh` — never `min-height: 100%` without `html { height: 100%; }`) so sparse tool lists never leave an untextured band at the bottom of the screen.
>
> **Embedded plugin helper**: `/ui/v1/theme.js` automatically sets `data-embed="true"` on `<html>` inside iframes, and `/ui/v1/toolstow.css` hides `.ts-standalone-only` when embedded. Do not override `:root[data-embed="true"] .ts-standalone-only`.

### 5.4 Static Theme Assets (`GET /ui/v1/theme-assets/{*path}`)

Both **Skin-Only** and **Full Custom Shell** themes can bundle `.woff2` fonts, background images, and SVG patterns under the theme root (e.g. `assets/grid-light.svg`, `fonts/display.woff2`) and reference them from `ui.css` or `style.css` via `/ui/v1/theme-assets/<relative-path>`:

```css
@font-face {
  font-family: "MyThemeFont";
  src: url("/ui/v1/theme-assets/fonts/my-font.woff2") format("woff2");
  font-display: swap;
}

html,
body {
  background-color: var(--ts-canvas);
  background-image: url("/ui/v1/theme-assets/assets/paper-grid-light.svg");
  background-size: 28px 28px;
}

:root[data-theme="dark"],
:root[data-theme="dark"] body {
  background-image: url("/ui/v1/theme-assets/assets/paper-grid-dark.svg");
}
```

**Backend Headers & Security Rules for `/ui/v1/theme-assets/{*path}`**:
- **CORS-enabled for sandboxed plugins**: Served on `GET`/`HEAD` with `Access-Control-Allow-Origin: *`, `Cache-Control: no-cache`, and `X-Content-Type-Options: nosniff` — so `@font-face` fonts and SVG/image textures work identically on `/` **and** inside `Origin: null` sandboxed plugin iframes.
- **CSP sandbox on active markup**: `.svg`, `.html`, and `.xhtml` responses automatically include `Content-Security-Policy: sandbox;`.
- **Path restrictions**: Resolved strictly inside the active theme root (or legacy `data/theme/` when `activeTheme === "default"`). Rejects `..`, `\`, `:`, leading/trailing `/`, any dot-prefixed hidden segment (`.git`, `.env`, `.DS_Store`), `theme.json` (`404`), and symlink escapes (`403`).

---

## 6. Public Runtime API Contract (Full Custom Shells)

### Endpoint Overview

| Endpoint | Method | Auth | Purpose |
| :--- | :--- | :--- | :--- |
| `/api/me` | `GET` | Public (reads optional session cookie) | Site title/description, admin visibility flag (`isAdmin`), active theme info. |
| `/api/tools` | `GET` | Public | Published tool catalog with optional `q`, `category`, `tag` filters. |
| `/t/{id}/{*path}` | `GET` | Public (published tools) | Sandboxed static tool assets (`CSP: sandbox ...;`, `nosniff`, `no-cache`). |
| `/ui/v1/toolstow.css` | `GET`/`HEAD` | Public | Base design tokens + `.ts-workbench` slots + active theme's `ui.css`. |
| `/ui/v1/theme.js` | `GET`/`HEAD` | Public | Plugin-side theme bridge (`toolstow:ready` / `toolstow:theme`). |
| `/ui/v1/host.js` | `GET`/`HEAD` | Public | Headless Host SDK (`window.ToolStowHost`) implementing Contract A for full custom shells. |
| `/ui/v1/theme-assets/{*path}` | `GET`/`HEAD` | Public | Active theme static assets with `Access-Control-Allow-Origin: *`. |
| `/admin*` & `/api/admin/*` | Locked | Admin Session ONLY (tools also accept API key) | Never called by public shells — only link to `/admin` when `me.isAdmin === true`. |

### 6.1 `GET /api/me`

Always fetch with `{ credentials: "same-origin", cache: "no-store" }` (backend returns `Cache-Control: no-store`):

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

| Field | Type | Semantics & UI Requirements |
| :--- | :--- | :--- |
| `siteTitle` | `string` | Site heading (1–80 chars, default `"ToolStow"`). Render in header and sync to `document.title` (`"<tool.name> · <siteTitle>"` when a tool is open). |
| `siteDescription` | `string` | Subtitle / tagline (0–240 chars, may be empty `""`). Handle empty string gracefully. |
| `isAdmin` | `boolean` | **Mandatory gating**: Show a link to `/admin` (e.g. `管理 / 设置`) **if and ONLY if `isAdmin === true`**. Default the link to `hidden` in HTML (`[hidden] { display: none !important; }` is provided by `toolstow.css`) so anonymous visitors never see it. |
| `adminEnabled` | `boolean` | `true` when `TOOLSTOW_ADMIN_KEY` is configured. Do NOT use `adminEnabled` to show `/admin` — gate strictly on `isAdmin === true`. |
| `activeTheme` | `string` | Active theme `short` (or `"default"`). |
| `themeSource` | `"custom" \| "embedded"` | `"custom"` when served from disk; `"embedded"` for built-in default. |

> **Strict Read-Only Public Shell Rule**: Never render upload, delete, draft-toggle, or settings forms on `/`. All mutations belong exclusively in `/admin`.

### 6.2 `GET /api/tools`

Fetch with `{ credentials: "same-origin", cache: "no-store" }`.

- **Optional query parameters**:
  - `q`: case-insensitive substring match against `name`, `description`, `id`, or any entry in `tags`.
  - `category`: exact case-insensitive match against `category` (trimmed).
  - `tag`: exact case-insensitive match against any item in `tags` (trimmed).
  - You may either query the server on filter changes OR fetch `/api/tools` once with no params and filter client-side (fetching all tools once for category/tag counts + querying or filtering for the grid works smoothly either way).
- **Deterministic server sort order**: `(category.toLowerCase(), name.toLowerCase(), id)`.
- **Draft filtering**: Returns only published tools (`draft: false`) unless the server was started with `--include-drafts`.

Example response (note that `iconUrl` is **omitted** when `iconIsPath` is `false`, and **present** when `iconIsPath` is `true`):

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
    "entry": "index.html",
    "entryUrl": "/t/docker-cheatsheet/index.html",
    "runtime": "static",
    "themeAware": true,
    "sandbox": ["allow-scripts", "allow-forms"],
    "draft": false
  },
  {
    "id": "svg-editor",
    "name": "SVG Icon Tool",
    "version": "0.2.0",
    "description": "Vector icon inspector",
    "category": "设计",
    "tags": ["svg"],
    "icon": "assets/icon.svg",
    "iconIsPath": true,
    "iconUrl": "/t/svg-editor/assets/icon.svg",
    "entry": "index.html",
    "entryUrl": "/t/svg-editor/index.html",
    "runtime": "static",
    "themeAware": true,
    "sandbox": ["allow-scripts", "allow-downloads"],
    "draft": false
  }
]
```

**Edge Cases Every Full Custom Shell MUST Handle**:
1. **Empty tool list (`[]`)**: Show a friendly, clear empty state (distinct from "no search/filter matches", where a "清空筛选" button resets `q`, `category`, and `tag`).
2. **Icons (`icon` / `iconIsPath` / `iconUrl`)**:
   - Render `<img src="...">` ONLY when `tool.iconIsPath === true` AND `tool.iconUrl` passes `isSafeToolUrl(url)` (`typeof url === "string" && url.startsWith("/t/") && !url.startsWith("//")`). Attach an `error` listener on `<img>` to fall back to a text glyph.
   - Otherwise render `tool.icon || "🛠️"` strictly via `textContent`.
3. **Entry URL (`entryUrl`)**: Validate with `isSafeToolUrl(tool.entryUrl)` (`startsWith("/t/") && !startsWith("//")`) before mounting the iframe or using the URL in an optional standalone link.
4. **Empty `sandbox: []`**: A tool may explicitly request `sandbox: []` (fully locked-down static page with zero JS). Preserve `[]` whenever `Array.isArray(tool.sandbox)` is true — only fall back to `["allow-scripts", "allow-downloads", "allow-forms"]` if `tool.sandbox` is missing/not an array!
5. **Network/HTTP error**: Display a non-blocking status/error banner and allow retrying without crashing the page.

---

## 7. Hosting Plugins & Live Theme Sync Contract (Mandatory for Full Shells)

Full custom shells can satisfy the host runtime contract (**Contract A**) in one of two ways:
- **Recommended — Use the Headless Host SDK (`/ui/v1/host.js`, `window.ToolStowHost`)**: Eliminates ~250 lines of fetch, filter, hash-routing, sandbox allowlist, iframe mounting, and `postMessage` boilerplate while leaving 100% of DOM structure and visual design to your theme. When `<script src="/ui/v1/host.js"></script>` is loaded in your shell HTML and `ToolStowHost` is used in JS, `validate.py` automatically marks the `/api/me`, `/api/tools`, `toolstow:theme`, and `toolstow:ready` checks as satisfied (while still enforcing `isAdmin` gating for `/admin`).
- **Alternative — Standalone Custom Implementation (e.g. bundled React/Vite SPA)**: Directly fetch `/api/me` and `/api/tools` and implement the URL validation, sandbox filtering, and `postMessage` handshake below.

### 7.0 Using `/ui/v1/host.js` (`window.ToolStowHost` Headless SDK)

Load `/ui/v1/host.js` in `<head>` or before your shell's `app.js`:

```html
<link rel="stylesheet" href="/ui/v1/toolstow.css">
<link rel="stylesheet" href="./style.css">
<script src="/ui/v1/host.js"></script>
<script type="module" src="./app.js"></script>
```

Create and bootstrap a host controller:

```js
const host = window.ToolStowHost.create({
  iframeHost: document.getElementById("iframe-host"), // optional auto-mount container
  defaultMode: "auto",   // "auto" | "light" | "dark"
  lockMode: null,        // set "dark" for dark-only themes like phosphor
  hashRouting: true,     // automatic #tool=<id> deep-linking
  onStateChange(state) {
    // state.site: { siteTitle, siteDescription, isAdmin, adminEnabled, activeTheme, themeSource }
    // state.allTools, state.filteredTools, state.categories ([{name, count}]), state.tags
    // state.filters: { q, category, tag }
    // state.selectedId, state.selectedTool, state.theme: { mode, accent }, state.loading, state.error
    adminLink.hidden = !state.site.isAdmin;
    renderShell(state);
  },
});

await host.bootstrap();
```

Key `ToolStowHost` static helpers & instance methods:
- **Static helpers**: `ToolStowHost.renderIcon(containerEl, tool, fallbackGlyph)`, `ToolStowHost.resolveIconUrl(tool)`, `ToolStowHost.isSafeToolUrl(url)`, `ToolStowHost.sanitizeSandboxTokens(tokens)`, `ToolStowHost.normalizeHexColor(val)`, `ToolStowHost.readStorage(key)` / `writeStorage(key, val)`.
- **Instance methods**: `host.bootstrap()`, `host.getState()`, `host.refresh()`, `host.setFilters({ q, category, tag })`, `host.clearFilters()`, `host.hasActiveFilters()`, `host.selectTool(id)`, `host.clearSelection()`, `host.mountTool(containerEl, toolOrId, { cacheBust })`, `host.reloadTool()`, `host.getStandaloneUrl(tool)`, `host.setThemeMode(mode)`, `host.toggleThemeMode()`, `host.syncTheme()` (call after switching custom sub-skins like `data-skin` or `data-phosphor` to push all 27 computed `--ts-*` tokens to the active plugin iframe), `host.subscribe(fn)`.

---

### 7.1 Stylesheet Load Order & Anti-FOUC `<head>` Bootstrap (Raw Contract Details)

Always load `/ui/v1/toolstow.css` **FIRST** in `<head>` before your shell stylesheet:

```html
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>ToolStow</title>
  <link rel="stylesheet" href="/ui/v1/toolstow.css">
  <link rel="stylesheet" href="./style.css">
  <script src="./boot.js"></script>
</head>
```

> **Anti-FOUC host bootstrap**: To apply saved `localStorage["toolstow:mode"]` (and optional `data-skin`) to `document.documentElement.dataset.theme` before first paint, **both** an inline `<script>` in `<head>` and an external blocking `<script src="./boot.js">` are allowed on `/` (ToolStow serves host `/` with no script-blocking CSP, and `validate.py` checks both inline and external scripts with `node --check` + DOM hygiene rules).

### 7.2 Strict URL Validation & Sandbox Allowlist

```js
const SANDBOX_ALLOWLIST = new Set([
  "allow-scripts",
  "allow-downloads",
  "allow-forms",
  "allow-modals",
  "allow-popups",
  "allow-popups-to-escape-sandbox",
]);
const DEFAULT_SANDBOX = ["allow-scripts", "allow-downloads", "allow-forms"];

function isSafeToolUrl(url) {
  return typeof url === "string" && url.startsWith("/t/") && !url.startsWith("//");
}

function sanitizeSandboxTokens(rawTokens) {
  if (!Array.isArray(rawTokens)) return DEFAULT_SANDBOX;
  // Preserves [] when a tool explicitly requests an empty sandbox!
  return rawTokens.filter((t) => typeof t === "string" && SANDBOX_ALLOWLIST.has(t));
}
```

- **NEVER** allow `allow-same-origin` or `allow-top-navigation*` anywhere in HTML or JS.
- Always set `iframe.setAttribute("referrerpolicy", "no-referrer")`.

### 7.3 Anti-FOUC Initial Iframe & Standalone URLs

Append `?theme=<mode>&accent=<computedHexAccent>` when constructing `iframe.src` (preserving any existing query params or hash on `tool.entryUrl`). ToolStow's `/ui/v1/theme.js` reads these synchronously in `<head>` so embedded plugins render in the right Light/Dark mode on frame 1 with zero white flash:

```js
function buildToolUrl(entryUrl, mode, accent, cacheBust = false) {
  const url = new URL(entryUrl, window.location.origin);
  url.searchParams.set("theme", mode);
  url.searchParams.set("accent", accent);
  if (cacheBust) url.searchParams.set("_ts", String(Date.now()));
  return `${url.pathname}${url.search}${url.hash}`;
}
```

> **Optional standalone link**. It opens a fresh tool instance in a new tab. Use `host.getStandaloneUrl(tool)` with the SDK, or validate `tool.entryUrl` and use `buildToolUrl(tool.entryUrl, mode, accent, false)` in a custom implementation. Update the link when mode or accent changes. A sandboxed `/t/{id}/*` tab uses these query parameters because it cannot read the host's `localStorage["toolstow:mode"]`.

### 7.4 Live `postMessage` Handshake (`toolstow:ready` & `toolstow:theme`)

Read the active tokens from `getComputedStyle(document.documentElement)` (never hardcode or call `setProperty("--ts-accent", ...)` inline), listen for `toolstow:ready` from the currently mounted iframe, and post `toolstow:theme` on:
1. `iframe`'s `load` event,
2. receiving `{ type: "toolstow:ready" }` from `state.activeIframe.contentWindow`, and
3. every Light/Dark mode (or custom sub-skin) switch.

Sync whenever **`tool.themeAware === true || state.activeIframeReady === true`**:

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

function normalizeHexColor(value) {
  if (typeof value !== "string") return null;
  const s = value.trim();
  if (/^#[0-9a-fA-F]{6}$/.test(s)) return s.toLowerCase();
  const m = s.match(/^#([0-9a-fA-F])([0-9a-fA-F])([0-9a-fA-F])$/);
  return m ? `#${m[1]}${m[1]}${m[2]}${m[2]}${m[3]}${m[3]}`.toLowerCase() : null;
}

function readComputedAccent(mode) {
  const raw = getComputedStyle(document.documentElement).getPropertyValue("--ts-accent");
  return normalizeHexColor(raw) || (mode === "dark" ? "#fafafa" : "#18181b");
}

function syncThemeToIframe(iframe, mode) {
  if (!iframe || !iframe.contentWindow) return;
  const cs = getComputedStyle(document.documentElement);
  const tokens = {};
  for (const name of SYNCED_TOKENS) {
    const val = cs.getPropertyValue(name).trim();
    if (val) tokens[name] = val;
  }
  const accent = readComputedAccent(mode);
  iframe.contentWindow.postMessage(
    { type: "toolstow:theme", payload: { mode, accent, tokens } },
    "*"
  );
}

// Listen for plugin readiness:
window.addEventListener("message", (event) => {
  if (!state.activeIframe || event.source !== state.activeIframe.contentWindow) return;
  if (event.data && event.data.type === "toolstow:ready") {
    state.activeIframeReady = true;
    syncThemeToIframe(state.activeIframe, state.theme.mode);
  }
});
```

> **Note on `/ui/v1/theme.js` sanitization**: Token values containing `;`, `{`, `}`, `url(`, `expression(`, or `@import` (or > 256 chars) are ignored by `theme.js`. Background images/textures belong in `ui.css` using `/ui/v1/theme-assets/...` (which plugins load directly via `/ui/v1/toolstow.css`), while colors, radii, spacing, and fonts sync live through `tokens`.

### 7.5 Safe DOM Rendering & Essential UX

- **Zero unsafe HTML sinks**: Never use `.innerHTML`, `.outerHTML`, `.insertAdjacentHTML()`, or `document.write()`. Build all DOM nodes with `document.createElement()`, `textContent`, and `replaceChildren()`.
- **Mode persistence**: Persist `"light"` / `"dark"` in host `localStorage` under `"toolstow:mode"` (wrapped in `try/catch`), falling back to `window.matchMedia("(prefers-color-scheme: dark)")`, and set `document.documentElement.dataset.theme = mode`.
- **Deep-linking & navigation**. Support `#tool=<encodedId>` on page load and when the hash changes. Keep a clear way to return to the theme's tool navigation, such as a directory or town map. If the catalog request fails, retain the hash and resolve it after a successful retry.
- **Optional workspace actions**. Themes choose whether and where to expose reload and standalone opening. A theme with a full-page tool view can put the standalone link in a menu or omit it. If provided, use a label that promises a new tab, such as `在新标签页打开`, with `target="_blank" rel="noopener noreferrer"`.
- **Keyboard & accessibility**: Every navigation or control surface the theme provides (search, map pins, modal triggers, quick switcher, etc.) must be keyboard-operable with visible `:focus-visible` outlines, alongside a responsive mobile layout and `@media (prefers-reduced-motion: reduce)` for animations/marquees. Map-, town-, or shortcut-driven themes with no persistent search bar are explicitly allowed. *(Optional for search-bar themes: e.g., `Cmd/Ctrl+K` to focus search or open a quick-switcher palette without discarding an active tool iframe, and `Escape` to close overlays or clear/blur search.)*
- **Recommended UX Principles (Soft Guidelines — Not Enforced by `validate.py`)**:
  - **Home & discovery freedom**: Full custom shells own the entire `/` layout (minimalist list, ops table, ad wall, or interactive town map), while skin-only themes inherit the built-in shell layout; simply avoid rendering the exact same tool list twice side-by-side on one screen.
  - **Active workspace**. Themes choose the size of the tool view and which navigation, metadata, or actions surround it. A focused workspace can use a compact toolbar and put secondary actions in a details popover.
  - **New-tool discoverability**: Spatial or curated themes (e.g., mapping known tools to specific town buildings) should provide a fallback discovery surface (such as a notice board or catalog drawer) so newly uploaded tools from `/api/tools` are immediately reachable without editing the theme.
- **Responsive & overflow guardrails (test at `320px`, `390px`, `844×390`, and `1280×600`)**:
  - **No clipped full-page backgrounds**: Put repeating wall/paper textures on `html, body` (or ensure `body { min-height: 100dvh; }`) so sparse pages (1–3 tools) fill the entire viewport without exposing a solid bottom band.
  - **No obstructive `position: fixed` stickers/badges**: Decorative floating widgets must never cover tool cards or buttons on short desktop (`1280×600`) or landscape mobile (`844×390`) viewports — keep them in normal document flow, or hide them with `@media (max-width: 1180px), (max-height: 780px)` and add bottom padding to the main container.
  - **Long unbroken categories & tags**: Backend allows arbitrary category/tag strings. Constrain chips/pills with `max-width: 100%; min-width: 0; overflow-wrap: anywhere;` so long unbroken strings never trigger horizontal overflow on `320px`/`390px` screens.
  - **Narrow topbar & workspace header**: On `<= 640px`, let search inputs wrap onto a full-width row when admin links/controls are visible, and allow the active tool workspace title to wrap cleanly (up to 2 lines) rather than crushing to a few characters.

---

## 8. Validation, Packaging & Installation

Always validate and package using the companion scripts before delivery.

### 8.1 Validate

```bash
# Standard validation
python3 skills/toolstow-theme/scripts/validate.py ./my-theme

# Strict mode (warnings become errors + enforce directory name === theme.json short)
python3 skills/toolstow-theme/scripts/validate.py --strict --strict-dir-name ./my-theme

# Machine-readable JSON report
python3 skills/toolstow-theme/scripts/validate.py --json ./my-theme
```

### 8.2 Package as Clean Flat `.zip`

```bash
python3 skills/toolstow-theme/scripts/pack.py --strict --strict-dir-name ./my-theme -o ./toolstow-theme-my-theme.zip
```

*(If this skill is installed standalone outside the ToolStow repo, replace `skills/toolstow-theme/scripts/...` with the path to this skill's `scripts/` folder.)*

### 8.3 Archive Limits & Upload Security

| Constraint | Limit / Rule |
| :--- | :--- |
| Max `.zip` upload size | 50 MiB default (configurable via `TOOLSTOW_MAX_UPLOAD_MB` / `--max-upload-mb`) |
| Max uncompressed size | 200 MiB total across all entries |
| Max file count | 1,000 regular files |
| Path & entry safety | No symlinks, no `..` traversal, no special Unix files, no duplicate/conflicting paths; macOS `__MACOSX`/`.DS_Store`/`._*` ignored; optional single top-level wrapper folder unwrapped on upload (`pack.py` emits a clean flat archive). |
| **Upload Auth** | **Admin Session ONLY!** `/api/admin/themes/*` strictly requires an authenticated `/admin` browser session (`TOOLSTOW_ADMIN_KEY`) and **rejects `TOOLSTOW_API_KEY` Bearer tokens**. Always upload and activate via `/admin` -> **主题管理**. |

---

## 9. Reference Themes & Pre-Delivery Checklist

Inspect the bundled examples in `examples/themes/` as working references:
- **Skin-Only**: `examples/themes/paper-warm/` (`theme.json`, `ui.css`, `preview.svg`)
- **Full TUI Shell + Workbench Slot Rearrangement**: `examples/themes/ascii-probe/` (uses `/ui/v1/host.js` and rearranges `.ts-workbench` in `ui.css` into a left vertical `.ts-wb-nav` rail, bottom `grep>` `.ts-wb-toolbar`, and 1-column `.ts-collection`)
- **Full Ops Table Shell**: `examples/themes/serverstatus/` (uses `/ui/v1/host.js`)
- **Full Sketchbook Shell + Shared SVG Paper Assets + Right-Edge Workbench Tabs**: `examples/themes/rakugaki/` (demonstrates `/ui/v1/theme-assets/assets/paper-grid-*.svg` and right-hand vertical notebook index tabs on `.ts-wb-nav`)
- **Dark-Only NOC CRT Phosphor Terminal Shell**: `examples/themes/phosphor/` (demonstrates dark-only `lockMode: "dark"`, `color-scheme: dark` on `:root`, P1 Green / P3 Amber / P4 Ice phosphor switching via `host.syncTheme()`, and CRT scanline overlays across host and plugins)

### Pre-Delivery Compliance Checklist

- [ ] `python3 skills/toolstow-theme/scripts/validate.py --strict --strict-dir-name <theme_dir>` passes with **0 errors and 0 warnings**.
- [ ] `theme.json` has valid `short` (`^[a-z0-9]+(?:-[a-z0-9]+)*$`, not `"default"`, matching directory name) and non-empty `name`.
- [ ] Contains `ui.css` (defining both `:root` + `color-scheme: light` and `:root[data-theme="dark"]` + `color-scheme: dark`, with hex `--ts-accent`) and/or `index.html` (`dist/index.html`).
- [ ] Every `.svg` file (`preview.svg`, `assets/*.svg`, etc.) is valid XML with zero `<script>`, zero `on*` attributes, and zero `javascript:`.
- [ ] Full custom shell loads `/ui/v1/toolstow.css` **first**, opens `#tool=<id>` deep links cleanly, provides a clear way to return to the theme's tool navigation entry (directory/list, map/town, etc.), keeps every provided navigation/control surface (search, map pins, modal triggers, quick switcher, etc.) keyboard-operable with visible `:focus-visible` outlines, shows `/admin` strictly when `me.isAdmin === true`, and renders zero upload/delete controls.
- [ ] Full custom shell validates `entryUrl`/`iconUrl` (`startsWith("/t/") && !startsWith("//")`), filters `sandbox` through the 6-token allowlist while preserving `[]`, sends `?theme=&accent=` + `toolstow:theme` on `load`/`toolstow:ready`/toggle, and contains zero `innerHTML`, zero `allow-same-origin`, zero `allow-top-navigation*`, and zero inline `setProperty("--ts-accent", ...)`.
- [ ] Packaged cleanly with `scripts/pack.py` (no `node_modules/`), uploaded via `/admin` -> **主题管理**, and verified on `/` in both Light and Dark modes alongside a plugin across desktop (`1280×600`), narrow mobile (`320px`/`390px`, including logged-in admin link), and landscape mobile (`844×390`) viewports (full-height background, zero horizontal overflow, zero overlapping floating elements).
