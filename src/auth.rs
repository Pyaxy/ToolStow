//! Dual-key authentication (`TOOLSTOW_ADMIN_KEY` session cookies + `TOOLSTOW_API_KEY` Bearer tokens)
//! and per-client authentication failure rate limiting.

use std::collections::HashMap;
use std::fmt;
use std::hint::black_box;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use axum::http::header::{AUTHORIZATION, COOKIE};
use tracing::warn;
use uuid::Uuid;

use crate::error::AppError;

/// HTTP cookie name storing the human admin session token.
pub const SESSION_COOKIE_NAME: &str = "toolstow_admin_session";
/// Lifetime of an in-memory admin session in seconds (24 hours).
pub const SESSION_TTL_SECS: u64 = 24 * 60 * 60;
/// Recommended minimum length for `TOOLSTOW_ADMIN_KEY` and `TOOLSTOW_API_KEY`.
pub const MIN_RECOMMENDED_KEY_LEN: usize = 16;

/// Maximum failed auth attempts permitted per client within [`RATE_LIMIT_WINDOW_SECS`].
const RATE_LIMIT_MAX_FAILURES: usize = 5;
/// Sliding rate-limit window duration in seconds (5 minutes).
const RATE_LIMIT_WINDOW_SECS: u64 = 5 * 60;

/// Compares two byte slices in constant time with respect to their contents using XOR folding.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let max_len = a.len().max(b.len());
    let mut diff: u8 = if a.len() == b.len() { 0 } else { 1 };

    for i in 0..max_len {
        let x = if i < a.len() { a[i] } else { 0 };
        let y = if i < b.len() { b[i] } else { 0 };
        diff |= black_box(x ^ y);
    }

    black_box(diff) == 0
}

/// Per-session state storing expiration and issued per-tool read-only capability grants.
#[derive(Debug)]
struct SessionRecord {
    expires_at: Instant,
    /// Maps `tool_id -> grant_token` issued for this session.
    tool_grants: HashMap<String, String>,
}

/// Reverse lookup entry binding a capability grant token back to its owning session and tool.
#[derive(Debug, Clone)]
struct ToolGrant {
    session_token: String,
    tool_id: String,
}

/// Combined in-memory session and capability-grant store guarded under a single lock so
/// session revocation and expiration always prune grants atomically.
#[derive(Debug, Default)]
struct SessionStore {
    sessions: HashMap<String, SessionRecord>,
    grants: HashMap<String, ToolGrant>,
}

/// Manages dual-key authentication, in-memory admin sessions, and per-client failure rate limiting.
///
/// - `admin_key` (`TOOLSTOW_ADMIN_KEY`) grants human browser login at `/admin` via a 256-bit
///   random `HttpOnly; SameSite=Strict` session cookie (24h TTL).
/// - `api_key` (`TOOLSTOW_API_KEY`) grants machine `Authorization: Bearer <token>` access to
///   tool management endpoints and CANNOT be used to log in to `/admin`.
#[derive(Clone)]
pub struct AuthManager {
    admin_key: Option<String>,
    api_key: Option<String>,
    cookie_secure: bool,
    sessions: Arc<RwLock<SessionStore>>,
    failures: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl fmt::Debug for AuthManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthManager")
            .field("admin_enabled", &self.admin_key.is_some())
            .field("api_enabled", &self.api_key.is_some())
            .field("cookie_secure", &self.cookie_secure)
            .finish()
    }
}

impl AuthManager {
    /// Creates a new [`AuthManager`]. Blank or whitespace-only keys are treated as disabled (`None`).
    pub fn new(admin_key: Option<String>, api_key: Option<String>, cookie_secure: bool) -> Self {
        let normalize = |opt: Option<String>| -> Option<String> {
            opt.map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };

        Self {
            admin_key: normalize(admin_key),
            api_key: normalize(api_key),
            cookie_secure,
            sessions: Arc::new(RwLock::new(SessionStore::default())),
            failures: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns `true` when `TOOLSTOW_ADMIN_KEY` is configured.
    pub fn is_admin_enabled(&self) -> bool {
        self.admin_key.is_some()
    }

    /// Returns `true` when `TOOLSTOW_API_KEY` is configured.
    pub fn is_api_enabled(&self) -> bool {
        self.api_key.is_some()
    }

    /// Returns `true` when at least one of admin or API authentication is enabled.
    pub fn is_any_management_enabled(&self) -> bool {
        self.is_admin_enabled() || self.is_api_enabled()
    }

    /// Returns whether the `Secure` cookie attribute is forced on all session cookies.
    pub fn cookie_secure(&self) -> bool {
        self.cookie_secure
    }

    /// Logs a warning if either configured key is shorter than [`MIN_RECOMMENDED_KEY_LEN`].
    ///
    /// Never logs key values.
    pub fn warn_weak_keys(&self) {
        if let Some(ref key) = self.admin_key
            && key.len() < MIN_RECOMMENDED_KEY_LEN
        {
            warn!(
                min_recommended_len = MIN_RECOMMENDED_KEY_LEN,
                actual_len = key.len(),
                "TOOLSTOW_ADMIN_KEY is shorter than recommended minimum length; consider generating a stronger key"
            );
        }

        if let Some(ref key) = self.api_key
            && key.len() < MIN_RECOMMENDED_KEY_LEN
        {
            warn!(
                min_recommended_len = MIN_RECOMMENDED_KEY_LEN,
                actual_len = key.len(),
                "TOOLSTOW_API_KEY is shorter than recommended minimum length; consider generating a stronger key"
            );
        }
    }

    /// Authenticates a human admin key and issues a new 256-bit hex session token (24h TTL).
    ///
    /// - Returns `404 Not Found` if admin login is disabled.
    /// - Returns `429 Too Many Requests` after 5 failed attempts by `client_key` within 5 minutes.
    /// - Returns `401 Unauthorized` on key mismatch.
    pub fn login_admin(&self, client_key: &str, candidate: &str) -> Result<String, AppError> {
        let Some(ref expected) = self.admin_key else {
            return Err(AppError::not_found("admin login is disabled"));
        };

        self.check_rate_limit(client_key)?;

        if !constant_time_eq(candidate.as_bytes(), expected.as_bytes()) {
            self.record_failure(client_key);
            return Err(AppError::unauthorized("invalid admin key"));
        }

        self.clear_failures(client_key);

        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let now = Instant::now();
        let expires_at = now + Duration::from_secs(SESSION_TTL_SECS);

        {
            let mut store = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            let expired_tokens: Vec<String> = store
                .sessions
                .iter()
                .filter(|(_, rec)| rec.expires_at <= now)
                .map(|(tok, _)| tok.clone())
                .collect();
            for expired in expired_tokens {
                if let Some(rec) = store.sessions.remove(&expired) {
                    for grant_token in rec.tool_grants.into_values() {
                        store.grants.remove(&grant_token);
                    }
                }
            }
            store.sessions.insert(
                token.clone(),
                SessionRecord {
                    expires_at,
                    tool_grants: HashMap::new(),
                },
            );
        }

        Ok(token)
    }

    /// Invalidates a single admin session token and immediately revokes all capability grants
    /// issued for that session.
    pub fn logout_session(&self, token: &str) {
        let mut store = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        if let Some(rec) = store.sessions.remove(token) {
            for grant_token in rec.tool_grants.into_values() {
                store.grants.remove(&grant_token);
            }
        }
    }

    /// Extracts the value of the [`SESSION_COOKIE_NAME`] cookie from `headers`, if present.
    pub fn extract_session_cookie(headers: &HeaderMap) -> Option<String> {
        for header_value in headers.get_all(COOKIE) {
            let Ok(cookie_header) = header_value.to_str() else {
                continue;
            };
            for pair in cookie_header.split(';') {
                let Some((raw_name, raw_val)) = pair.split_once('=') else {
                    continue;
                };
                if raw_name.trim() == SESSION_COOKIE_NAME {
                    let val = raw_val.trim();
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
        None
    }

    /// Returns `true` if `headers` contains an active, non-expired admin session cookie.
    pub fn has_valid_admin_session(&self, headers: &HeaderMap) -> bool {
        if !self.is_admin_enabled() {
            return false;
        }
        let Some(token) = Self::extract_session_cookie(headers) else {
            return false;
        };

        let now = Instant::now();
        let store = self.sessions.read().unwrap_or_else(|e| e.into_inner());
        matches!(store.sessions.get(&token), Some(rec) if rec.expires_at > now)
    }

    /// Returns an existing or newly generated 64-char lowercase hex read-only capability grant
    /// bound to `(session_token, tool_id)` when `headers` carries a valid non-expired admin session.
    pub fn issue_tool_grant(&self, headers: &HeaderMap, tool_id: &str) -> Option<String> {
        if !self.is_admin_enabled() || tool_id.is_empty() {
            return None;
        }
        let session_token = Self::extract_session_cookie(headers)?;
        let now = Instant::now();

        // Fast path: reuse existing grant under a shared read lock.
        {
            let store = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            let rec = store.sessions.get(&session_token)?;
            if rec.expires_at <= now {
                return None;
            }
            if let Some(existing) = rec.tool_grants.get(tool_id) {
                return Some(existing.clone());
            }
        }

        // Slow path: issue a fresh 256-bit grant under a write lock.
        let mut store = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        let rec = store.sessions.get_mut(&session_token)?;
        if rec.expires_at <= now {
            return None;
        }
        if let Some(existing) = rec.tool_grants.get(tool_id) {
            return Some(existing.clone());
        }

        let grant = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        rec.tool_grants.insert(tool_id.to_string(), grant.clone());
        store.grants.insert(
            grant.clone(),
            ToolGrant {
                session_token,
                tool_id: tool_id.to_string(),
            },
        );
        Some(grant)
    }

    /// Verifies that `grant` is a well-formed 64-char lowercase hex capability token bound to
    /// `tool_id` and that its owning admin session is still active and non-expired.
    pub fn verify_tool_grant(&self, grant: &str, tool_id: &str) -> bool {
        if !self.is_admin_enabled()
            || grant.len() != 64
            || !grant
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return false;
        }

        let now = Instant::now();
        let store = self.sessions.read().unwrap_or_else(|e| e.into_inner());
        let Some(tool_grant) = store.grants.get(grant) else {
            return false;
        };
        if tool_grant.tool_id != tool_id {
            return false;
        }
        matches!(
            store.sessions.get(&tool_grant.session_token),
            Some(rec) if rec.expires_at > now
        )
    }

    /// Verifies an `Authorization: Bearer <token>` header against `TOOLSTOW_API_KEY`.
    ///
    /// - Returns `Ok(false)` when no `Authorization` header is present.
    /// - Returns `Err(AppError::Unauthorized)` for non-Bearer schemes or when API auth is disabled.
    /// - Returns `Err(AppError::TooManyRequests)` if `client_key` exceeded 5 failures within 5 minutes.
    /// - Returns `Ok(true)` on constant-time match, or `Err(AppError::Unauthorized)` on mismatch.
    pub fn verify_bearer_api_key(
        &self,
        client_key: &str,
        headers: &HeaderMap,
    ) -> Result<bool, AppError> {
        let Some(auth_header) = headers.get(AUTHORIZATION) else {
            return Ok(false);
        };

        let raw = auth_header
            .to_str()
            .map_err(|_| AppError::unauthorized("invalid Authorization header"))?
            .trim();

        let Some((scheme, rest)) = raw.split_once(char::is_whitespace) else {
            return Err(AppError::unauthorized(
                "Authorization header must use Bearer scheme",
            ));
        };

        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(AppError::unauthorized(
                "Authorization header must use Bearer scheme",
            ));
        }

        let token = rest.trim();
        if token.is_empty() {
            return Err(AppError::unauthorized("missing Bearer token"));
        }

        let Some(ref expected) = self.api_key else {
            return Err(AppError::unauthorized(
                "api key authentication is disabled",
            ));
        };

        self.check_rate_limit(client_key)?;

        if !constant_time_eq(token.as_bytes(), expected.as_bytes()) {
            self.record_failure(client_key);
            return Err(AppError::unauthorized("invalid API key"));
        }

        self.clear_failures(client_key);
        Ok(true)
    }

    /// Builds the `Set-Cookie` header value establishing an admin session cookie.
    pub fn build_session_cookie_header(&self, token: &str, is_https: bool) -> String {
        let mut header = format!(
            "{SESSION_COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={SESSION_TTL_SECS}"
        );
        if self.cookie_secure || is_https {
            header.push_str("; Secure");
        }
        header
    }

    /// Builds the `Set-Cookie` header value clearing the admin session cookie.
    pub fn build_clear_cookie_header(&self, is_https: bool) -> String {
        let mut header =
            format!("{SESSION_COOKIE_NAME}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0");
        if self.cookie_secure || is_https {
            header.push_str("; Secure");
        }
        header
    }

    /// Checks whether `client_key` is currently rate-limited.
    fn check_rate_limit(&self, client_key: &str) -> Result<(), AppError> {
        let now = Instant::now();
        let window = Duration::from_secs(RATE_LIMIT_WINDOW_SECS);
        let mut guard = self.failures.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(timestamps) = guard.get_mut(client_key) {
            timestamps.retain(|ts| now.saturating_duration_since(*ts) < window);
            if timestamps.len() >= RATE_LIMIT_MAX_FAILURES {
                return Err(AppError::too_many_requests(
                    "too many failed authentication attempts; please try again later",
                ));
            }
            if timestamps.is_empty() {
                guard.remove(client_key);
            }
        }

        Ok(())
    }

    /// Records a failed authentication attempt for `client_key`.
    fn record_failure(&self, client_key: &str) {
        let now = Instant::now();
        let window = Duration::from_secs(RATE_LIMIT_WINDOW_SECS);
        let mut guard = self.failures.lock().unwrap_or_else(|e| e.into_inner());

        let timestamps = guard.entry(client_key.to_string()).or_default();
        timestamps.retain(|ts| now.saturating_duration_since(*ts) < window);
        timestamps.push(now);
    }

    /// Clears recorded authentication failures for `client_key` after a successful login/auth.
    fn clear_failures(&self, client_key: &str) {
        let mut guard = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(client_key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderValue, StatusCode};

    fn headers_with_cookie(cookie: &str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(COOKIE, HeaderValue::from_str(cookie).unwrap());
        map
    }

    fn headers_with_auth(auth: &str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(AUTHORIZATION, HeaderValue::from_str(auth).unwrap());
        map
    }

    #[test]
    fn constant_time_eq_checks_equality_and_lengths() {
        assert!(constant_time_eq(b"", b""));
        assert!(constant_time_eq(b"secret-key-123", b"secret-key-123"));
        assert!(!constant_time_eq(b"secret-key-123", b"secret-key-124"));
        assert!(!constant_time_eq(b"secret", b"secret-longer"));
        assert!(!constant_time_eq(b"secret-longer", b"secret"));
        assert!(!constant_time_eq(b"", b"a"));
    }

    #[test]
    fn auth_manager_flags_and_redacted_debug() {
        let disabled = AuthManager::new(None, Some("   ".to_string()), false);
        assert!(!disabled.is_admin_enabled());
        assert!(!disabled.is_api_enabled());
        assert!(!disabled.is_any_management_enabled());
        assert!(!disabled.cookie_secure());

        let enabled = AuthManager::new(
            Some("admin-super-secret-012345".to_string()),
            Some("api-super-secret-67890".to_string()),
            true,
        );
        assert!(enabled.is_admin_enabled());
        assert!(enabled.is_api_enabled());
        assert!(enabled.is_any_management_enabled());
        assert!(enabled.cookie_secure());

        let debug_str = format!("{enabled:?}");
        assert!(!debug_str.contains("admin-super-secret-012345"));
        assert!(!debug_str.contains("api-super-secret-67890"));
    }

    #[test]
    fn admin_login_session_and_logout_lifecycle() {
        // Disabled admin login returns 404
        let disabled = AuthManager::new(None, None, false);
        let err = disabled.login_admin("127.0.0.1", "anything").unwrap_err();
        assert_eq!(err.status_code(), StatusCode::NOT_FOUND);

        let auth = AuthManager::new(Some("my-strong-admin-key-1234".to_string()), None, false);

        // Wrong key -> 401
        let err = auth.login_admin("127.0.0.1", "wrong-key").unwrap_err();
        assert_eq!(err.status_code(), StatusCode::UNAUTHORIZED);
        assert_eq!(err.error_code(), "unauthorized");

        // Correct key -> 64 hex char token (256 bits)
        let token = auth
            .login_admin("127.0.0.1", "my-strong-admin-key-1234")
            .expect("valid admin login");
        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));

        // Valid session cookie recognized
        let headers = headers_with_cookie(&format!("other=1; {SESSION_COOKIE_NAME}={token}"));
        assert!(auth.has_valid_admin_session(&headers));

        // Unknown token rejected
        let bad_headers = headers_with_cookie(&format!("{SESSION_COOKIE_NAME}=not-a-valid-token"));
        assert!(!auth.has_valid_admin_session(&bad_headers));

        // Logout revokes session
        auth.logout_session(&token);
        assert!(!auth.has_valid_admin_session(&headers));
    }

    #[test]
    fn rate_limits_after_five_failures_per_client() {
        let auth = AuthManager::new(
            Some("correct-admin-key-123456".to_string()),
            Some("correct-api-key-654321".to_string()),
            false,
        );

        for attempt in 1..=5 {
            let err = auth.login_admin("10.0.0.99", "wrong").unwrap_err();
            assert_eq!(
                err.status_code(),
                StatusCode::UNAUTHORIZED,
                "attempt #{attempt} should return 401"
            );
        }

        // 6th attempt gets 429 even with the right key!
        let err = auth
            .login_admin("10.0.0.99", "correct-admin-key-123456")
            .unwrap_err();
        assert_eq!(err.status_code(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(err.error_code(), "too_many_requests");

        // Same client is also rate-limited on Bearer verification
        let bearer_headers = headers_with_auth("Bearer correct-api-key-654321");
        let err = auth
            .verify_bearer_api_key("10.0.0.99", &bearer_headers)
            .unwrap_err();
        assert_eq!(err.status_code(), StatusCode::TOO_MANY_REQUESTS);

        // Different client is unaffected
        assert!(
            auth.login_admin("10.0.0.100", "correct-admin-key-123456")
                .is_ok()
        );
    }

    #[test]
    fn bearer_api_key_verification_and_key_separation() {
        let auth = AuthManager::new(
            Some("admin-only-secret-123456".to_string()),
            Some("api-only-secret-654321".to_string()),
            false,
        );

        // No Authorization header -> Ok(false)
        assert!(!auth.verify_bearer_api_key("127.0.0.1", &HeaderMap::new()).unwrap());

        // Non-Bearer scheme -> 401
        let basic = headers_with_auth("Basic dXNlcjpwYXNz");
        assert_eq!(
            auth.verify_bearer_api_key("127.0.0.1", &basic)
                .unwrap_err()
                .status_code(),
            StatusCode::UNAUTHORIZED
        );

        // API key CANNOT log into admin!
        assert_eq!(
            auth.login_admin("127.0.0.1", "api-only-secret-654321")
                .unwrap_err()
                .status_code(),
            StatusCode::UNAUTHORIZED
        );

        // Admin key is REJECTED as Bearer API key!
        let admin_as_bearer = headers_with_auth("Bearer admin-only-secret-123456");
        assert_eq!(
            auth.verify_bearer_api_key("127.0.0.1", &admin_as_bearer)
                .unwrap_err()
                .status_code(),
            StatusCode::UNAUTHORIZED
        );

        // Valid API key accepted (case-insensitive Bearer scheme)
        let valid_bearer = headers_with_auth("bearer api-only-secret-654321");
        assert!(
            auth.verify_bearer_api_key("127.0.0.1", &valid_bearer)
                .unwrap()
        );

        // Disabled API key returns 401 when Bearer header supplied
        let api_disabled = AuthManager::new(Some("admin-only-secret-123456".to_string()), None, false);
        let err = api_disabled
            .verify_bearer_api_key("127.0.0.1", &valid_bearer)
            .unwrap_err();
        assert_eq!(err.status_code(), StatusCode::UNAUTHORIZED);
        assert!(err.to_string().contains("disabled"));
    }

    #[test]
    fn cookie_extraction_and_header_formatting() {
        // Multiple Cookie headers + multiple pairs
        let mut headers = HeaderMap::new();
        headers.append(COOKIE, HeaderValue::from_static("theme=dark; foo=bar"));
        headers.append(
            COOKIE,
            HeaderValue::from_static("toolstow_admin_session=abc123; extra=1"),
        );
        assert_eq!(
            AuthManager::extract_session_cookie(&headers).as_deref(),
            Some("abc123")
        );

        let empty_cookie = headers_with_cookie("toolstow_admin_session=   ");
        assert_eq!(AuthManager::extract_session_cookie(&empty_cookie), None);

        // Formatting without forced Secure
        let auth_plain = AuthManager::new(None, None, false);
        assert_eq!(
            auth_plain.build_session_cookie_header("tok123", false),
            "toolstow_admin_session=tok123; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400"
        );
        assert_eq!(
            auth_plain.build_session_cookie_header("tok123", true),
            "toolstow_admin_session=tok123; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400; Secure"
        );
        assert_eq!(
            auth_plain.build_clear_cookie_header(false),
            "toolstow_admin_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"
        );
        assert_eq!(
            auth_plain.build_clear_cookie_header(true),
            "toolstow_admin_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0; Secure"
        );

        // Formatting with cookie_secure = true forces Secure even over plain HTTP
        let auth_secure = AuthManager::new(None, None, true);
        assert!(
            auth_secure
                .build_session_cookie_header("tok123", false)
                .ends_with("; Secure")
        );
        assert!(
            auth_secure
                .build_clear_cookie_header(false)
                .ends_with("; Secure")
        );
    }

    #[test]
    fn tool_grants_issue_reuse_scope_to_tool_and_revoke_on_logout_and_expiry() {
        let auth = AuthManager::new(Some("my-strong-admin-key-1234".to_string()), None, false);

        // No session -> cannot issue grant
        assert!(auth.issue_tool_grant(&HeaderMap::new(), "tool-a").is_none());

        let session1 = auth
            .login_admin("127.0.0.1", "my-strong-admin-key-1234")
            .unwrap();
        let session2 = auth
            .login_admin("127.0.0.2", "my-strong-admin-key-1234")
            .unwrap();
        let headers1 = headers_with_cookie(&format!("{SESSION_COOKIE_NAME}={session1}"));
        let headers2 = headers_with_cookie(&format!("{SESSION_COOKIE_NAME}={session2}"));

        // Issue grant for (session1, tool-a) -> 64 lowercase hex chars
        let grant_a1 = auth
            .issue_tool_grant(&headers1, "tool-a")
            .expect("grant for tool-a");
        assert_eq!(grant_a1.len(), 64);
        assert!(
            grant_a1
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        );

        // Reused for same (session1, tool-a)
        assert_eq!(
            auth.issue_tool_grant(&headers1, "tool-a").as_deref(),
            Some(grant_a1.as_str())
        );

        // Distinct grant for different tool or different session
        let grant_b1 = auth
            .issue_tool_grant(&headers1, "tool-b")
            .expect("grant for tool-b");
        let grant_a2 = auth
            .issue_tool_grant(&headers2, "tool-a")
            .expect("grant for session2 tool-a");
        assert_ne!(grant_a1, grant_b1);
        assert_ne!(grant_a1, grant_a2);

        // Verification checks exact tool_id and rejects malformed/uppercase/cross-tool grants
        assert!(auth.verify_tool_grant(&grant_a1, "tool-a"));
        assert!(!auth.verify_tool_grant(&grant_a1, "tool-b"));
        assert!(!auth.verify_tool_grant(&grant_a1.to_uppercase(), "tool-a"));
        assert!(!auth.verify_tool_grant("short", "tool-a"));
        assert!(auth.verify_tool_grant(&grant_b1, "tool-b"));
        assert!(auth.verify_tool_grant(&grant_a2, "tool-a"));

        // Logout session1 immediately revokes all grants for session1 without touching session2
        auth.logout_session(&session1);
        assert!(!auth.verify_tool_grant(&grant_a1, "tool-a"));
        assert!(!auth.verify_tool_grant(&grant_b1, "tool-b"));
        assert!(auth.issue_tool_grant(&headers1, "tool-a").is_none());
        assert!(auth.verify_tool_grant(&grant_a2, "tool-a"));

        // Expired session grants fail verification and are pruned during login_admin
        {
            let mut store = auth.sessions.write().unwrap();
            let rec = store.sessions.get_mut(&session2).unwrap();
            rec.expires_at = Instant::now() - Duration::from_secs(1);
        }
        assert!(!auth.verify_tool_grant(&grant_a2, "tool-a"));
        assert!(auth.issue_tool_grant(&headers2, "tool-a").is_none());

        let _session3 = auth
            .login_admin("127.0.0.3", "my-strong-admin-key-1234")
            .unwrap();
        let store = auth.sessions.read().unwrap();
        assert!(!store.sessions.contains_key(&session2));
        assert!(!store.grants.contains_key(&grant_a2));
    }
}
