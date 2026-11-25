# Changelog

All notable changes to **ToolStow** are documented in this file.

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
