//! Tool `manifest.json` schema, validation, normalization, and sandbox allowlist filtering.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::error::AppError;

/// Default manifest version when omitted or blank.
pub const DEFAULT_VERSION: &str = "0.1.0";
/// Default category when omitted or blank.
pub const DEFAULT_CATEGORY: &str = "未分类";
/// Default icon Emoji when omitted or blank.
pub const DEFAULT_ICON: &str = "🛠️";
/// Default entry HTML file when omitted or blank.
pub const DEFAULT_ENTRY: &str = "index.html";
/// Default runtime mode (Phase 1 only supports `"static"`).
pub const DEFAULT_RUNTIME: &str = "static";
/// Maximum allowed length for a tool `id`.
pub const MAX_ID_LEN: usize = 64;

/// Default `iframe sandbox` permissions applied when `manifest.sandbox` is omitted.
pub const DEFAULT_SANDBOX: &[&str] = &["allow-scripts", "allow-downloads", "allow-forms"];

/// Strict security allowlist for `iframe sandbox` and `Content-Security-Policy: sandbox`.
///
/// High-risk tokens such as `allow-same-origin` and `allow-top-navigation*` are
/// deliberately excluded and stripped with a warning if requested.
pub const SANDBOX_ALLOWLIST: &[&str] = &[
    "allow-scripts",
    "allow-downloads",
    "allow-forms",
    "allow-modals",
    "allow-popups",
    "allow-popups-to-escape-sandbox",
];

/// Raw deserialized representation of a tool's `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawManifest {
    /// Globally unique tool identifier (`^[a-z0-9]+(?:-[a-z0-9]+)*$`, 1..=64).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Optional version string (defaults to `"0.1.0"`).
    #[serde(default)]
    pub version: Option<String>,
    /// Optional one-line description (defaults to `""`).
    #[serde(default)]
    pub description: Option<String>,
    /// Optional category name (defaults to `"未分类"`).
    #[serde(default)]
    pub category: Option<String>,
    /// Optional list of search/filter tags.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Optional Emoji icon or safe relative image path (defaults to `"🛠️"`).
    #[serde(default)]
    pub icon: Option<String>,
    /// Optional relative entry HTML path (defaults to `"index.html"`).
    #[serde(default)]
    pub entry: Option<String>,
    /// Optional runtime mode (`"static"` in Phase 1).
    #[serde(default)]
    pub runtime: Option<String>,
    /// Whether the tool subscribes to host `toolstow:theme` `postMessage` events.
    #[serde(default)]
    pub theme_aware: Option<bool>,
    /// Requested `iframe sandbox` tokens (filtered against [`SANDBOX_ALLOWLIST`]).
    #[serde(default)]
    pub sandbox: Option<Vec<String>>,
    /// Whether this tool is a draft (hidden from public listing and anonymous access).
    #[serde(default)]
    pub draft: bool,
}

/// Validated and normalized tool metadata exposed by the registry and HTTP API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolEntry {
    /// Validated unique tool identifier.
    pub id: String,
    /// Trimmed non-empty display name.
    pub name: String,
    /// Normalized version string.
    pub version: String,
    /// Normalized description string.
    pub description: String,
    /// Normalized category name.
    pub category: String,
    /// Deduplicated non-empty tags in declaration order.
    pub tags: Vec<String>,
    /// Normalized icon value (Emoji or relative path within tool directory).
    pub icon: String,
    /// `true` when `icon` refers to a validated relative file inside the tool directory.
    pub icon_is_path: bool,
    /// Served URL for path-based icons (e.g. `"/t/{id}/assets/icon.svg"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    /// Normalized relative entry HTML path (POSIX `/` separators).
    pub entry: String,
    /// Public URL for loading the tool entry (e.g. `"/t/{id}/index.html"`).
    pub entry_url: String,
    /// Normalized runtime mode (`"static"`).
    pub runtime: String,
    /// Whether the tool supports host light/dark & accent theme synchronization.
    pub theme_aware: bool,
    /// Sanitized `iframe sandbox` tokens guaranteed to be a subset of [`SANDBOX_ALLOWLIST`].
    pub sandbox: Vec<String>,
    /// Whether this tool is marked as a draft (hidden from public listing and anonymous access).
    pub draft: bool,
}

impl ToolEntry {
    /// Formats the `Content-Security-Policy` header value for serving this tool's static assets.
    pub fn csp_sandbox_header(&self) -> String {
        if self.sandbox.is_empty() {
            "sandbox;".to_string()
        } else {
            format!("sandbox {};", self.sandbox.join(" "))
        }
    }
}

/// Validates a tool `id` against `^[a-z0-9]+(?:-[a-z0-9]+)*$` with length `1..=64`.
pub fn validate_id(id: &str) -> Result<(), AppError> {
    if id.is_empty() || id.len() > MAX_ID_LEN {
        return Err(AppError::unprocessable(format!(
            "invalid tool id '{id}': length must be between 1 and {MAX_ID_LEN} characters"
        )));
    }

    let bytes = id.as_bytes();
    if bytes.first() == Some(&b'-') || bytes.last() == Some(&b'-') {
        return Err(AppError::unprocessable(format!(
            "invalid tool id '{id}': must match ^[a-z0-9]+(?:-[a-z0-9]+)*$"
        )));
    }

    let mut prev_hyphen = false;
    for &b in bytes {
        if b == b'-' {
            if prev_hyphen {
                return Err(AppError::unprocessable(format!(
                    "invalid tool id '{id}': must not contain consecutive hyphens"
                )));
            }
            prev_hyphen = true;
        } else if b.is_ascii_lowercase() || b.is_ascii_digit() {
            prev_hyphen = false;
        } else {
            return Err(AppError::unprocessable(format!(
                "invalid tool id '{id}': only lowercase ASCII letters, digits, and single hyphens are allowed"
            )));
        }
    }

    Ok(())
}

/// Validates a relative file path inside a tool directory, rejecting traversal,
/// leading/trailing slashes, backslashes, drive prefixes, URI schemes, and `.`/`..` segments.
pub fn validate_safe_rel_path(path: &str) -> Result<PathBuf, AppError> {
    if path.is_empty() {
        return Err(AppError::unprocessable(
            "relative path must not be empty",
        ));
    }
    if path.bytes().any(|b| b == 0 || b < 0x20 || b == 0x7f) {
        return Err(AppError::unprocessable(format!(
            "invalid relative path '{path}': contains control characters"
        )));
    }
    if path.starts_with('/') || path.ends_with('/') {
        return Err(AppError::unprocessable(format!(
            "invalid relative path '{path}': must not start or end with '/'"
        )));
    }
    if path.contains('\\') {
        return Err(AppError::unprocessable(format!(
            "invalid relative path '{path}': backslashes are not allowed"
        )));
    }
    if path.contains(':') {
        return Err(AppError::unprocessable(format!(
            "invalid relative path '{path}': drive prefixes and URI schemes are not allowed"
        )));
    }

    let mut normalized = PathBuf::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(AppError::unprocessable(format!(
                "invalid relative path '{path}': contains disallowed segment '{segment}'"
            )));
        }
        normalized.push(segment);
    }

    if normalized.as_os_str().is_empty()
        || normalized
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(AppError::unprocessable(format!(
            "invalid relative path '{path}': contains non-normal path components"
        )));
    }

    Ok(normalized)
}

/// Filters requested `sandbox` tokens against [`SANDBOX_ALLOWLIST`].
///
/// - When `tokens` is `None`, returns [`DEFAULT_SANDBOX`].
/// - Unknown or disallowed tokens (such as `allow-same-origin` or `allow-top-navigation*`)
///   are stripped and logged via [`tracing::warn!`].
/// - Allowed tokens are deduplicated while preserving order.
pub fn sanitize_sandbox(tokens: Option<&[String]>) -> Vec<String> {
    let Some(raw_tokens) = tokens else {
        return DEFAULT_SANDBOX.iter().map(|s| (*s).to_string()).collect();
    };

    let mut seen = HashSet::new();
    let mut allowed = Vec::new();

    for raw in raw_tokens {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        if SANDBOX_ALLOWLIST.contains(&token) {
            if seen.insert(token.to_string()) {
                allowed.push(token.to_string());
            }
        } else {
            warn!(
                token = %token,
                "stripping disallowed sandbox token from manifest"
            );
        }
    }

    allowed
}

/// Returns `true` when an `icon` string looks like a file path or URI rather than an Emoji/text glyph.
pub fn is_path_like_icon(icon: &str) -> bool {
    icon.contains('/') || icon.contains('\\') || icon.contains('.') || icon.contains(':')
}

/// Loads, validates, and normalizes `manifest.json` inside `tool_dir`.
///
/// When `expected_dir_name` is `Some(dir_name)` (such as during disk directory scans),
/// `manifest.id` must match `dir_name` exactly.
pub fn load_and_validate_manifest(
    tool_dir: &Path,
    expected_dir_name: Option<&str>,
) -> Result<ToolEntry, AppError> {
    let manifest_path = tool_dir.join("manifest.json");
    let manifest_meta = fs::symlink_metadata(&manifest_path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::unprocessable("missing manifest.json in tool directory")
        } else {
            AppError::unprocessable(format!("failed to inspect manifest.json: {err}"))
        }
    })?;

    if manifest_meta.file_type().is_symlink() || !manifest_meta.file_type().is_file() {
        return Err(AppError::unprocessable(
            "manifest.json must be a regular file (symlinks are not allowed)",
        ));
    }

    let raw_bytes = fs::read(&manifest_path)
        .map_err(|err| AppError::unprocessable(format!("failed to read manifest.json: {err}")))?;

    let raw: RawManifest = serde_json::from_slice(&raw_bytes)
        .map_err(|err| AppError::unprocessable(format!("invalid manifest.json: {err}")))?;

    validate_id(&raw.id)?;

    if let Some(expected) = expected_dir_name
        && raw.id != expected
    {
        return Err(AppError::unprocessable(format!(
            "manifest id '{}' does not match directory name '{}'",
            raw.id, expected
        )));
    }

    let name = raw.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::unprocessable(
            "manifest 'name' must not be empty",
        ));
    }

    let runtime = raw
        .runtime
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_RUNTIME);

    match runtime {
        "static" => {}
        "script" => {
            return Err(AppError::unprocessable(
                "runtime 'script' is not supported in Phase 1 (only runtime 'static' is supported)",
            ));
        }
        other => {
            return Err(AppError::unprocessable(format!(
                "unsupported runtime '{other}': expected 'static'"
            )));
        }
    }

    let version = raw
        .version
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_VERSION)
        .to_string();

    let description = raw
        .description
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();

    let category = raw
        .category
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_CATEGORY)
        .to_string();

    let mut seen_tags = HashSet::new();
    let mut tags = Vec::new();
    if let Some(raw_tags) = raw.tags {
        for tag in raw_tags {
            let trimmed = tag.trim();
            if !trimmed.is_empty() && seen_tags.insert(trimmed.to_string()) {
                tags.push(trimmed.to_string());
            }
        }
    }

    let entry_raw = raw
        .entry
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_ENTRY);
    let entry_rel = validate_safe_rel_path(entry_raw)?;
    ensure_regular_file_within(tool_dir, &entry_rel, "entry")?;
    let entry = rel_path_to_posix(&entry_rel);
    let entry_url = format!("/t/{}/{}", raw.id, entry);

    let icon_raw = raw
        .icon
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_ICON);

    let (icon, icon_is_path, icon_url) = if is_path_like_icon(icon_raw) {
        let icon_rel = validate_safe_rel_path(icon_raw)?;
        ensure_regular_file_within(tool_dir, &icon_rel, "icon")?;
        let posix = rel_path_to_posix(&icon_rel);
        let url = format!("/t/{}/{}", raw.id, posix);
        (posix, true, Some(url))
    } else {
        (icon_raw.to_string(), false, None)
    };

    let theme_aware = raw.theme_aware.unwrap_or(false);
    let sandbox = sanitize_sandbox(raw.sandbox.as_deref());

    Ok(ToolEntry {
        id: raw.id,
        name,
        version,
        description,
        category,
        tags,
        icon,
        icon_is_path,
        icon_url,
        entry,
        entry_url,
        runtime: runtime.to_string(),
        theme_aware,
        sandbox,
        draft: raw.draft,
    })
}

/// Converts a validated relative `Path` into a `/` separated string.
fn rel_path_to_posix(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(os) => os.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Walks each component of `rel` inside `tool_dir` using `symlink_metadata`, ensuring no
/// component is a symlink and the final target is an existing regular file.
fn ensure_regular_file_within(tool_dir: &Path, rel: &Path, field: &str) -> Result<(), AppError> {
    let rel_display = rel_path_to_posix(rel);
    let mut current = tool_dir.to_path_buf();
    let mut last_meta = None;

    for component in rel.components() {
        current.push(component);
        let meta = fs::symlink_metadata(&current).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::unprocessable(format!("{field} file '{rel_display}' does not exist"))
            } else {
                AppError::unprocessable(format!(
                    "failed to inspect {field} path '{rel_display}': {err}"
                ))
            }
        })?;

        if meta.file_type().is_symlink() {
            return Err(AppError::unprocessable(format!(
                "{field} path '{rel_display}' must not contain or point to a symlink"
            )));
        }

        last_meta = Some(meta);
    }

    match last_meta {
        Some(meta) if meta.file_type().is_file() => Ok(()),
        _ => Err(AppError::unprocessable(format!(
            "{field} path '{rel_display}' must be a regular file"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_file(dir: &Path, rel: &str, content: &str) {
        let full = dir.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(full, content).expect("write test file");
    }

    #[test]
    fn validate_id_accepts_valid_identifiers() {
        for valid in [
            "a",
            "0",
            "docker-cheatsheet",
            "dynamic-favicon-v2",
            "tool-123-abc",
            &"a".repeat(64),
        ] {
            assert!(validate_id(valid).is_ok(), "expected '{valid}' to be valid");
        }
    }

    #[test]
    fn validate_id_rejects_invalid_identifiers() {
        let too_long = "a".repeat(65);
        for invalid in [
            "",
            too_long.as_str(),
            "-leading",
            "trailing-",
            "double--hyphen",
            "UpperCase",
            "has_underscore",
            "has space",
            "has.dot",
            "a/b",
            "../escape",
            " leading",
        ] {
            let err = validate_id(invalid).expect_err(&format!("expected '{invalid}' to fail"));
            assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");
        }
    }

    #[test]
    fn validate_safe_rel_path_accepts_safe_paths() {
        assert_eq!(
            validate_safe_rel_path("index.html").unwrap(),
            PathBuf::from("index.html")
        );
        assert_eq!(
            validate_safe_rel_path("assets/icons/logo.svg").unwrap(),
            PathBuf::from("assets/icons/logo.svg")
        );
        assert_eq!(
            validate_safe_rel_path("dist/v1.0/app.html").unwrap(),
            PathBuf::from("dist/v1.0/app.html")
        );
    }

    #[test]
    fn validate_safe_rel_path_rejects_unsafe_paths() {
        for invalid in [
            "",
            "/etc/passwd",
            "index.html/",
            ".",
            "..",
            "../secret.html",
            "a/../b.html",
            "a/./b.html",
            "./index.html",
            "a//b.html",
            "assets\\icon.svg",
            "C:index.html",
            "C:/index.html",
            "http://example.com/a.html",
            "https://example.com/icon.png",
            "javascript:alert(1)",
            "data:image/svg+xml,<svg/>",
            "index\0.html",
        ] {
            assert!(
                validate_safe_rel_path(invalid).is_err(),
                "expected '{invalid}' to be rejected"
            );
        }
    }

    #[test]
    fn sanitize_sandbox_applies_defaults_and_strips_disallowed_tokens() {
        assert_eq!(
            sanitize_sandbox(None),
            vec!["allow-scripts", "allow-downloads", "allow-forms"]
        );

        let empty: Vec<String> = vec![];
        assert!(sanitize_sandbox(Some(&empty)).is_empty());

        let requested: Vec<String> = vec![
            "allow-scripts",
            "allow-same-origin",
            "allow-top-navigation",
            "allow-top-navigation-by-user-activation",
            "allow-popups",
            "allow-scripts",
            "allow-modals",
            "allow-popups-to-escape-sandbox",
            "allow-unknown-token",
            "  ",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        assert_eq!(
            sanitize_sandbox(Some(&requested)),
            vec![
                "allow-scripts",
                "allow-popups",
                "allow-modals",
                "allow-popups-to-escape-sandbox",
            ]
        );
    }

    #[test]
    fn load_minimal_manifest_applies_defaults() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "manifest.json",
            r#"{"id": "hello-tool", "name": "  Hello Tool  "}"#,
        );
        write_file(dir.path(), "index.html", "<h1>Hello</h1>");

        let entry = load_and_validate_manifest(dir.path(), Some("hello-tool")).unwrap();
        assert_eq!(entry.id, "hello-tool");
        assert_eq!(entry.name, "Hello Tool");
        assert_eq!(entry.version, "0.1.0");
        assert_eq!(entry.description, "");
        assert_eq!(entry.category, "未分类");
        assert!(entry.tags.is_empty());
        assert_eq!(entry.icon, "🛠️");
        assert!(!entry.icon_is_path);
        assert_eq!(entry.icon_url, None);
        assert_eq!(entry.entry, "index.html");
        assert_eq!(entry.entry_url, "/t/hello-tool/index.html");
        assert_eq!(entry.runtime, "static");
        assert!(!entry.theme_aware);
        assert_eq!(
            entry.sandbox,
            vec!["allow-scripts", "allow-downloads", "allow-forms"]
        );
        assert_eq!(
            entry.csp_sandbox_header(),
            "sandbox allow-scripts allow-downloads allow-forms;"
        );

        let serialized = serde_json::to_value(&entry).unwrap();
        assert_eq!(serialized["themeAware"], false);
        assert_eq!(serialized["entryUrl"], "/t/hello-tool/index.html");
        assert_eq!(serialized["iconIsPath"], false);
        assert!(serialized.get("iconUrl").is_none());
    }

    #[test]
    fn load_full_manifest_deduplicates_tags_sanitizes_sandbox_and_resolves_icon_path() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "manifest.json",
            r#"{
                "id": "dynamic-favicon",
                "name": "Dynamic Favicon",
                "version": "1.2.3",
                "description": "Generates themed favicons",
                "category": "设计",
                "tags": ["icon", " svg ", "", "icon", "favicon", "svg"],
                "icon": "assets/icon.svg",
                "entry": "app/main.html",
                "runtime": "static",
                "themeAware": true,
                "sandbox": ["allow-scripts", "allow-same-origin", "allow-downloads"]
            }"#,
        );
        write_file(dir.path(), "app/main.html", "<html></html>");
        write_file(dir.path(), "assets/icon.svg", "<svg></svg>");

        let entry = load_and_validate_manifest(dir.path(), None).unwrap();
        assert_eq!(entry.version, "1.2.3");
        assert_eq!(entry.category, "设计");
        assert_eq!(entry.tags, vec!["icon", "svg", "favicon"]);
        assert_eq!(entry.icon, "assets/icon.svg");
        assert!(entry.icon_is_path);
        assert_eq!(
            entry.icon_url.as_deref(),
            Some("/t/dynamic-favicon/assets/icon.svg")
        );
        assert_eq!(entry.entry, "app/main.html");
        assert_eq!(entry.entry_url, "/t/dynamic-favicon/app/main.html");
        assert!(entry.theme_aware);
        assert_eq!(entry.sandbox, vec!["allow-scripts", "allow-downloads"]);

        let serialized = serde_json::to_value(&entry).unwrap();
        assert_eq!(serialized["iconIsPath"], true);
        assert_eq!(
            serialized["iconUrl"],
            "/t/dynamic-favicon/assets/icon.svg"
        );
    }

    #[test]
    fn load_manifest_rejects_script_runtime_with_clear_message() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "manifest.json",
            r#"{"id": "calc", "name": "Calc", "runtime": "script"}"#,
        );
        write_file(dir.path(), "index.html", "<h1>Calc</h1>");

        let err = load_and_validate_manifest(dir.path(), None).unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");
        let msg = err.to_string();
        assert!(
            msg.contains("script") && msg.contains("static"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn load_manifest_rejects_missing_entry_and_mismatched_dir_and_bad_icon() {
        // Missing entry file
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "manifest.json",
            r#"{"id": "missing-entry", "name": "Missing Entry"}"#,
        );
        let err = load_and_validate_manifest(dir.path(), None).unwrap_err();
        assert!(err.to_string().contains("index.html"));

        // Mismatched directory name
        write_file(dir.path(), "index.html", "<p>ok</p>");
        let err = load_and_validate_manifest(dir.path(), Some("other-name")).unwrap_err();
        assert!(err.to_string().contains("does not match directory name"));

        // Missing relative path icon
        write_file(
            dir.path(),
            "manifest.json",
            r#"{"id": "missing-entry", "name": "Missing Icon", "icon": "assets/nope.svg"}"#,
        );
        let err = load_and_validate_manifest(dir.path(), None).unwrap_err();
        assert!(err.to_string().contains("assets/nope.svg"));

        // Traversal icon path
        write_file(
            dir.path(),
            "manifest.json",
            r#"{"id": "missing-entry", "name": "Evil Icon", "icon": "../secret.png"}"#,
        );
        let err = load_and_validate_manifest(dir.path(), None).unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");

        // URI scheme icon
        write_file(
            dir.path(),
            "manifest.json",
            r#"{"id": "missing-entry", "name": "URI Icon", "icon": "javascript:alert(1)"}"#,
        );
        let err = load_and_validate_manifest(dir.path(), None).unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");
    }

    #[test]
    fn load_manifest_rejects_blank_name_and_directory_entry() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "manifest.json",
            r#"{"id": "blank-name", "name": "   "}"#,
        );
        write_file(dir.path(), "index.html", "ok");
        assert!(load_and_validate_manifest(dir.path(), None).is_err());

        // Entry pointing to a directory rather than regular file
        let dir2 = TempDir::new().unwrap();
        write_file(
            dir2.path(),
            "manifest.json",
            r#"{"id": "dir-entry", "name": "Dir Entry", "entry": "sub"}"#,
        );
        fs::create_dir_all(dir2.path().join("sub")).unwrap();
        assert!(load_and_validate_manifest(dir2.path(), None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn load_manifest_rejects_symlink_entry() {
        use std::os::unix::fs::symlink;

        let outside = TempDir::new().unwrap();
        write_file(outside.path(), "secret.html", "secret");

        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "manifest.json",
            r#"{"id": "sym-tool", "name": "Sym Tool"}"#,
        );
        symlink(outside.path().join("secret.html"), dir.path().join("index.html")).unwrap();

        let err = load_and_validate_manifest(dir.path(), None).unwrap_err();
        assert!(
            err.to_string().contains("symlink"),
            "expected symlink rejection, got: {err}"
        );
    }

    #[test]
    fn load_manifest_rejects_missing_or_corrupt_manifest_and_unknown_runtime() {
        // Missing manifest.json
        let empty_dir = TempDir::new().unwrap();
        let err = load_and_validate_manifest(empty_dir.path(), None).unwrap_err();
        assert!(err.to_string().contains("missing manifest.json"));

        // Corrupt JSON
        let corrupt_dir = TempDir::new().unwrap();
        write_file(corrupt_dir.path(), "manifest.json", "{not-json");
        let err = load_and_validate_manifest(corrupt_dir.path(), None).unwrap_err();
        assert!(err.to_string().contains("invalid manifest.json"));

        // Unknown runtime
        let unknown_rt = TempDir::new().unwrap();
        write_file(
            unknown_rt.path(),
            "manifest.json",
            r#"{"id": "wasm-tool", "name": "Wasm", "runtime": "wasm"}"#,
        );
        write_file(unknown_rt.path(), "index.html", "ok");
        let err = load_and_validate_manifest(unknown_rt.path(), None).unwrap_err();
        assert!(err.to_string().contains("unsupported runtime"));
    }

    #[test]
    fn load_manifest_blank_optional_fields_fall_back_to_defaults() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "manifest.json",
            r#"{
                "id": "fallback-tool",
                "name": "Fallback Tool",
                "version": "   ",
                "category": "   ",
                "icon": "   ",
                "entry": "   ",
                "runtime": "   ",
                "sandbox": []
            }"#,
        );
        write_file(dir.path(), "index.html", "<p>ok</p>");

        let entry = load_and_validate_manifest(dir.path(), Some("fallback-tool")).unwrap();
        assert_eq!(entry.version, DEFAULT_VERSION);
        assert_eq!(entry.category, DEFAULT_CATEGORY);
        assert_eq!(entry.icon, DEFAULT_ICON);
        assert!(!entry.icon_is_path);
        assert_eq!(entry.entry, DEFAULT_ENTRY);
        assert_eq!(entry.runtime, DEFAULT_RUNTIME);
        assert!(entry.sandbox.is_empty());
        assert_eq!(entry.csp_sandbox_header(), "sandbox;");
        assert!(!entry.draft);
    }

    #[test]
    fn load_manifest_draft_defaults_false_and_parses_true() {
        let dir_default = TempDir::new().unwrap();
        write_file(
            dir_default.path(),
            "manifest.json",
            r#"{"id": "pub-tool", "name": "Published Tool"}"#,
        );
        write_file(dir_default.path(), "index.html", "<h1>Published</h1>");

        let published = load_and_validate_manifest(dir_default.path(), Some("pub-tool")).unwrap();
        assert!(!published.draft);
        let pub_json = serde_json::to_value(&published).unwrap();
        assert_eq!(pub_json["draft"], false);

        let dir_draft = TempDir::new().unwrap();
        write_file(
            dir_draft.path(),
            "manifest.json",
            r#"{"id": "wip-tool", "name": "WIP Tool", "draft": true}"#,
        );
        write_file(dir_draft.path(), "index.html", "<h1>WIP</h1>");

        let draft = load_and_validate_manifest(dir_draft.path(), Some("wip-tool")).unwrap();
        assert!(draft.draft);
        let draft_json = serde_json::to_value(&draft).unwrap();
        assert_eq!(draft_json["draft"], true);
    }
}
