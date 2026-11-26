# Changelog

All notable changes to **ToolStow** are documented in this file.

## [0.3.3] - 2026-11-26

### Added
- **Single-file HTML import.** Drop an HTML file or paste its source in `/admin`, preview it in a sandbox, and save it as a tool with optional shared UI integration. Automated clients can use `/api/admin/tools/import/html`.
- **Host SDK and semantic workbench slots.** Full themes can use `/ui/v1/host.js` for tool discovery, deep links, iframe mounting, and theme sync. Tools can expose shared slots for themes to rearrange through CSS.
- **Phosphor theme.** Added a dark CRT-style theme with green, amber, and ice palettes. The selected palette follows tools opened in a new tab.

### Changed
- Simplified the default homepage to a searchable tool list that refreshes when revisited. Moved the new-tab action into the active tool's details.
- Reorganized the admin console around tool collection, tool management, and theme settings. Updated the tool and theme development guides for the new layouts and SDKs.
- Bumped crate version to `0.3.3` (`toolstow --version`).

### Fixed
- Preserve the active tool and its inputs while searching, and restore the requested tool after its deep link is retried.
- Keep keyboard focus and result highlighting in sync in the default search dialog. Tools using the shared SDK can forward `Cmd/Ctrl+K` to open it.
- Remove duplicate tag filters and fix narrow-screen metadata overflow, drawer focus, and control contrast in the bundled themes.

## [0.3.2] - 2026-11-26

### Added
- **Docker & Docker Compose support (`Dockerfile`, `.dockerignore`, `compose.yml`)**:
  - Added a non-root (`UID/GID 1000`) `alpine:3.24`-based runtime image with `TOOLSTOW_HOST=0.0.0.0`, `/data` persistence volume (`toolstow_data`), built-in `HEALTHCHECK`, fast CI `prebuilt` multi-arch stage, and local source `builder` stage, plus Compose validation in CI (`.github/workflows/ci.yml`).
- **Multi-arch GHCR release pipeline (`.github/workflows/release.yml`)**:
  - Automatically smoke-tests and publishes `linux/amd64` and `linux/arm64` images to `ghcr.io/pyaxy/toolstow` on every `v*` tag (preserving `v`-prefixed tags on prereleases without updating `latest`, and scoping GitHub Release artifact downloads to `release-*`).
- **Locked health check endpoint (`GET /healthz`)**:
  - Added non-shadowable `GET /healthz` returning `200 OK`, `Cache-Control: no-store`, and `{"status":"ok","version":"..."}` with integration test coverage.

### Changed
- Updated `README.md` and `docs/deployment.md` with Docker / Docker Compose installation, volume permissions, and container port configuration guides.
- Bumped crate version to `0.3.2` (`toolstow --version`).

## [0.3.1] - 2026-11-26

### Added
- **Self-contained Agent Skills (`skills/toolstow-tool/`, `skills/toolstow-theme/`)**:
  - Packaged ToolStow's tool and theme specifications as portable Agent Skill folders (`SKILL.md` + zero-dependency Python 3.9+ `scripts/validate.py`, `pack.py`, and `publish.py`) covering manifest/theme schema validation, sandbox/XSS/SVG safety checks, optional `node --check`, clean flat `.zip` packaging, and `409`-safe Bearer API publishing.
- **Unified CI & Release bundling (`scripts/bundle_examples.py`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `.github/release.yml`)**:
  - Auto-discovers bundled example themes and plugins to validate and package `toolstow-themes.zip` and `toolstow-plugins.zip` identically across PR CI and Release builds.
  - Added CI syntax checks for built-in frontend JS (`frontend/app.js`, `admin/admin.js`, `ui/v1/theme.js`) and all skill/bundling Python scripts.
  - Hardened Release pipeline with a `preflight` tag-vs-`Cargo.toml` version check, unpacked single-binary smoke tests (`--version`, `/`, `/ui/v1/theme.js`) across all 4 native platforms, artifact completeness checks, narrowed `GITHUB_TOKEN` permissions, automatic `prerelease` detection, and categorized GitHub Release notes (`.github/release.yml`).

### Changed
- **Smaller release binaries (`Cargo.toml`)**: Added `[profile.release]` (`strip = true`, `lto = "thin"`, `codegen-units = 1`) to shrink single-binary distribution size.
- **Documentation (`README.md`)**: Rewrote `README.md` to highlight AI autonomous tool/theme creation via Agent Skills and prioritize direct GitHub Releases installation.
- Bumped crate version to `0.3.1` (`toolstow --version`).

### Fixed
- **Full-viewport background height (`/ui/v1/toolstow.css`)**: Set `body { min-height: 100vh; min-height: 100dvh; }` so themes and short embedded plugins with `body` background textures fill the entire viewport without exposing a solid bottom band.
- **Narrow mobile default shell (`frontend/style.css`)**: Wrapped `.topbar` cleanly onto a second row below `640px` so the search bar gets full width even when the authenticated admin link is shown, aligned the mobile sidebar drawer cleanly beneath `.topbar`, and prevented long unbroken categories/tags from overflowing.

## [0.3.0] - 2026-11-25

### Added
- **Multi-Theme Manager (`src/themes.rs`, `/api/admin/themes*`, `/admin` Theme Gallery)**:
  - Stores installed themes under `--themes-dir` / `TOOLSTOW_THEMES_DIR` (default `<data-dir>/themes/<short>/`) with `theme.json` validation, safe ZIP extraction (Zip Slip / symlink / zip-bomb defenses, single-folder unwrap), `?overwrite=false` `409 THEME_EXISTS` confirmation, sandboxed preview image serving (`GET /api/admin/themes/{short}/preview`), one-click switching (`PUT /api/admin/themes/active` persisted to `data/settings.json`), and safe deletion (auto-reverting active theme to `"default"`).
  - Supports both **full custom shell themes** (`index.html` / `dist/index.html` + optional `ui.css`) and **skin-only themes** (`ui.css` only, reskinning the built-in shell and all plugins).
  - Restricted all `/api/admin/themes*` routes to **Admin Session ONLY** (`TOOLSTOW_ADMIN_KEY`), rejecting AI Bearer keys (`401`).
- **Cross-origin theme assets (`GET /ui/v1/theme-assets/{*path}` + CORS)**:
  - Added locked read-only `/ui/v1/theme-assets/{*path}` and `Access-Control-Allow-Origin: *` headers on custom theme static assets (with `CSP: sandbox;` on SVG/HTML and symlink/dot-segment blocking) so custom `.woff2` fonts, background images, and SVG textures work seamlessly on both `/` and inside `Origin: null` sandboxed plugin iframes.
- **4 curated bundled themes (`examples/themes/`)** inspired by `monitor-probe/themes`:
  - `ascii-probe` (retro terminal TUI full shell + `ui.css`)
  - `serverstatus` (high-density ops table full shell + `ui.css`)
  - `paper-warm` (warm parchment editorial skin-only `ui.css`)
  - `rakugaki` (hand-drawn sketchbook & chalkboard doodle full shell + `ui.css` + tiled grid SVG assets)
- **AI Theme Development Skill** (`docs/ai-theme-skill.md`) for generating drop-in ToolStow themes.

### Changed
- **Theme-owned visual styling**: Removed manual accent-color pickers from the public topbar, `/admin` site settings, `SiteSettings`, and `GET /api/me` (`defaultAccent`). Themes now own 100% of color and accent styling via `ui.css`, while shells still read computed `--ts-accent` and `--ts-*` tokens to sync to plugin iframes.
- Bumped crate version to `0.3.0` (`toolstow --version`).

## [0.2.2] - 2026-11-25

### Added
- **Shared `/ui/v1/*` design system & plugin theme SDK**:
  - Added locked backend routes `GET /ui/v1/toolstow.css` and `GET /ui/v1/theme.js` (never shadowed by `data/theme/`), with automatic safe cascade append of optional `data/theme/ui.css` (`403` on symlink escape) so custom skins update host, `/admin`, and all plugins together.
  - Added synchronous `?theme=&accent=` anti-FOUC initialization, unconditional `toolstow:ready` theme handshake, computed `--ts-*` token mirroring, `.ts-standalone-only` anti-double-header helper, and `window.ToolStow` (`getTheme`, `onThemeChange`, `copy`, `toast`).
- **Same-ID upload overwrite confirmation**:
  - `POST /api/admin/tools/upload` now supports `?overwrite=false` returning `409 Conflict` (`TOOL_EXISTS`) with both `existing` and `incoming` tool metadata without touching the live tool directory.
  - `/admin` upload flow checks `?overwrite=false` first and shows a monochrome confirmation dialog detailing existing vs. incoming version/status before retrying with `?overwrite=true`.

### Changed
- Migrated `frontend/`, `admin/`, `examples/docker-cheatsheet`, and `examples/dynamic-favicon` to `/ui/v1/toolstow.css` + `/ui/v1/theme.js`.
- Updated `docs/ai-tool-skill.md` with the 3-layer UI boundary, anti-duplicate-header rule, `/ui/v1/*` template, and `?overwrite=false` reference.
- Bumped crate version to `0.2.2` (`toolstow --version`).

## [0.2.1] - 2026-11-24

### Changed
- Bumped crate version in `Cargo.toml` / `Cargo.lock` to `0.2.1` (`toolstow --version`).
- Reorganized project documentation: moved `REQUIREMENTS.md` out of the repository root into `docs/requirements.md`.
- Added root `README.md`, `CHANGELOG.md`, and `docs/deployment.md` (VPS deployment, `systemd`, Caddy HTTPS, and `data/theme/` override guide).

## [0.2.0] - 2026-11-24

### Added
- **Unified `--data-dir` runtime layout** (`./data` containing `tools/`, optional `theme/`, and `settings.json`) and zero-dependency `.env` configuration loading (`CLI > env > .env > defaults`).
- **Dual-key fail-closed authentication** (`src/auth.rs`):
  - `TOOLSTOW_ADMIN_KEY`: single-key `/admin` login exchanging for an in-memory 256-bit `HttpOnly; SameSite=Strict` session cookie (24h TTL), constant-time comparison, and per-IP rate limiting (`429` after 5 failures in 5 minutes).
  - `TOOLSTOW_API_KEY`: `Authorization: Bearer` token restricted to `/api/admin/tools*` (cannot log into `/admin` or edit site settings).
- **Hugo-style `draft` visibility**: optional `"draft": false` field in `manifest.json`, hidden from public `/api/tools` and returning `404` on anonymous `/t/{id}/*`, previewable via admin session cookie or `--include-drafts`, with atomic `PATCH /api/admin/tools/{id}` toggling.
- **Decoupled public theme hot-override**: drop static assets into `data/theme/` to override `/` without recompiling Rust while `/admin*`, `/api/*`, and `/t/*` stay locked by the backend.
- **`GET /api/me` and `GET/PUT /api/admin/settings`** backed by atomic `data/settings.json` (`src/settings.rs`).
- **Minimalist monochrome UI** inspired by `monitor-probe/monitor` for both the read-only public shell (`frontend/`, revealing「管理 / 设置」strictly when `isAdmin === true`) and the built-in `/admin` panel (`admin/`).

### Changed
- Removed unauthenticated `/api/tools/upload` and `DELETE /api/tools/{id}` endpoints in favor of authenticated `/api/admin/tools/*`.
- Updated `docs/ai-tool-skill.md` and `.env.example` for dual-key Bearer publishing and `draft` management.

## [0.1.0] - 2026-11-24

### Added
- Initial Rust (Edition 2024) single-binary ToolStow host with `axum`, `tokio`, `rust-embed`, `notify`, and `zip`.
- `manifest.json` parser & validator with sandbox allowlist stripping (`allow-same-origin` strictly forbidden).
- Safe ZIP archive extraction (`enclosed_name()` Zip Slip defense, symlink rejection, streaming zip-bomb quotas, single-root folder unwrap, same-filesystem atomic swap).
- In-memory tool registry with startup scan, filtering, and debounced `notify` filesystem hot-reload.
- Sandboxed static hosting (`/t/{id}/*`) with `Content-Security-Policy: sandbox`, `nosniff`, and `no-cache`.
- Bundled example tools (`examples/docker-cheatsheet`, `examples/dynamic-favicon`) and initial AI tool skill guide.
