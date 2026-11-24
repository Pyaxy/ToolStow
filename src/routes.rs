//! Axum HTTP router, API endpoints, CSRF protection, static tool hosting,
//! and embedded frontend shell serving.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::multipart::{Multipart, MultipartError, MultipartRejection};
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path as AxumPath, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HOST, ORIGIN, X_CONTENT_TYPE_OPTIONS};
use axum::http::request::Parts;
use axum::http::uri::Authority;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Json, Redirect, Response};
use axum::routing::{delete, get, post};
use rust_embed::RustEmbed;
use serde::Deserialize;
use tower_http::trace::TraceLayer;

use crate::archive::{self, ArchiveLimits};
use crate::config::Config;
use crate::error::AppError;
use crate::manifest::{self, ToolEntry};
use crate::registry::ToolRegistry;

/// Custom header required on browser-originated mutating requests so cross-origin
/// or sandboxed `multipart/form-data` submissions trigger CORS preflight and are blocked.
pub const CLIENT_HEADER_NAME: &str = "x-toolstow-client";

/// Embedded frontend shell assets from `frontend/`.
///
/// In debug builds `rust-embed` reads files from disk for fast iteration; in
/// release builds assets are compiled directly into the single binary.
#[derive(RustEmbed)]
#[folder = "frontend/"]
struct FrontendAssets;

/// Shared application state injected into Axum route handlers.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Thread-safe in-memory tool registry.
    pub registry: Arc<ToolRegistry>,
    /// Parsed runtime server configuration.
    pub config: Arc<Config>,
}

/// Optional query filters accepted by `GET /api/tools`.
#[derive(Debug, Default, Deserialize)]
struct ListToolsQuery {
    q: Option<String>,
    category: Option<String>,
    tag: Option<String>,
}

/// Request guard enforcing CSRF and Origin checks on mutating API routes.
///
/// Running this as a [`FromRequestParts`] extractor ensures cross-site and
/// sandboxed iframe (`Origin: null`) requests are rejected with `403 Forbidden`
/// before any multipart body is parsed.
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

/// Validates request headers for state-mutating endpoints (`POST /api/tools/upload`,
/// `DELETE /api/tools/{id}`).
///
/// Rules:
/// 1. Reject if `Sec-Fetch-Site` is `"cross-site"`.
/// 2. If `Origin` is present (browser request):
///    - Reject if `Origin` is `"null"` (sandboxed opaque origin).
///    - Verify `Origin`'s host and effective port match the `Host` header.
///    - Require a non-empty `X-ToolStow-Client` header.
/// 3. Non-browser CLI / `curl` requests (no `Origin` header and not `Sec-Fetch-Site: cross-site`)
///    are permitted.
fn verify_csrf_headers(headers: &HeaderMap) -> Result<(), AppError> {
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
            return Err(AppError::forbidden("Origin header must not include userinfo"));
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
    }

    Ok(())
}

/// Builds the full ToolStow Axum [`Router`].
pub fn build_router(state: AppState) -> Router {
    let upload_body_limit = state
        .config
        .max_upload_bytes()
        .saturating_add(1024 * 1024);

    let upload_route = Router::new()
        .route("/api/tools/upload", post(upload_tool))
        .layer(DefaultBodyLimit::max(upload_body_limit));

    Router::new()
        .route("/", get(serve_index))
        .route("/api/tools", get(list_tools))
        .merge(upload_route)
        .route("/api/tools/{id}", delete(delete_tool))
        .route("/t/{id}", get(redirect_tool_entry))
        .route("/t/{id}/", get(redirect_tool_entry))
        .route("/t/{id}/{*path}", get(serve_tool_asset))
        .fallback(serve_frontend_fallback)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

/// `GET /`: serves the embedded `frontend/index.html` shell.
async fn serve_index() -> Result<Response, AppError> {
    serve_embedded_asset("index.html", false)
}

/// Fallback handler serving embedded frontend shell assets without ever shadowing `/api/*` or `/t/*`.
async fn serve_frontend_fallback(method: Method, uri: Uri) -> Result<Response, AppError> {
    let raw_path = uri.path();

    if raw_path == "/api"
        || raw_path.starts_with("/api/")
        || raw_path == "/t"
        || raw_path.starts_with("/t/")
    {
        return Err(AppError::not_found(format!("route not found: {raw_path}")));
    }

    if method != Method::GET && method != Method::HEAD {
        return Err(AppError::not_found(format!("route not found: {raw_path}")));
    }

    let rel_path = raw_path.trim_start_matches('/');
    let rel_path = if rel_path.is_empty() {
        "index.html"
    } else {
        rel_path
    };

    serve_embedded_asset(rel_path, method == Method::HEAD)
}

/// Serves a single file from [`FrontendAssets`].
fn serve_embedded_asset(path: &str, head_only: bool) -> Result<Response, AppError> {
    let asset = FrontendAssets::get(path)
        .ok_or_else(|| AppError::not_found(format!("frontend asset not found: /{path}")))?;

    let content_type = content_type_for_path(Path::new(path));
    let body = if head_only {
        Body::empty()
    } else {
        Body::from(asset.data.into_owned())
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"))
        .header(CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .body(body)
        .map_err(|err| AppError::internal(format!("failed to build frontend response: {err}")))
}

/// `GET /api/tools`: returns sorted tool entries filtered by optional `q`, `category`, and `tag`.
async fn list_tools(
    State(state): State<AppState>,
    Query(query): Query<ListToolsQuery>,
) -> Json<Vec<ToolEntry>> {
    let q = normalize_query_param(query.q.as_deref());
    let category = normalize_query_param(query.category.as_deref());
    let tag = normalize_query_param(query.tag.as_deref());

    let tools = state.registry.list(q, category, tag);
    Json(tools)
}

/// `POST /api/tools/upload`: validates and atomically installs or updates a `.zip` tool package.
async fn upload_tool(
    _csrf: CsrfGuard,
    State(state): State<AppState>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<(StatusCode, Json<ToolEntry>), AppError> {
    let mut multipart = multipart.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            AppError::payload_too_large(format!(
                "upload exceeds maximum allowed size of {} MiB",
                state.config.max_upload_mb
            ))
        } else {
            AppError::bad_request(format!("invalid multipart request: {}", rejection.body_text()))
        }
    })?;

    let max_upload_bytes = state.config.max_upload_bytes();
    let max_upload_mb = state.config.max_upload_mb;
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

    let _write_guard = state.registry.write_lock().lock_owned().await;
    let tools_dir = state.registry.tools_dir().to_path_buf();
    let limits = ArchiveLimits {
        max_zip_bytes: max_upload_bytes,
        ..ArchiveLimits::default()
    };

    let result = tokio::task::spawn_blocking(move || {
        archive::install_zip_bytes(&tools_dir, &zip_bytes, limits)
    })
    .await
    .map_err(|err| AppError::internal(format!("archive install task failed: {err}")))??;

    state.registry.upsert(result.entry.clone());

    let status = if result.updated {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };

    Ok((status, Json(result.entry)))
}

/// `DELETE /api/tools/{id}`: deletes a tool directory from disk and removes it from the registry.
async fn delete_tool(
    _csrf: CsrfGuard,
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
async fn redirect_tool_entry(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Response, AppError> {
    let entry = state
        .registry
        .get(&id)
        .ok_or_else(|| AppError::not_found(format!("tool '{id}' not found")))?;

    Ok(Redirect::temporary(&entry.entry_url).into_response())
}

/// `GET /t/{id}/{*path}`: serves a static asset inside a registered tool directory with strict
/// path containment and `Content-Security-Policy: sandbox` isolation headers.
async fn serve_tool_asset(
    State(state): State<AppState>,
    AxumPath((id, path)): AxumPath<(String, String)>,
) -> Result<Response, AppError> {
    let entry = state
        .registry
        .get(&id)
        .ok_or_else(|| AppError::not_found(format!("tool '{id}' not found")))?;

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

    let canonical_target = tokio::fs::canonicalize(&candidate_path).await.map_err(|err| {
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

    let metadata = tokio::fs::metadata(&canonical_target).await.map_err(|err| {
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

/// Converts blank/whitespace query parameters into `None`.
fn normalize_query_param(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
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
}
