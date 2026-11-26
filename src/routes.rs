//! Axum HTTP router, dual-key authentication, CSRF protection, admin APIs,
//! decoupled custom theme serving, static tool hosting, and embedded assets.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Component, Path};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, HttpBody};
use axum::extract::connect_info::ConnectInfo;
use axum::extract::multipart::{Multipart, MultipartError, MultipartRejection};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path as AxumPath, Query, State};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_ORIGIN, AUTHORIZATION, CACHE_CONTROL, CONTENT_LENGTH,
    CONTENT_SECURITY_POLICY, CONTENT_TYPE, HOST, ORIGIN, SET_COOKIE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::request::Parts;
use axum::http::uri::Authority;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Json, Redirect, Response};
use axum::routing::{delete, get, patch, post, put};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

use crate::archive::{self, ArchiveLimits, SingleHtmlToolInput};
use crate::auth::AuthManager;
use crate::config::Config;
use crate::error::AppError;
use crate::manifest::{self, ToolEntry};
use crate::registry::ToolRegistry;
use crate::settings::{SettingsStore, SiteSettings, SiteSettingsUpdate};
use crate::themes::{self, BUILTIN_DEFAULT_THEME, ResolvedTheme, ThemeEntry, ThemeInstallOutcome};

/// Custom header required on browser-originated and cookie-authenticated mutating requests so
/// cross-origin or sandboxed submissions trigger CORS preflight and are blocked.
pub const CLIENT_HEADER_NAME: &str = "x-toolstow-client";

/// Embedded default public frontend shell assets from `frontend/`.
///
/// In debug builds `rust-embed` reads files from disk for fast iteration; in
/// release builds assets are compiled directly into the single binary.
#[derive(RustEmbed)]
#[folder = "frontend/"]
struct FrontendAssets;

/// Embedded locked admin panel assets from `admin/`.
///
/// Always served directly from the binary under `/admin*` and never shadowable by `data/theme/`.
#[derive(RustEmbed)]
#[folder = "admin/"]
struct AdminAssets;

/// Embedded locked shared UI design system assets from `ui/`.
///
/// Always served directly from the binary under `/ui/v1/*` and never shadowable by `data/theme/`.
#[derive(RustEmbed)]
#[folder = "ui/"]
struct UiAssets;

/// Banner inserted before optional `data/theme/ui.css` overrides appended to `/ui/v1/toolstow.css`.
const THEME_UI_OVERRIDE_BANNER: &[u8] =
    b"\n\n/* --- ToolStow Custom Theme Override (data/theme/ui.css) --- */\n";

/// Shared application state injected into Axum route handlers.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Thread-safe in-memory tool registry.
    pub registry: Arc<ToolRegistry>,
    /// Parsed runtime server configuration.
    pub config: Arc<Config>,
    /// File-backed site settings store (`<data_dir>/settings.json`).
    pub settings: SettingsStore,
    /// Dual-key authentication, admin session, and rate-limit manager.
    pub auth: AuthManager,
}

/// Optional query filters accepted by `GET /api/tools`.
#[derive(Debug, Default, Deserialize)]
struct ListToolsQuery {
    q: Option<String>,
    category: Option<String>,
    tag: Option<String>,
}

/// Optional query filters accepted by `GET /api/admin/tools`.
#[derive(Debug, Default, Deserialize)]
struct AdminListToolsQuery {
    q: Option<String>,
    category: Option<String>,
    tag: Option<String>,
    draft: Option<String>,
}

/// JSON response payload for `GET /healthz`.
#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

/// JSON response payload for `GET /api/me`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MeResponse {
    site_title: String,
    site_description: String,
    is_admin: bool,
    admin_enabled: bool,
    active_theme: String,
    theme_source: &'static str,
}

/// JSON response payload for `GET /api/admin/session`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminSessionResponse {
    authenticated: bool,
    admin_enabled: bool,
}

/// JSON request payload for `POST /api/admin/login`.
#[derive(Debug, Deserialize)]
struct LoginRequest {
    key: String,
}

/// JSON response payload for `POST /api/admin/login`.
#[derive(Debug, Serialize)]
struct LoginResponse {
    ok: bool,
}

/// JSON request payload for `PATCH /api/admin/tools/{id}`.
#[derive(Debug, Deserialize)]
struct PatchToolRequest {
    draft: bool,
}

/// JSON request payload for `POST /api/admin/tools/import/html`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadSingleHtmlToolRequest {
    id: String,
    name: String,
    html: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    inject_sdk: bool,
}

/// Optional query parameters accepted by `POST /api/admin/tools/upload`,
/// `POST /api/admin/tools/import/html`, and `POST /api/admin/themes/upload`.
#[derive(Debug, Default, Deserialize)]
struct UploadOverwriteQuery {
    /// When `false`, existing tools/themes with the same identifier return `409 Conflict`
    /// instead of overwriting. Defaults to `true` for `.zip` uploads and `false` for
    /// single-HTML imports when omitted.
    overwrite: Option<bool>,
}

/// JSON conflict payload returned by `POST /api/admin/tools/upload?overwrite=false` when a tool
/// with the same `id` is already installed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolExistsConflictResponse {
    code: &'static str,
    error: String,
    existing: Option<ToolEntry>,
    incoming: ToolEntry,
}

/// JSON conflict payload returned by `POST /api/admin/themes/upload?overwrite=false` when a theme
/// with the same `short` is already installed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThemeExistsConflictResponse {
    code: &'static str,
    error: String,
    existing: Option<ThemeEntry>,
    incoming: ThemeEntry,
}

/// JSON request payload for `PUT /api/admin/themes/active`.
#[derive(Debug, Deserialize)]
struct SetActiveThemeRequest {
    short: String,
}

/// JSON response payload for `PUT /api/admin/themes/active`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActiveThemeResponse {
    active_theme: String,
}

/// Extracts a rate-limit client key from optional `ConnectInfo<SocketAddr>`, falling back to
/// `"local"` in in-process `oneshot` tests where no TCP peer address is attached.
#[derive(Debug, Clone)]
struct ClientKey(String);

impl<S> FromRequestParts<S> for ClientKey
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(extract_client_key(parts)))
    }
}

/// Extracts the client IP string from request extensions if `ConnectInfo<SocketAddr>` is present.
fn extract_client_key(parts: &Parts) -> String {
    parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip().to_string())
        .unwrap_or_else(|| "local".to_string())
}

/// Request guard enforcing CSRF and Origin checks on browser-originated mutating routes.
#[derive(Debug, Clone, Copy)]
struct CsrfGuard;

impl<S> FromRequestParts<S> for CsrfGuard
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        verify_csrf_headers(&parts.headers)?;
        Ok(Self)
    }
}

/// Request guard for Admin-Session-only routes (`GET /api/admin/settings`, `PUT /api/admin/settings`).
///
/// - Fail-closed (`404`) when `TOOLSTOW_ADMIN_KEY` is not configured.
/// - Always rejects `Origin: null` and `Sec-Fetch-Site: cross-site` (`403`).
/// - Strictly requires a valid Admin Session Cookie (`401` otherwise; `TOOLSTOW_API_KEY` Bearer
///   tokens are NEVER accepted).
/// - Enforces CSRF + `X-ToolStow-Client` on mutating requests and whenever `Origin` is present.
#[derive(Debug, Clone, Copy)]
struct AdminSessionOnly;

impl FromRequestParts<AppState> for AdminSessionOnly {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if !state.auth.is_admin_enabled() {
            return Err(AppError::not_found("admin panel is disabled"));
        }

        verify_no_cross_site_or_null_origin(&parts.headers)?;

        if !state.auth.has_valid_admin_session(&parts.headers) {
            return Err(AppError::unauthorized("admin session required"));
        }

        let is_mutating = !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
        if is_mutating {
            verify_csrf_headers(&parts.headers)?;
            require_client_header(&parts.headers)?;
        } else if parts.headers.contains_key(ORIGIN) {
            verify_csrf_headers(&parts.headers)?;
        }

        Ok(Self)
    }
}

/// Request guard for Admin Tool Management routes (`/api/admin/tools*`).
///
/// Accepts EITHER a valid `Authorization: Bearer <TOOLSTOW_API_KEY>` OR a valid Admin Session
/// Cookie (with CSRF protection on mutating requests and whenever `Origin` is present).
/// Running as a [`FromRequestParts`] extractor ensures unauthorized or cross-site requests are
/// rejected before any multipart upload body is parsed.
#[derive(Debug, Clone, Copy)]
struct AdminToolsAuth;

impl FromRequestParts<AppState> for AdminToolsAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if !state.auth.is_any_management_enabled() {
            return Err(AppError::not_found("tool management endpoints are disabled"));
        }

        verify_no_cross_site_or_null_origin(&parts.headers)?;

        if parts.headers.contains_key(AUTHORIZATION) {
            let client_key = extract_client_key(parts);
            let verified = state
                .auth
                .verify_bearer_api_key(&client_key, &parts.headers)?;
            if !verified {
                return Err(AppError::unauthorized("authentication required"));
            }
            return Ok(Self);
        }

        if state.auth.has_valid_admin_session(&parts.headers) {
            let is_mutating = !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
            if is_mutating {
                verify_csrf_headers(&parts.headers)?;
                require_client_header(&parts.headers)?;
            } else if parts.headers.contains_key(ORIGIN) {
                verify_csrf_headers(&parts.headers)?;
            }
            return Ok(Self);
        }

        Err(AppError::unauthorized("authentication required"))
    }
}

/// Rejects `Sec-Fetch-Site: cross-site` and opaque/null `Origin` headers with `403 Forbidden`.
fn verify_no_cross_site_or_null_origin(headers: &HeaderMap) -> Result<(), AppError> {
    if let Some(sec_fetch_site) = headers.get("sec-fetch-site") {
        let value = sec_fetch_site
            .to_str()
            .map_err(|_| AppError::forbidden("invalid Sec-Fetch-Site header"))?
            .trim();
        if value.eq_ignore_ascii_case("cross-site") {
            return Err(AppError::forbidden("cross-site request rejected"));
        }
    }

    if let Some(origin_header) = headers.get(ORIGIN) {
        let origin_raw = origin_header
            .to_str()
            .map_err(|_| AppError::forbidden("invalid Origin header"))?
            .trim();

        if origin_raw.is_empty() || origin_raw.eq_ignore_ascii_case("null") {
            return Err(AppError::forbidden(
                "requests from opaque or null origins are not allowed",
            ));
        }
    }

    Ok(())
}

/// Requires a non-empty `X-ToolStow-Client` header.
fn require_client_header(headers: &HeaderMap) -> Result<(), AppError> {
    let client_header = headers
        .get(CLIENT_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty());

    if client_header.is_none() {
        return Err(AppError::forbidden(
            "missing required X-ToolStow-Client header",
        ));
    }

    Ok(())
}

/// Validates request headers for browser-originated state-mutating endpoints.
///
/// Rules:
/// 1. Reject if `Sec-Fetch-Site` is `"cross-site"`.
/// 2. If `Origin` is present (browser request):
///    - Reject if `Origin` is empty or `"null"` (sandboxed opaque origin).
///    - Verify `Origin`'s host and effective port match the `Host` header.
///    - Require a non-empty `X-ToolStow-Client` header.
fn verify_csrf_headers(headers: &HeaderMap) -> Result<(), AppError> {
    verify_no_cross_site_or_null_origin(headers)?;

    if let Some(origin_header) = headers.get(ORIGIN) {
        let origin_raw = origin_header
            .to_str()
            .map_err(|_| AppError::forbidden("invalid Origin header"))?
            .trim();

        let origin_uri: Uri = origin_raw
            .parse()
            .map_err(|_| AppError::forbidden("malformed Origin header"))?;

        let default_port: u16 = match origin_uri.scheme_str() {
            Some("http") => 80,
            Some("https") => 443,
            _ => {
                return Err(AppError::forbidden(
                    "Origin header must use http or https scheme",
                ));
            }
        };

        let origin_authority = origin_uri
            .authority()
            .ok_or_else(|| AppError::forbidden("Origin header missing host authority"))?;

        if origin_authority.as_str().contains('@') {
            return Err(AppError::forbidden(
                "Origin header must not include userinfo",
            ));
        }

        let host_raw = headers
            .get(HOST)
            .ok_or_else(|| AppError::forbidden("missing Host header"))?
            .to_str()
            .map_err(|_| AppError::forbidden("invalid Host header"))?
            .trim();

        let host_authority: Authority = host_raw
            .parse()
            .map_err(|_| AppError::forbidden("malformed Host header"))?;

        if host_authority.as_str().contains('@') {
            return Err(AppError::forbidden("malformed Host header"));
        }

        let origin_host = origin_authority.host().to_ascii_lowercase();
        let origin_port = origin_authority.port_u16().unwrap_or(default_port);

        let request_host = host_authority.host().to_ascii_lowercase();
        let request_port = host_authority.port_u16().unwrap_or(default_port);

        if origin_host != request_host || origin_port != request_port {
            return Err(AppError::forbidden(
                "Origin header does not match request Host",
            ));
        }

        require_client_header(headers)?;
    }

    Ok(())
}

/// Detects whether the client reached ToolStow over HTTPS via reverse-proxy headers
/// (`X-Forwarded-Proto: https` or RFC 7239 `Forwarded: ...proto=https`).
pub fn is_request_https(headers: &HeaderMap) -> bool {
    if let Some(proto_val) = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        && let Some(first) = proto_val.split(',').next()
        && first.trim().eq_ignore_ascii_case("https")
    {
        return true;
    }

    if let Some(forwarded_val) = headers.get("forwarded").and_then(|v| v.to_str().ok())
        && let Some(first_hop) = forwarded_val.split(',').next()
    {
        for param in first_hop.split(';') {
            if let Some((k, v)) = param.split_once('=')
                && k.trim().eq_ignore_ascii_case("proto")
            {
                let unquoted = v.trim().trim_matches('"');
                if unquoted.eq_ignore_ascii_case("https") {
                    return true;
                }
            }
        }
    }

    false
}

/// Returns `true` when an external custom public theme (`<theme_dir>/index.html`) is active.
pub fn is_custom_theme_active(theme_dir: &Path) -> bool {
    theme_dir.join("index.html").is_file()
}

/// Builds the full ToolStow Axum [`Router`].
pub fn build_router(state: AppState) -> Router {
    let upload_body_limit = state
        .config
        .max_upload_bytes()
        .saturating_add(1024 * 1024);

    let upload_routes = Router::new()
        .route("/api/admin/tools/upload", post(upload_tool))
        .route(
            "/api/admin/tools/import/html",
            post(upload_single_html_tool),
        )
        .route("/api/admin/themes/upload", post(upload_theme))
        .layer(DefaultBodyLimit::max(upload_body_limit));

    Router::new()
        // Public frontend shell root
        .route("/", get(serve_index))
        // Locked liveness / health check endpoint
        .route("/healthz", get(healthz))
        // Public read-only API
        .route("/api/me", get(get_me))
        .route("/api/tools", get(list_tools))
        // Static sandboxed tool hosting
        .route("/t/{id}", get(redirect_tool_entry))
        .route("/t/{id}/", get(redirect_tool_entry))
        .route("/t/{id}/{*path}", get(serve_tool_asset))
        // Locked embedded admin panel (never shadowable by data/theme/ or data/themes/)
        .route("/admin", get(serve_admin_index))
        .route("/admin/", get(serve_admin_index))
        .route("/admin/{*path}", get(serve_admin_asset))
        // Locked shared UI design system & theme bridge (never shadowable by data/theme/ or data/themes/)
        .route("/ui/v1/toolstow.css", get(serve_ui_css))
        .route("/ui/v1/theme.js", get(serve_ui_theme_js))
        .route("/ui/v1/host.js", get(serve_ui_host_js))
        .route("/ui/v1/theme-assets/{*path}", get(serve_active_theme_asset))
        .route("/ui/{*path}", get(serve_ui_not_found))
        // Admin session, settings & theme management endpoints (Admin Session Cookie ONLY)
        .route("/api/admin/session", get(get_admin_session))
        .route("/api/admin/login", post(admin_login))
        .route("/api/admin/logout", post(admin_logout))
        .route(
            "/api/admin/settings",
            get(get_admin_settings).put(update_admin_settings),
        )
        .route("/api/admin/themes", get(list_admin_themes))
        .route("/api/admin/themes/active", put(set_active_theme))
        .route("/api/admin/themes/{short}", delete(delete_theme))
        .route(
            "/api/admin/themes/{short}/preview",
            get(serve_theme_preview),
        )
        // Admin tool management endpoints (Admin Session Cookie OR AI Bearer API key)
        .route("/api/admin/tools", get(list_admin_tools))
        .merge(upload_routes)
        .route(
            "/api/admin/tools/{id}",
            patch(patch_tool).delete(delete_tool),
        )
        // Decoupled public theme / embedded frontend fallback
        .fallback(serve_frontend_fallback)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

/// Resolves the currently active disk-installed theme off the async executor thread.
///
/// Fast-paths `"default"` without spawning a blocking task.
async fn resolve_active_theme_async(state: &AppState, active_short: String) -> Option<ResolvedTheme> {
    if active_short == BUILTIN_DEFAULT_THEME {
        return None;
    }

    let themes_dir = state.config.themes_dir.clone();
    tokio::task::spawn_blocking(move || themes::resolve_active_theme(&themes_dir, &active_short))
        .await
        .ok()
        .flatten()
}

/// `GET /`: serves `index.html` from the active public theme (`data/themes/<short>/`,
/// legacy `data/theme/`, or embedded `FrontendAssets`).
async fn serve_index(State(state): State<AppState>) -> Response {
    into_no_store_response(serve_active_public_asset(&state, "index.html", false).await)
}

/// Fallback handler serving active custom theme or embedded frontend assets without ever shadowing
/// `/admin*`, `/api/*`, `/t/*`, or `/ui*`.
async fn serve_frontend_fallback(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
) -> Response {
    let head_only = method == Method::HEAD;
    let raw_path = uri.path();

    let result = if raw_path == "/api"
        || raw_path.starts_with("/api/")
        || raw_path == "/t"
        || raw_path.starts_with("/t/")
        || raw_path == "/admin"
        || raw_path.starts_with("/admin/")
        || raw_path == "/ui"
        || raw_path.starts_with("/ui/")
        || (method != Method::GET && method != Method::HEAD)
    {
        Err(AppError::not_found(format!("route not found: {raw_path}")))
    } else {
        let rel_path = raw_path.strip_prefix('/').unwrap_or(raw_path);
        let rel_path = if rel_path.is_empty() {
            "index.html"
        } else {
            rel_path
        };

        serve_active_public_asset(&state, rel_path, head_only).await
    };

    let mut resp = into_no_store_response(result);
    if head_only {
        strip_body_preserving_content_length(&mut resp);
    }
    resp
}

/// Serves a public frontend asset according to active theme resolution:
/// 1. Installed active theme with a custom shell (`web_root: Some(...)`) -> serve from `web_root`.
/// 2. Installed active skin-only theme (`web_root: None`) -> serve embedded [`FrontendAssets`].
/// 3. `"default"` (or missing/invalid active theme) -> legacy `data/theme/` if active, else embedded.
async fn serve_active_public_asset(
    state: &AppState,
    rel_path: &str,
    head_only: bool,
) -> Result<Response, AppError> {
    let active_short = state.settings.get().active_theme;
    match resolve_active_theme_async(state, active_short).await {
        Some(resolved) => match resolved.web_root {
            Some(web_root) => serve_custom_theme_asset(&web_root, rel_path, head_only).await,
            None => serve_embedded_frontend_asset(rel_path, head_only),
        },
        None => serve_public_asset(&state.config.theme_dir, rel_path, head_only).await,
    }
}

/// Serves a public frontend asset from `theme_dir` when custom theme is active, otherwise from
/// embedded [`FrontendAssets`].
async fn serve_public_asset(
    theme_dir: &Path,
    rel_path: &str,
    head_only: bool,
) -> Result<Response, AppError> {
    if is_custom_theme_active(theme_dir) {
        serve_custom_theme_asset(theme_dir, rel_path, head_only).await
    } else {
        serve_embedded_frontend_asset(rel_path, head_only)
    }
}

/// Safely serves a static file from `theme_dir` with relative path validation and canonical
/// containment check to block traversal and symlink escapes.
async fn serve_custom_theme_asset(
    theme_dir: &Path,
    rel_path_str: &str,
    head_only: bool,
) -> Result<Response, AppError> {
    let rel_path = manifest::validate_safe_rel_path(rel_path_str)
        .map_err(|err| AppError::bad_request(err.to_string()))?;

    let candidate_path = theme_dir.join(&rel_path);

    let canonical_theme_dir = tokio::fs::canonicalize(theme_dir).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("custom theme directory not found")
        } else {
            AppError::internal(format!("failed to resolve custom theme directory: {err}"))
        }
    })?;

    let canonical_target = tokio::fs::canonicalize(&candidate_path)
        .await
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!("theme asset not found: /{rel_path_str}"))
            } else {
                AppError::internal(format!("failed to resolve theme asset path: {err}"))
            }
        })?;

    if !canonical_target.starts_with(&canonical_theme_dir) {
        return Err(AppError::forbidden(
            "requested path escapes custom theme directory boundary",
        ));
    }

    let metadata = tokio::fs::metadata(&canonical_target)
        .await
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!("theme asset not found: /{rel_path_str}"))
            } else {
                AppError::internal(format!("failed to inspect theme asset metadata: {err}"))
            }
        })?;

    if !metadata.is_file() {
        return Err(AppError::not_found(format!(
            "theme asset '/{rel_path_str}' is not a regular file"
        )));
    }

    let content_type = content_type_for_path(&canonical_target);
    let body = if head_only {
        Body::empty()
    } else {
        let bytes = tokio::fs::read(&canonical_target).await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!("theme asset not found: /{rel_path_str}"))
            } else {
                AppError::internal(format!("failed to read theme asset: {err}"))
            }
        })?;
        Body::from(bytes)
    };

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"))
        .header(CACHE_CONTROL, HeaderValue::from_static("no-store"))
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));

    if head_only {
        builder = builder.header(CONTENT_LENGTH, HeaderValue::from(metadata.len()));
    }

    builder
        .body(body)
        .map_err(|err| AppError::internal(format!("failed to build theme asset response: {err}")))
}

/// Serves a single file from embedded [`FrontendAssets`].
fn serve_embedded_frontend_asset(path: &str, head_only: bool) -> Result<Response, AppError> {
    let asset = FrontendAssets::get(path)
        .ok_or_else(|| AppError::not_found(format!("frontend asset not found: /{path}")))?;

    let content_type = content_type_for_path(Path::new(path));
    let len = asset.data.len();
    let body = if head_only {
        Body::empty()
    } else {
        Body::from(asset.data.into_owned())
    };

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"))
        .header(CACHE_CONTROL, HeaderValue::from_static("no-store"));

    if head_only {
        builder = builder.header(CONTENT_LENGTH, HeaderValue::from(len));
    }

    builder
        .body(body)
        .map_err(|err| AppError::internal(format!("failed to build frontend response: {err}")))
}

/// `GET /admin` and `GET /admin/`: serves locked embedded `admin/index.html`.
async fn serve_admin_index(State(state): State<AppState>) -> Result<Response, AppError> {
    if !state.auth.is_admin_enabled() {
        return Err(AppError::not_found("admin panel is disabled"));
    }

    serve_embedded_admin_asset("index.html")
}

/// `GET /admin/{*path}`: serves locked embedded admin panel assets (`admin.css`, `admin.js`, etc.).
async fn serve_admin_asset(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<Response, AppError> {
    if !state.auth.is_admin_enabled() {
        return Err(AppError::not_found("admin panel is disabled"));
    }

    let rel_path = path.trim_start_matches('/');
    let rel_path = if rel_path.is_empty() {
        "index.html"
    } else {
        rel_path
    };

    serve_embedded_admin_asset(rel_path)
}

/// Serves built-in admin assets, including the shared UI defaults without theme overrides.
fn serve_embedded_admin_asset(path: &str) -> Result<Response, AppError> {
    let asset = match path {
        "ui.css" => UiAssets::get("v1/toolstow.css"),
        _ => AdminAssets::get(path),
    }
    .ok_or_else(|| AppError::not_found(format!("admin asset not found: /admin/{path}")))?;

    let content_type = content_type_for_path(Path::new(path));
    let cache_control = if path == "index.html" {
        HeaderValue::from_static("no-store")
    } else {
        HeaderValue::from_static("no-cache")
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"))
        .header(CACHE_CONTROL, cache_control)
        .body(Body::from(asset.data.into_owned()))
        .map_err(|err| AppError::internal(format!("failed to build admin response: {err}")))
}

/// `GET` / `HEAD /ui/v1/toolstow.css`: serves embedded `ui/v1/toolstow.css` with optional
/// active theme (`data/themes/<short>/ui.css`) or legacy `data/theme/ui.css` cascade override
/// appended safely.
async fn serve_ui_css(State(state): State<AppState>) -> Response {
    into_no_store_response(serve_ui_css_inner(&state).await)
}

async fn serve_ui_css_inner(state: &AppState) -> Result<Response, AppError> {
    let base = UiAssets::get("v1/toolstow.css")
        .ok_or_else(|| AppError::not_found("UI asset not found: /ui/v1/toolstow.css"))?;

    let mut body = base.data.into_owned();
    let active_short = state.settings.get().active_theme;

    match resolve_active_theme_async(state, active_short).await {
        Some(resolved) => {
            if resolved.ui_css_path.is_some()
                && let Some(override_bytes) =
                    read_optional_theme_ui_override(&resolved.root_dir).await?
            {
                let banner = format!(
                    "\n\n/* --- ToolStow Active Theme Override ({}/ui.css) --- */\n",
                    resolved.entry.short
                );
                body.extend_from_slice(banner.as_bytes());
                body.extend_from_slice(&override_bytes);
            }
        }
        None => {
            if let Some(override_bytes) =
                read_optional_theme_ui_override(&state.config.theme_dir).await?
            {
                body.extend_from_slice(THEME_UI_OVERRIDE_BANNER);
                body.extend_from_slice(&override_bytes);
            }
        }
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("text/css; charset=utf-8"),
        )
        .header(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"))
        .header(CACHE_CONTROL, HeaderValue::from_static("no-store"))
        .body(Body::from(body))
        .map_err(|err| AppError::internal(format!("failed to build UI CSS response: {err}")))
}

/// `GET` / `HEAD /ui/v1/theme.js`: serves embedded `ui/v1/theme.js`.
async fn serve_ui_theme_js() -> Result<Response, AppError> {
    serve_embedded_ui_js("v1/theme.js", "/ui/v1/theme.js")
}

/// `GET` / `HEAD /ui/v1/host.js`: serves embedded `ui/v1/host.js`.
async fn serve_ui_host_js() -> Result<Response, AppError> {
    serve_embedded_ui_js("v1/host.js", "/ui/v1/host.js")
}

/// Serves a locked embedded JavaScript asset from [`UiAssets`].
fn serve_embedded_ui_js(asset_path: &str, public_path: &str) -> Result<Response, AppError> {
    let asset = UiAssets::get(asset_path)
        .ok_or_else(|| AppError::not_found(format!("UI asset not found: {public_path}")))?;

    Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("text/javascript; charset=utf-8"),
        )
        .header(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"))
        .header(CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .body(Body::from(asset.data.into_owned()))
        .map_err(|err| AppError::internal(format!("failed to build UI JS response: {err}")))
}

/// `GET` / `HEAD /ui/v1/theme-assets/{*path}`: safely serves read-only static assets (fonts,
/// background textures, illustrations, etc.) from the currently active theme directory (both
/// full-shell and skin-only themes, as well as legacy `data/theme/` when on `"default"`) with
/// `Access-Control-Allow-Origin: *` so assets load cleanly inside `Origin: null` sandboxed iframes.
async fn serve_active_theme_asset(
    State(state): State<AppState>,
    method: Method,
    AxumPath(path): AxumPath<String>,
) -> Response {
    into_no_store_response(
        serve_active_theme_asset_inner(&state, method == Method::HEAD, &path).await,
    )
}

async fn serve_active_theme_asset_inner(
    state: &AppState,
    head_only: bool,
    path: &str,
) -> Result<Response, AppError> {
    let rel_path = manifest::validate_safe_rel_path(path)
        .map_err(|err| AppError::bad_request(err.to_string()))?;

    for component in rel_path.components() {
        match component {
            Component::Normal(seg) => {
                if seg.to_string_lossy().starts_with('.') {
                    return Err(AppError::bad_request(format!(
                        "invalid theme asset path '{path}': hidden path segments are not allowed"
                    )));
                }
            }
            _ => {
                return Err(AppError::bad_request(format!(
                    "invalid theme asset path '{path}': contains non-normal path components"
                )));
            }
        }
    }

    if path.eq_ignore_ascii_case("theme.json") {
        return Err(AppError::not_found(format!(
            "theme asset not found: /ui/v1/theme-assets/{path}"
        )));
    }

    let active = state.settings.get().active_theme;
    let base_dir = if let Some(resolved) = resolve_active_theme_async(state, active.clone()).await
    {
        resolved.root_dir
    } else if active == BUILTIN_DEFAULT_THEME
        && tokio::fs::metadata(&state.config.theme_dir)
            .await
            .map(|meta| meta.is_dir())
            .unwrap_or(false)
    {
        state.config.theme_dir.clone()
    } else {
        return Err(AppError::not_found(format!(
            "theme asset not found: /ui/v1/theme-assets/{path}"
        )));
    };

    let candidate_path = base_dir.join(&rel_path);

    let canonical_base_dir = tokio::fs::canonicalize(&base_dir).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("active theme directory not found")
        } else {
            AppError::internal(format!("failed to resolve active theme directory: {err}"))
        }
    })?;

    let canonical_target = match tokio::fs::canonicalize(&candidate_path).await {
        Ok(target) => target,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if let Ok(link_meta) = tokio::fs::symlink_metadata(&candidate_path).await
                && link_meta.file_type().is_symlink()
            {
                return Err(AppError::forbidden(
                    "requested theme asset symlink escapes or cannot be resolved within theme directory",
                ));
            }
            return Err(AppError::not_found(format!(
                "theme asset not found: /ui/v1/theme-assets/{path}"
            )));
        }
        Err(err) => {
            return Err(AppError::internal(format!(
                "failed to resolve theme asset path: {err}"
            )));
        }
    };

    if !canonical_target.starts_with(&canonical_base_dir) {
        return Err(AppError::forbidden(
            "requested path escapes active theme directory boundary",
        ));
    }

    let metadata = tokio::fs::metadata(&canonical_target)
        .await
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!(
                    "theme asset not found: /ui/v1/theme-assets/{path}"
                ))
            } else {
                AppError::internal(format!("failed to inspect theme asset metadata: {err}"))
            }
        })?;

    if !metadata.is_file() {
        return Err(AppError::not_found(format!(
            "theme asset '/ui/v1/theme-assets/{path}' is not a regular file"
        )));
    }

    let content_type = content_type_for_path(&canonical_target);
    let needs_sandbox_csp = content_type
        .to_str()
        .map(|ct| {
            let essence = ct.split(';').next().unwrap_or(ct).trim();
            essence.eq_ignore_ascii_case("image/svg+xml")
                || essence.eq_ignore_ascii_case("text/html")
                || essence.eq_ignore_ascii_case("application/xhtml+xml")
        })
        .unwrap_or(false);

    let body = if head_only {
        Body::empty()
    } else {
        let bytes = tokio::fs::read(&canonical_target).await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!(
                    "theme asset not found: /ui/v1/theme-assets/{path}"
                ))
            } else {
                AppError::internal(format!("failed to read theme asset: {err}"))
            }
        })?;
        Body::from(bytes)
    };

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CACHE_CONTROL, HeaderValue::from_static("no-store"))
        .header(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"))
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));

    if head_only {
        builder = builder.header(CONTENT_LENGTH, HeaderValue::from(metadata.len()));
    }

    if needs_sandbox_csp {
        builder = builder.header(CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox;"));
    }

    builder
        .body(body)
        .map_err(|err| AppError::internal(format!("failed to build theme asset response: {err}")))
}

/// `GET` / `HEAD /ui/{*path}`: strictly locked `404 Not Found` JSON error for unknown UI paths.
async fn serve_ui_not_found(AxumPath(path): AxumPath<String>) -> AppError {
    AppError::not_found(format!("UI asset not found: /ui/{path}"))
}

/// Reads optional `<theme_dir>/ui.css` with canonical path containment verification.
///
/// Returns `Ok(None)` when `theme_dir` or `ui.css` does not exist (or is not a regular file),
/// `Err(AppError::Forbidden)` when `ui.css` is a symlink escaping `theme_dir`, and `Ok(Some(bytes))`
/// when a valid override file is present.
async fn read_optional_theme_ui_override(theme_dir: &Path) -> Result<Option<Vec<u8>>, AppError> {
    let candidate = theme_dir.join("ui.css");
    let link_meta = match tokio::fs::symlink_metadata(&candidate).await {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(AppError::internal(format!(
                "failed to inspect custom UI override metadata: {err}"
            )));
        }
    };

    let canonical_theme_dir = tokio::fs::canonicalize(theme_dir).await.map_err(|err| {
        AppError::internal(format!("failed to resolve custom theme directory: {err}"))
    })?;

    let canonical_target = match tokio::fs::canonicalize(&candidate).await {
        Ok(path) => path,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if link_meta.file_type().is_symlink() {
                return Err(AppError::forbidden(
                    "custom UI override symlink escapes or cannot be resolved within theme directory",
                ));
            }
            return Ok(None);
        }
        Err(err) => {
            return Err(AppError::internal(format!(
                "failed to resolve custom UI override path: {err}"
            )));
        }
    };

    if !canonical_target.starts_with(&canonical_theme_dir) {
        return Err(AppError::forbidden(
            "custom UI override escapes theme directory boundary",
        ));
    }

    let metadata = match tokio::fs::metadata(&canonical_target).await {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(AppError::internal(format!(
                "failed to inspect custom UI override target metadata: {err}"
            )));
        }
    };

    if !metadata.is_file() {
        return Ok(None);
    }

    let bytes = match tokio::fs::read(&canonical_target).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(AppError::internal(format!(
                "failed to read custom UI override: {err}"
            )));
        }
    };

    Ok(Some(bytes))
}

/// `GET /healthz`: returns server liveness and compiled crate version with `no-store` caching.
async fn healthz() -> Response {
    no_store_json(
        StatusCode::OK,
        HealthResponse {
            status: "ok",
            version: env!("CARGO_PKG_VERSION"),
        },
    )
}

/// `GET /api/me`: returns public site settings, current admin session state, active theme,
/// and theme source.
async fn get_me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let settings = state.settings.get();
    let (active_theme, theme_source) =
        match resolve_active_theme_async(&state, settings.active_theme).await {
            Some(resolved) => (resolved.entry.short, "custom"),
            None => (
                BUILTIN_DEFAULT_THEME.to_string(),
                if is_custom_theme_active(&state.config.theme_dir) {
                    "custom"
                } else {
                    "embedded"
                },
            ),
        };

    let body = MeResponse {
        site_title: settings.site_title,
        site_description: settings.site_description,
        is_admin: state.auth.has_valid_admin_session(&headers),
        admin_enabled: state.auth.is_admin_enabled(),
        active_theme,
        theme_source,
    };

    no_store_json(StatusCode::OK, body)
}

/// `GET /api/tools`: returns sorted public tool entries filtered by optional `q`, `category`,
/// and `tag`. Drafts are hidden unless `config.include_drafts` is enabled.
async fn list_tools(
    State(state): State<AppState>,
    Query(query): Query<ListToolsQuery>,
) -> Json<Vec<ToolEntry>> {
    let q = normalize_query_param(query.q.as_deref());
    let category = normalize_query_param(query.category.as_deref());
    let tag = normalize_query_param(query.tag.as_deref());

    let tools = state
        .registry
        .list_filtered(q, category, tag, state.config.include_drafts);
    Json(tools)
}

/// `GET /api/admin/session`: reports admin authentication and availability status (always 200 JSON).
async fn get_admin_session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let body = AdminSessionResponse {
        authenticated: state.auth.has_valid_admin_session(&headers),
        admin_enabled: state.auth.is_admin_enabled(),
    };
    no_store_json(StatusCode::OK, body)
}

/// `POST /api/admin/login`: validates `TOOLSTOW_ADMIN_KEY`, enforces rate limits, and sets the
/// `HttpOnly; SameSite=Strict` admin session cookie.
async fn admin_login(
    _csrf: CsrfGuard,
    ClientKey(client_key): ClientKey,
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Response, AppError> {
    let Json(body) = payload
        .map_err(|err| AppError::bad_request(format!("invalid JSON login payload: {err}")))?;

    let token = state.auth.login_admin(&client_key, &body.key)?;
    let is_https = is_request_https(&headers);
    let cookie_value = state.auth.build_session_cookie_header(&token, is_https);
    let cookie_header = HeaderValue::from_str(&cookie_value)
        .map_err(|err| AppError::internal(format!("failed to format session cookie: {err}")))?;

    let mut resp = no_store_json(StatusCode::OK, LoginResponse { ok: true });
    resp.headers_mut().append(SET_COOKIE, cookie_header);
    Ok(resp)
}

/// `POST /api/admin/logout`: revokes the current admin session (if any) and clears the cookie.
async fn admin_logout(
    _csrf: CsrfGuard,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Some(token) = AuthManager::extract_session_cookie(&headers) {
        state.auth.logout_session(&token);
    }

    let is_https = is_request_https(&headers);
    let clear_value = state.auth.build_clear_cookie_header(is_https);
    let clear_header = HeaderValue::from_str(&clear_value)
        .map_err(|err| AppError::internal(format!("failed to format clear cookie: {err}")))?;

    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(SET_COOKIE, clear_header)
        .header(CACHE_CONTROL, HeaderValue::from_static("no-store"))
        .body(Body::empty())
        .map_err(|err| AppError::internal(format!("failed to build logout response: {err}")))
}

/// `GET /api/admin/settings`: returns persisted site settings (Admin Session ONLY).
async fn get_admin_settings(
    _admin: AdminSessionOnly,
    State(state): State<AppState>,
) -> Response {
    let settings: SiteSettings = state.settings.get();
    no_store_json(StatusCode::OK, settings)
}

/// `PUT /api/admin/settings`: validates and atomically updates site settings (Admin Session ONLY).
async fn update_admin_settings(
    _admin: AdminSessionOnly,
    State(state): State<AppState>,
    payload: Result<Json<SiteSettingsUpdate>, JsonRejection>,
) -> Result<Response, AppError> {
    let Json(update) = payload
        .map_err(|err| AppError::bad_request(format!("invalid JSON settings payload: {err}")))?;

    let settings_store = state.settings.clone();
    let updated = tokio::task::spawn_blocking(move || settings_store.update(update))
        .await
        .map_err(|err| AppError::internal(format!("settings update task failed: {err}")))??;

    Ok(no_store_json(StatusCode::OK, updated))
}

/// `GET /api/admin/themes`: lists the built-in default theme followed by all valid installed
/// themes under `<themes_dir>` (Admin Session ONLY).
async fn list_admin_themes(
    _admin: AdminSessionOnly,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let themes_dir = state.config.themes_dir.clone();
    let active_short = state.settings.get().active_theme;

    let themes =
        tokio::task::spawn_blocking(move || themes::list_themes(&themes_dir, &active_short))
            .await
            .map_err(|err| AppError::internal(format!("list themes task failed: {err}")))?;

    Ok(no_store_json(StatusCode::OK, themes))
}

/// `GET /api/admin/themes/{short}/preview`: safely serves a theme's preview image with
/// `Content-Security-Policy: sandbox;` and `X-Content-Type-Options: nosniff` (Admin Session ONLY).
async fn serve_theme_preview(
    _admin: AdminSessionOnly,
    State(state): State<AppState>,
    AxumPath(short): AxumPath<String>,
) -> Result<Response, AppError> {
    let themes_dir = state.config.themes_dir.clone();
    let preview_path =
        tokio::task::spawn_blocking(move || themes::resolve_theme_preview(&themes_dir, &short))
            .await
            .map_err(|err| AppError::internal(format!("resolve theme preview task failed: {err}")))??;

    let bytes = tokio::fs::read(&preview_path).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("theme preview image not found")
        } else {
            AppError::internal(format!("failed to read theme preview image: {err}"))
        }
    })?;

    let content_type = content_type_for_path(&preview_path);

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox;"))
        .header(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"))
        .header(CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .body(Body::from(bytes))
        .map_err(|err| AppError::internal(format!("failed to build theme preview response: {err}")))
}

/// `POST /api/admin/themes/upload`: validates and atomically installs or updates a `.zip` theme
/// package (Admin Session ONLY).
///
/// Supports optional `?overwrite=false` to reject existing theme shorts with `409 Conflict`
/// (`THEME_EXISTS`) and both `existing` / `incoming` metadata for confirmation dialogs.
async fn upload_theme(
    _admin: AdminSessionOnly,
    State(state): State<AppState>,
    Query(query): Query<UploadOverwriteQuery>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Response, AppError> {
    let allow_overwrite = query.overwrite.unwrap_or(true);
    let max_upload_bytes = state.config.max_upload_bytes();
    let max_upload_mb = state.config.max_upload_mb;
    let zip_bytes = extract_uploaded_zip_bytes(multipart, max_upload_bytes, max_upload_mb).await?;

    let _write_guard = state.registry.write_lock().lock_owned().await;
    let themes_dir = state.config.themes_dir.clone();
    let limits = ArchiveLimits {
        max_zip_bytes: max_upload_bytes,
        ..ArchiveLimits::default()
    };

    let outcome = tokio::task::spawn_blocking(move || {
        themes::install_theme_zip_checked(&themes_dir, &zip_bytes, limits, allow_overwrite)
    })
    .await
    .map_err(|err| AppError::internal(format!("theme install task failed: {err}")))??;

    let active_short = state.settings.get().active_theme;

    match outcome {
        ThemeInstallOutcome::Installed { mut entry, updated } => {
            entry.active = entry.short == active_short;

            let status = if updated {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };

            Ok(no_store_json(status, entry))
        }
        ThemeInstallOutcome::AlreadyExists {
            mut existing,
            incoming,
        } => {
            if let Some(ref mut existing_entry) = existing {
                existing_entry.active = existing_entry.short == active_short;
            }

            let body = ThemeExistsConflictResponse {
                code: "THEME_EXISTS",
                error: format!(
                    "theme '{}' already exists; confirm overwrite to replace it",
                    incoming.short
                ),
                existing: existing.map(|e| *e),
                incoming,
            };
            Ok(no_store_json(StatusCode::CONFLICT, body))
        }
    }
}

/// `PUT /api/admin/themes/active`: switches the active public theme to `"default"` or a valid
/// installed theme short and persists it to `settings.json` (Admin Session ONLY).
async fn set_active_theme(
    _admin: AdminSessionOnly,
    State(state): State<AppState>,
    payload: Result<Json<SetActiveThemeRequest>, JsonRejection>,
) -> Result<Response, AppError> {
    let Json(body) = payload
        .map_err(|err| AppError::bad_request(format!("invalid JSON theme payload: {err}")))?;

    let short = body.short;
    if short != BUILTIN_DEFAULT_THEME {
        themes::validate_theme_short(&short)?;
    }

    let _write_guard = state.registry.write_lock().lock_owned().await;

    if short != BUILTIN_DEFAULT_THEME {
        let themes_dir = state.config.themes_dir.clone();
        let short_for_check = short.clone();
        let exists = tokio::task::spawn_blocking(move || {
            themes::resolve_active_theme(&themes_dir, &short_for_check).is_some()
        })
        .await
        .map_err(|err| AppError::internal(format!("resolve theme task failed: {err}")))?;

        if !exists {
            return Err(AppError::not_found(format!("theme '{short}' not found")));
        }
    }

    let settings_store = state.settings.clone();
    let updated = tokio::task::spawn_blocking(move || settings_store.set_active_theme(short))
        .await
        .map_err(|err| AppError::internal(format!("set active theme task failed: {err}")))??;

    Ok(no_store_json(
        StatusCode::OK,
        ActiveThemeResponse {
            active_theme: updated.active_theme,
        },
    ))
}

/// `DELETE /api/admin/themes/{short}`: deletes an installed theme directory from disk and
/// automatically reverts `activeTheme` to `"default"` if the deleted theme was active
/// (Admin Session ONLY).
async fn delete_theme(
    _admin: AdminSessionOnly,
    State(state): State<AppState>,
    AxumPath(short): AxumPath<String>,
) -> Result<StatusCode, AppError> {
    let _write_guard = state.registry.write_lock().lock_owned().await;
    let themes_dir = state.config.themes_dir.clone();
    let short_for_task = short.clone();

    let removed =
        tokio::task::spawn_blocking(move || themes::delete_theme_dir(&themes_dir, &short_for_task))
            .await
            .map_err(|err| AppError::internal(format!("delete theme task failed: {err}")))??;

    if !removed {
        return Err(AppError::not_found(format!("theme '{short}' not found")));
    }

    if state.settings.get().active_theme == short {
        let settings_store = state.settings.clone();
        tokio::task::spawn_blocking(move || {
            settings_store.set_active_theme(BUILTIN_DEFAULT_THEME.to_string())
        })
        .await
        .map_err(|err| AppError::internal(format!("reset active theme task failed: {err}")))??;
    }

    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/admin/tools`: returns all tools (including drafts) with optional `q`, `category`,
/// `tag`, and `draft=true|false` filters (Admin Session OR AI Bearer API key).
async fn list_admin_tools(
    _auth: AdminToolsAuth,
    State(state): State<AppState>,
    Query(query): Query<AdminListToolsQuery>,
) -> Result<Response, AppError> {
    let q = normalize_query_param(query.q.as_deref());
    let category = normalize_query_param(query.category.as_deref());
    let tag = normalize_query_param(query.tag.as_deref());
    let draft_filter = parse_optional_bool_param("draft", query.draft.as_deref())?;

    let mut tools = state.registry.list_filtered(q, category, tag, true);
    if let Some(expected_draft) = draft_filter {
        tools.retain(|tool| tool.draft == expected_draft);
    }

    Ok(no_store_json(StatusCode::OK, tools))
}

/// `POST /api/admin/tools/upload`: validates and atomically installs or updates a `.zip` tool
/// package (Admin Session OR AI Bearer API key).
///
/// Supports optional `?overwrite=false` to reject existing tool IDs with `409 Conflict`
/// (`TOOL_EXISTS`) and both `existing` / `incoming` metadata for confirmation dialogs.
async fn upload_tool(
    _auth: AdminToolsAuth,
    State(state): State<AppState>,
    Query(query): Query<UploadOverwriteQuery>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Response, AppError> {
    let allow_overwrite = query.overwrite.unwrap_or(true);
    let max_upload_bytes = state.config.max_upload_bytes();
    let max_upload_mb = state.config.max_upload_mb;
    let zip_bytes = extract_uploaded_zip_bytes(multipart, max_upload_bytes, max_upload_mb).await?;

    let _write_guard = state.registry.write_lock().lock_owned().await;
    let tools_dir = state.registry.tools_dir().to_path_buf();
    let limits = ArchiveLimits {
        max_zip_bytes: max_upload_bytes,
        ..ArchiveLimits::default()
    };

    let outcome = tokio::task::spawn_blocking(move || {
        archive::install_zip_bytes_checked(&tools_dir, &zip_bytes, limits, allow_overwrite)
    })
    .await
    .map_err(|err| AppError::internal(format!("archive install task failed: {err}")))??;

    match outcome {
        archive::InstallOutcome::Installed(result) => {
            state.registry.upsert(result.entry.clone());

            let status = if result.updated {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };

            Ok((status, Json(result.entry)).into_response())
        }
        archive::InstallOutcome::AlreadyExists { existing, incoming } => {
            let body = ToolExistsConflictResponse {
                code: "TOOL_EXISTS",
                error: format!(
                    "tool '{}' already exists; confirm overwrite to replace it",
                    incoming.id
                ),
                existing: existing.map(|e| *e),
                incoming,
            };
            Ok((StatusCode::CONFLICT, Json(body)).into_response())
        }
    }
}

/// `POST /api/admin/tools/import/html`: validates and atomically installs or updates a standalone
/// single-file HTML tool (Admin Session OR AI Bearer API key).
///
/// Defaults to safe create (`overwrite = false`), rejecting existing tool IDs with `409 Conflict`
/// (`TOOL_EXISTS`) and both `existing` / `incoming` metadata without modifying existing files on
/// disk. Pass `?overwrite=true` to explicitly replace an existing tool.
async fn upload_single_html_tool(
    _auth: AdminToolsAuth,
    State(state): State<AppState>,
    Query(query): Query<UploadOverwriteQuery>,
    payload: Result<Json<UploadSingleHtmlToolRequest>, JsonRejection>,
) -> Result<Response, AppError> {
    let allow_overwrite = query.overwrite.unwrap_or(false);
    let max_upload_bytes = state.config.max_upload_bytes();
    let max_upload_mb = state.config.max_upload_mb;

    let Json(body) = payload.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            AppError::payload_too_large(format!(
                "upload exceeds maximum allowed size of {max_upload_mb} MiB"
            ))
        } else {
            AppError::bad_request(format!("invalid JSON HTML tool payload: {rejection}"))
        }
    })?;

    let input = SingleHtmlToolInput {
        id: body.id,
        name: body.name,
        html: body.html,
        description: body.description,
        category: body.category,
        tags: body.tags,
        icon: body.icon,
        draft: body.draft,
        inject_sdk: body.inject_sdk,
    };

    let _write_guard = state.registry.write_lock().lock_owned().await;
    let tools_dir = state.registry.tools_dir().to_path_buf();
    let limits = ArchiveLimits {
        max_zip_bytes: max_upload_bytes,
        ..ArchiveLimits::default()
    };

    let outcome = tokio::task::spawn_blocking(move || {
        archive::install_single_html_checked(&tools_dir, input, limits, allow_overwrite)
    })
    .await
    .map_err(|err| AppError::internal(format!("single-HTML install task failed: {err}")))??;

    match outcome {
        archive::InstallOutcome::Installed(result) => {
            state.registry.upsert(result.entry.clone());

            let status = if result.updated {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };

            Ok((status, Json(result.entry)).into_response())
        }
        archive::InstallOutcome::AlreadyExists { existing, incoming } => {
            let body = ToolExistsConflictResponse {
                code: "TOOL_EXISTS",
                error: format!(
                    "tool '{}' already exists; confirm overwrite to replace it",
                    incoming.id
                ),
                existing: existing.map(|e| *e),
                incoming,
            };
            Ok((StatusCode::CONFLICT, Json(body)).into_response())
        }
    }
}

/// Extracts and size-checks the multipart `file` field for `.zip` uploads.
async fn extract_uploaded_zip_bytes(
    multipart: Result<Multipart, MultipartRejection>,
    max_upload_bytes: usize,
    max_upload_mb: usize,
) -> Result<Vec<u8>, AppError> {
    let mut multipart = multipart.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            AppError::payload_too_large(format!(
                "upload exceeds maximum allowed size of {max_upload_mb} MiB"
            ))
        } else {
            AppError::bad_request(format!(
                "invalid multipart request: {}",
                rejection.body_text()
            ))
        }
    })?;

    let mut zip_bytes: Option<Vec<u8>> = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|err| map_multipart_error(err, max_upload_mb))?
    {
        if field.name() == Some("file") {
            let mut buf = Vec::new();
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|err| map_multipart_error(err, max_upload_mb))?
            {
                if buf.len().saturating_add(chunk.len()) > max_upload_bytes {
                    return Err(AppError::payload_too_large(format!(
                        "upload exceeds maximum allowed size of {max_upload_mb} MiB"
                    )));
                }
                buf.extend_from_slice(&chunk);
            }
            zip_bytes = Some(buf);
            break;
        }
    }

    let zip_bytes =
        zip_bytes.ok_or_else(|| AppError::bad_request("missing multipart field 'file'"))?;

    if zip_bytes.is_empty() {
        return Err(AppError::bad_request("uploaded 'file' must not be empty"));
    }

    Ok(zip_bytes)
}

/// `PATCH /api/admin/tools/{id}`: atomically updates a tool's `draft` flag in `manifest.json`
/// and refreshes the in-memory registry (Admin Session OR AI Bearer API key).
async fn patch_tool(
    _auth: AdminToolsAuth,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    payload: Result<Json<PatchToolRequest>, JsonRejection>,
) -> Result<Json<ToolEntry>, AppError> {
    manifest::validate_id(&id)?;
    let Json(body) = payload
        .map_err(|err| AppError::bad_request(format!("invalid JSON patch payload: {err}")))?;

    let _write_guard = state.registry.write_lock().lock_owned().await;
    let tools_dir = state.registry.tools_dir().to_path_buf();
    let id_for_task = id.clone();

    let updated = tokio::task::spawn_blocking(move || {
        archive::set_tool_draft(&tools_dir, &id_for_task, body.draft)
    })
    .await
    .map_err(|err| AppError::internal(format!("update tool draft task failed: {err}")))??;

    state.registry.upsert(updated.clone());
    Ok(Json(updated))
}

/// `DELETE /api/admin/tools/{id}`: deletes a tool directory from disk and removes it from the
/// registry (Admin Session OR AI Bearer API key).
async fn delete_tool(
    _auth: AdminToolsAuth,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, AppError> {
    manifest::validate_id(&id)?;

    let _write_guard = state.registry.write_lock().lock_owned().await;
    let tools_dir = state.registry.tools_dir().to_path_buf();
    let id_for_task = id.clone();

    let removed_from_disk =
        tokio::task::spawn_blocking(move || archive::delete_tool_dir(&tools_dir, &id_for_task))
            .await
            .map_err(|err| AppError::internal(format!("delete tool task failed: {err}")))??;

    let removed_from_registry = state.registry.remove(&id).is_some();

    if removed_from_disk || removed_from_registry {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found(format!("tool '{id}' not found")))
    }
}

/// `GET /t/{id}` and `GET /t/{id}/`: redirects to the registered tool's `entry_url`.
/// Draft tools return `404` unless `--include-drafts` is enabled or a valid Admin Session Cookie
/// is present.
async fn redirect_tool_entry(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Result<Response, AppError> {
    let entry = state
        .registry
        .get(&id)
        .ok_or_else(|| AppError::not_found(format!("tool '{id}' not found")))?;

    if entry.draft && !state.config.include_drafts && !state.auth.has_valid_admin_session(&headers)
    {
        return Err(AppError::not_found(format!("tool '{id}' not found")));
    }

    Ok(Redirect::temporary(&entry.entry_url).into_response())
}

/// `GET /t/{id}/{*path}`: serves a static asset inside a registered tool directory with strict
/// draft visibility gating, path containment, and `Content-Security-Policy: sandbox` headers.
async fn serve_tool_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((id, path)): AxumPath<(String, String)>,
) -> Result<Response, AppError> {
    let entry = state
        .registry
        .get(&id)
        .ok_or_else(|| AppError::not_found(format!("tool '{id}' not found")))?;

    if entry.draft && !state.config.include_drafts && !state.auth.has_valid_admin_session(&headers)
    {
        return Err(AppError::not_found(format!("tool '{id}' not found")));
    }

    let rel_path = manifest::validate_safe_rel_path(&path)
        .map_err(|err| AppError::bad_request(err.to_string()))?;

    let tool_dir = state.registry.tools_dir().join(&entry.id);
    let candidate_path = tool_dir.join(&rel_path);

    let canonical_tool_dir = tokio::fs::canonicalize(&tool_dir).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found(format!("tool '{}' directory not found", entry.id))
        } else {
            AppError::internal(format!("failed to resolve tool directory: {err}"))
        }
    })?;

    let canonical_target = tokio::fs::canonicalize(&candidate_path)
        .await
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!("asset '{path}' not found in tool '{}'", entry.id))
            } else {
                AppError::internal(format!("failed to resolve asset path: {err}"))
            }
        })?;

    if !canonical_target.starts_with(&canonical_tool_dir) {
        return Err(AppError::forbidden(
            "requested path escapes tool directory boundary",
        ));
    }

    let metadata = tokio::fs::metadata(&canonical_target)
        .await
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!("asset '{path}' not found in tool '{}'", entry.id))
            } else {
                AppError::internal(format!("failed to inspect asset metadata: {err}"))
            }
        })?;

    if !metadata.is_file() {
        return Err(AppError::not_found(format!(
            "asset '{path}' is not a regular file"
        )));
    }

    let bytes = tokio::fs::read(&canonical_target).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found(format!("asset '{path}' not found in tool '{}'", entry.id))
        } else {
            AppError::internal(format!("failed to read tool asset: {err}"))
        }
    })?;

    let content_type = content_type_for_path(&canonical_target);

    // Defense in depth: re-sanitize sandbox tokens so `allow-same-origin` can never be emitted.
    let safe_sandbox = manifest::sanitize_sandbox(Some(&entry.sandbox));
    let csp_str = if safe_sandbox.is_empty() {
        "sandbox;".to_string()
    } else {
        format!("sandbox {};", safe_sandbox.join(" "))
    };
    debug_assert!(!csp_str.contains("allow-same-origin"));

    let csp_header = HeaderValue::from_str(&csp_str)
        .unwrap_or_else(|_| HeaderValue::from_static("sandbox;"));

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_SECURITY_POLICY, csp_header)
        .header(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"))
        .header(CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .body(Body::from(bytes))
        .map_err(|err| AppError::internal(format!("failed to build tool asset response: {err}")))
}

/// Converts a theme-dependent handler result into a response guaranteed to carry
/// `Cache-Control: no-store` (including `404` and other error responses).
fn into_no_store_response(result: Result<Response, AppError>) -> Response {
    let mut resp = match result {
        Ok(resp) => resp,
        Err(err) => err.into_response(),
    };
    resp.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

/// Clears a `HEAD` response body on fallback routes while preserving `Content-Length`.
fn strip_body_preserving_content_length(resp: &mut Response) {
    if !resp.headers().contains_key(CONTENT_LENGTH)
        && let Some(len) = resp.body().size_hint().exact()
    {
        resp.headers_mut()
            .insert(CONTENT_LENGTH, HeaderValue::from(len));
    }
    *resp.body_mut() = Body::empty();
}

/// Builds a JSON response with `Cache-Control: no-store`.
fn no_store_json<T: Serialize>(status: StatusCode, payload: T) -> Response {
    let mut resp = (status, Json(payload)).into_response();
    resp.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

/// Converts blank/whitespace query parameters into `None`.
fn normalize_query_param(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

/// Parses an optional boolean query parameter (`"true"` / `"false"`, blank treated as `None`).
fn parse_optional_bool_param(name: &str, value: Option<&str>) -> Result<Option<bool>, AppError> {
    let Some(raw) = normalize_query_param(value) else {
        return Ok(None);
    };

    if raw.eq_ignore_ascii_case("true") {
        Ok(Some(true))
    } else if raw.eq_ignore_ascii_case("false") {
        Ok(Some(false))
    } else {
        Err(AppError::bad_request(format!(
            "invalid '{name}' query parameter '{raw}': expected 'true' or 'false'"
        )))
    }
}

/// Maps an Axum [`MultipartError`] into a structured [`AppError`], preserving `413 Payload Too Large`.
fn map_multipart_error(err: MultipartError, max_upload_mb: usize) -> AppError {
    if err.status() == StatusCode::PAYLOAD_TOO_LARGE {
        AppError::payload_too_large(format!(
            "upload exceeds maximum allowed size of {max_upload_mb} MiB"
        ))
    } else {
        AppError::bad_request(format!("failed to read multipart upload: {err}"))
    }
}

/// Infers an HTTP `Content-Type` header for `path`, appending `charset=utf-8` for `text/*` types.
fn content_type_for_path(path: &Path) -> HeaderValue {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let header_str = if mime.type_() == mime_guess::mime::TEXT
        && mime.get_param(mime_guess::mime::CHARSET).is_none()
    {
        format!("{}; charset=utf-8", mime.essence_str())
    } else {
        mime.to_string()
    };

    HeaderValue::from_str(&header_str)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderName;

    fn headers_from(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    #[test]
    fn csrf_allows_cli_without_origin() {
        let headers = headers_from(&[("host", "127.0.0.1:8080")]);
        assert!(verify_csrf_headers(&headers).is_ok());
    }

    #[test]
    fn csrf_rejects_null_origin_and_cross_site_fetch() {
        let null_origin = headers_from(&[
            ("host", "127.0.0.1:8080"),
            ("origin", "null"),
            ("x-toolstow-client", "1"),
        ]);
        assert_eq!(
            verify_csrf_headers(&null_origin).unwrap_err().status_code(),
            StatusCode::FORBIDDEN
        );

        let cross_site = headers_from(&[
            ("host", "127.0.0.1:8080"),
            ("sec-fetch-site", "cross-site"),
            ("x-toolstow-client", "1"),
        ]);
        assert_eq!(
            verify_csrf_headers(&cross_site).unwrap_err().status_code(),
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn csrf_checks_same_origin_and_client_header() {
        let missing_client = headers_from(&[
            ("host", "127.0.0.1:8080"),
            ("origin", "http://127.0.0.1:8080"),
        ]);
        assert_eq!(
            verify_csrf_headers(&missing_client)
                .unwrap_err()
                .status_code(),
            StatusCode::FORBIDDEN
        );

        let mismatched_host = headers_from(&[
            ("host", "127.0.0.1:8080"),
            ("origin", "http://evil.example:8080"),
            ("x-toolstow-client", "1"),
        ]);
        assert_eq!(
            verify_csrf_headers(&mismatched_host)
                .unwrap_err()
                .status_code(),
            StatusCode::FORBIDDEN
        );

        let valid_same_origin = headers_from(&[
            ("host", "127.0.0.1:8080"),
            ("origin", "http://127.0.0.1:8080"),
            ("x-toolstow-client", "1"),
        ]);
        assert!(verify_csrf_headers(&valid_same_origin).is_ok());

        let default_port_match = headers_from(&[
            ("host", "tools.local"),
            ("origin", "https://tools.local:443"),
            ("x-toolstow-client", "1"),
        ]);
        assert!(verify_csrf_headers(&default_port_match).is_ok());
    }

    #[test]
    fn is_request_https_detects_proxy_headers() {
        assert!(!is_request_https(&HeaderMap::new()));
        assert!(!is_request_https(&headers_from(&[(
            "x-forwarded-proto",
            "http",
        )])));
        assert!(is_request_https(&headers_from(&[(
            "x-forwarded-proto",
            "https",
        )])));
        assert!(is_request_https(&headers_from(&[(
            "x-forwarded-proto",
            "HTTPS, http",
        )])));
        assert!(is_request_https(&headers_from(&[(
            "forwarded",
            "for=192.0.2.60;proto=https;by=203.0.113.43",
        )])));
        assert!(is_request_https(&headers_from(&[(
            "forwarded",
            "for=192.0.2.60; PROTO=\"https\"",
        )])));
        assert!(!is_request_https(&headers_from(&[(
            "forwarded",
            "for=192.0.2.60;proto=http",
        )])));
    }
}
