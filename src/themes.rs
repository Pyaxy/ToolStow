//! Multi-theme manifest validation, discovery, preview resolution, and safe `.zip` installation.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{error, warn};
use uuid::Uuid;
use zip::ZipArchive;

use crate::archive::ArchiveLimits;
use crate::error::AppError;
use crate::manifest::validate_safe_rel_path;

/// Reserved short identifier for the built-in default theme.
pub const BUILTIN_DEFAULT_THEME: &str = "default";

/// Maximum allowed length for a theme `short` identifier.
const MAX_THEME_SHORT_LEN: usize = 64;
/// Default theme version when omitted or blank in `theme.json`.
const DEFAULT_THEME_VERSION: &str = "0.1.0";
/// Hidden subdirectory under `themes_dir` used for same-filesystem staging, backup, and trash.
const STAGING_DIR_NAME: &str = ".staging";
/// Buffer size used when streaming decompressed bytes out of a `.zip` entry.
const COPY_BUF_SIZE: usize = 16 * 1024;

/// Unix file type bitmask (`S_IFMT`).
const UNIX_FILE_TYPE_MASK: u32 = 0o170_000;
/// Unix symbolic link file type (`S_IFLNK`).
const UNIX_SYMLINK_TYPE: u32 = 0o120_000;
/// Unix regular file type (`S_IFREG`).
const UNIX_REGULAR_FILE_TYPE: u32 = 0o100_000;
/// Unix directory file type (`S_IFDIR`).
const UNIX_DIR_TYPE: u32 = 0o040_000;

/// Raw deserialized representation of a theme's `theme.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawThemeManifest {
    /// Unique theme short identifier (`^[a-z0-9]+(?:-[a-z0-9]+)*$`, 1..=64, not `"default"`).
    short: String,
    /// Human-readable display name.
    name: String,
    /// Optional version string (defaults to `"0.1.0"`).
    #[serde(default)]
    version: Option<String>,
    /// Optional theme description (defaults to `""`).
    #[serde(default)]
    description: Option<String>,
    /// Optional theme author (defaults to `""`).
    #[serde(default)]
    author: Option<String>,
    /// Optional homepage/repository URL (`http://` or `https://`, defaults to `""`).
    #[serde(default)]
    url: Option<String>,
    /// Optional relative path to a preview image inside the theme directory.
    #[serde(default)]
    preview: Option<String>,
}

/// Validated metadata describing a built-in or installed theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeEntry {
    /// Theme short identifier (`"default"` for built-in, or validated slug).
    pub short: String,
    /// Trimmed non-empty display name.
    pub name: String,
    /// Normalized version string.
    pub version: String,
    /// Normalized description string.
    pub description: String,
    /// Normalized author string.
    pub author: String,
    /// Normalized homepage URL (`""` or `http://`/`https://`).
    pub url: String,
    /// `true` for the embedded built-in theme, `false` for disk-installed themes.
    pub builtin: bool,
    /// `true` when this theme is currently active.
    pub active: bool,
    /// `true` when this theme provides a custom public HTML shell (`dist/index.html` or `index.html`).
    pub has_custom_shell: bool,
    /// `true` when this theme provides a `ui.css` stylesheet.
    pub has_ui_css: bool,
    /// Admin API URL for the theme's preview image, when present.
    pub preview_url: Option<String>,
}

/// Validated on-disk theme along with resolved filesystem paths for serving assets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTheme {
    /// Validated theme metadata.
    pub entry: ThemeEntry,
    /// Root directory of the installed theme (`<themes_dir>/<short>`).
    pub root_dir: PathBuf,
    /// Static web root for serving custom shell assets (`dist/` if `dist/index.html` exists,
    /// else `root_dir` if `index.html` exists, else `None` for skin-only themes).
    pub web_root: Option<PathBuf>,
    /// Path to `<root_dir>/ui.css` if present as a regular file.
    pub ui_css_path: Option<PathBuf>,
}

/// Outcome of [`install_theme_zip_checked`], distinguishing a completed install from an
/// existing-theme conflict when overwrite is not permitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeInstallOutcome {
    /// Archive passed validation and was installed or atomically replaced.
    Installed {
        /// Validated metadata of the installed theme.
        entry: ThemeEntry,
        /// `true` if an existing theme with the same `short` was atomically replaced.
        updated: bool,
    },
    /// A theme with the same `short` already exists and `allow_overwrite` was `false`.
    AlreadyExists {
        /// Currently installed theme metadata (if its existing `theme.json` is valid).
        existing: Option<Box<ThemeEntry>>,
        /// Validated metadata of the newly uploaded archive.
        incoming: ThemeEntry,
    },
}

/// Inspected metadata for a non-ignored zip entry prior to extraction.
#[derive(Debug)]
struct InspectedEntry {
    index: usize,
    path: PathBuf,
    is_dir: bool,
}

/// Normalized extraction plan entry after optional single-folder unwrapping.
#[derive(Debug)]
struct PlannedEntry {
    index: usize,
    rel_path: PathBuf,
    is_dir: bool,
}

/// RAII guard that removes a persisted staging directory if an error occurs before rename succeeds.
struct CleanupPath(Option<PathBuf>);

impl CleanupPath {
    fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for CleanupPath {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// Validates a theme `short` identifier against `^[a-z0-9]+(?:-[a-z0-9]+)*$` (1..=64 chars)
/// and rejects the reserved `"default"` identifier.
pub fn validate_theme_short(short: &str) -> Result<(), AppError> {
    if short == BUILTIN_DEFAULT_THEME {
        return Err(AppError::unprocessable(
            "theme short 'default' is reserved for the built-in theme",
        ));
    }

    if short.is_empty() || short.len() > MAX_THEME_SHORT_LEN {
        return Err(AppError::unprocessable(format!(
            "invalid theme short '{short}': length must be between 1 and {MAX_THEME_SHORT_LEN} characters"
        )));
    }

    let bytes = short.as_bytes();
    if bytes.first() == Some(&b'-') || bytes.last() == Some(&b'-') {
        return Err(AppError::unprocessable(format!(
            "invalid theme short '{short}': must match ^[a-z0-9]+(?:-[a-z0-9]+)*$"
        )));
    }

    let mut prev_hyphen = false;
    for &b in bytes {
        if b == b'-' {
            if prev_hyphen {
                return Err(AppError::unprocessable(format!(
                    "invalid theme short '{short}': must not contain consecutive hyphens"
                )));
            }
            prev_hyphen = true;
        } else if b.is_ascii_lowercase() || b.is_ascii_digit() {
            prev_hyphen = false;
        } else {
            return Err(AppError::unprocessable(format!(
                "invalid theme short '{short}': only lowercase ASCII letters, digits, and single hyphens are allowed"
            )));
        }
    }

    Ok(())
}

/// Returns the metadata entry for the built-in default theme.
pub fn builtin_default_entry(active: bool) -> ThemeEntry {
    ThemeEntry {
        short: BUILTIN_DEFAULT_THEME.to_string(),
        name: "Default Monochrome".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        description: "Built-in minimalist black & white theme inspired by monitor-probe/monitor"
            .to_string(),
        author: "ToolStow".to_string(),
        url: String::new(),
        builtin: true,
        active,
        has_custom_shell: true,
        has_ui_css: true,
        preview_url: None,
    }
}

/// Loads, validates, and resolves a theme directory containing `theme.json`.
///
/// When `expected_short` is `Some(dir_name)` (such as during disk directory scans),
/// `theme.json`'s `short` field must match `dir_name` exactly.
pub fn load_and_validate_theme(
    theme_dir: &Path,
    expected_short: Option<&str>,
    active: bool,
) -> Result<ResolvedTheme, AppError> {
    let dir_meta = fs::symlink_metadata(theme_dir).map_err(|err| {
        AppError::unprocessable(format!(
            "failed to inspect theme directory '{}': {err}",
            theme_dir.display()
        ))
    })?;
    if dir_meta.file_type().is_symlink() || !dir_meta.file_type().is_dir() {
        return Err(AppError::unprocessable(
            "theme directory must be a regular directory (symlinks are not allowed)",
        ));
    }

    let manifest_path = theme_dir.join("theme.json");
    let manifest_meta = fs::symlink_metadata(&manifest_path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::unprocessable("missing theme.json in theme directory")
        } else {
            AppError::unprocessable(format!("failed to inspect theme.json: {err}"))
        }
    })?;

    if manifest_meta.file_type().is_symlink() || !manifest_meta.file_type().is_file() {
        return Err(AppError::unprocessable(
            "theme.json must be a regular file (symlinks are not allowed)",
        ));
    }

    let raw_bytes = fs::read(&manifest_path)
        .map_err(|err| AppError::unprocessable(format!("failed to read theme.json: {err}")))?;

    let raw: RawThemeManifest = serde_json::from_slice(&raw_bytes)
        .map_err(|err| AppError::unprocessable(format!("invalid theme.json: {err}")))?;

    validate_theme_short(&raw.short)?;

    if let Some(expected) = expected_short
        && raw.short != expected
    {
        return Err(AppError::unprocessable(format!(
            "theme short '{}' does not match directory name '{}'",
            raw.short, expected
        )));
    }

    let name = raw.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::unprocessable("theme 'name' must not be empty"));
    }

    let version = raw
        .version
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_THEME_VERSION)
        .to_string();

    let description = raw
        .description
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();

    let author = raw
        .author
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();

    let url = validate_theme_url(raw.url.as_deref().unwrap_or(""))?;

    let preview_url = match raw.preview.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(preview_raw) => {
            let preview_rel = validate_safe_rel_path(preview_raw)?;
            ensure_regular_file_within(theme_dir, &preview_rel, "preview")?;
            Some(format!("/api/admin/themes/{}/preview", raw.short))
        }
        None => None,
    };

    let web_root = detect_web_root(theme_dir)?;
    let ui_css_path = detect_ui_css(theme_dir)?;

    if web_root.is_none() && ui_css_path.is_none() {
        return Err(AppError::unprocessable(
            "theme must provide at least one of 'dist/index.html', 'index.html', or 'ui.css'",
        ));
    }

    let entry = ThemeEntry {
        short: raw.short,
        name,
        version,
        description,
        author,
        url,
        builtin: false,
        active,
        has_custom_shell: web_root.is_some(),
        has_ui_css: ui_css_path.is_some(),
        preview_url,
    };

    Ok(ResolvedTheme {
        entry,
        root_dir: theme_dir.to_path_buf(),
        web_root,
        ui_css_path,
    })
}

/// Lists the built-in default theme first, followed by all valid installed themes in `themes_dir`
/// sorted by `(name.to_lowercase(), short)`.
///
/// Hidden entries (such as `.staging`), symlinks, and invalid theme directories are skipped
/// (with warnings logged for broken themes) without failing the listing. If `active_short` does
/// not match any valid installed theme, the built-in `"default"` theme is marked active.
pub fn list_themes(themes_dir: &Path, active_short: &str) -> Vec<ThemeEntry> {
    let mut installed = Vec::new();

    match fs::read_dir(themes_dir) {
        Ok(read_dir) => {
            for dir_entry_res in read_dir {
                let dir_entry = match dir_entry_res {
                    Ok(e) => e,
                    Err(err) => {
                        warn!(
                            themes_dir = %themes_dir.display(),
                            error = %err,
                            "failed to read entry in themes directory"
                        );
                        continue;
                    }
                };

                let file_name = dir_entry.file_name();
                let Some(dir_name) = file_name.to_str() else {
                    continue;
                };
                if dir_name.starts_with('.') {
                    continue;
                }

                let path = dir_entry.path();
                let meta = match fs::symlink_metadata(&path) {
                    Ok(m) => m,
                    Err(err) => {
                        warn!(
                            theme = %dir_name,
                            path = %path.display(),
                            error = %err,
                            "failed to inspect theme directory entry; skipping"
                        );
                        continue;
                    }
                };
                if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
                    continue;
                }

                let is_active =
                    active_short != BUILTIN_DEFAULT_THEME && dir_name == active_short;
                match load_and_validate_theme(&path, Some(dir_name), is_active) {
                    Ok(resolved) => installed.push(resolved.entry),
                    Err(err) => {
                        warn!(
                            theme = %dir_name,
                            path = %path.display(),
                            error = %err,
                            "skipping invalid theme directory"
                        );
                    }
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            warn!(
                themes_dir = %themes_dir.display(),
                error = %err,
                "failed to scan themes directory"
            );
        }
    }

    installed.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.short.cmp(&b.short))
    });

    let active_found = installed.iter().any(|entry| entry.active);
    let mut result = Vec::with_capacity(installed.len() + 1);
    result.push(builtin_default_entry(!active_found));
    result.extend(installed);
    result
}

/// Resolves the currently active disk-installed theme, or returns `None` when `active_short`
/// is `"default"`, invalid, missing, or broken.
pub fn resolve_active_theme(themes_dir: &Path, active_short: &str) -> Option<ResolvedTheme> {
    if active_short == BUILTIN_DEFAULT_THEME {
        return None;
    }
    if validate_theme_short(active_short).is_err() {
        return None;
    }

    let theme_dir = themes_dir.join(active_short);
    match fs::symlink_metadata(&theme_dir) {
        Ok(meta) if !meta.file_type().is_symlink() && meta.file_type().is_dir() => {}
        Ok(_) => {
            warn!(
                theme = %active_short,
                path = %theme_dir.display(),
                "active theme path is not a regular directory; falling back to default"
            );
            return None;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            warn!(
                theme = %active_short,
                path = %theme_dir.display(),
                error = %err,
                "failed to inspect active theme directory; falling back to default"
            );
            return None;
        }
    }

    match load_and_validate_theme(&theme_dir, Some(active_short), true) {
        Ok(resolved) => Some(resolved),
        Err(err) => {
            warn!(
                theme = %active_short,
                path = %theme_dir.display(),
                error = %err,
                "active theme failed validation; falling back to default"
            );
            None
        }
    }
}

/// Validates `short`, reads the theme manifest, and returns the canonical path to its preview
/// image inside the canonical theme directory.
///
/// Returns `403 Forbidden` if the theme directory, `theme.json`, or preview path uses a symlink
/// or escapes the theme directory; returns `404 Not Found` if the theme, `preview` field, or
/// preview file is missing.
pub fn resolve_theme_preview(themes_dir: &Path, short: &str) -> Result<PathBuf, AppError> {
    validate_theme_short(short)?;

    let theme_dir = themes_dir.join(short);
    let dir_meta = fs::symlink_metadata(&theme_dir).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found(format!("theme '{short}' not found"))
        } else {
            AppError::internal(format!(
                "failed to inspect theme directory '{}': {err}",
                theme_dir.display()
            ))
        }
    })?;
    if dir_meta.file_type().is_symlink() {
        return Err(AppError::forbidden("theme directory must not be a symlink"));
    }
    if !dir_meta.file_type().is_dir() {
        return Err(AppError::not_found(format!("theme '{short}' not found")));
    }

    let manifest_path = theme_dir.join("theme.json");
    let manifest_meta = fs::symlink_metadata(&manifest_path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found(format!("theme '{short}' not found"))
        } else {
            AppError::internal(format!("failed to inspect theme.json: {err}"))
        }
    })?;
    if manifest_meta.file_type().is_symlink() {
        return Err(AppError::forbidden("theme.json must not be a symlink"));
    }
    if !manifest_meta.file_type().is_file() {
        return Err(AppError::unprocessable("theme.json must be a regular file"));
    }

    let raw_bytes = fs::read(&manifest_path)
        .map_err(|err| AppError::internal(format!("failed to read theme.json: {err}")))?;
    let raw: RawThemeManifest = serde_json::from_slice(&raw_bytes)
        .map_err(|err| AppError::unprocessable(format!("invalid theme.json: {err}")))?;

    if raw.short != short {
        return Err(AppError::unprocessable(format!(
            "theme short '{}' does not match directory name '{}'",
            raw.short, short
        )));
    }

    let preview_raw = raw
        .preview
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::not_found(format!("theme '{short}' has no preview image")))?;

    let preview_rel = validate_safe_rel_path(preview_raw).map_err(|_| {
        AppError::forbidden(format!(
            "theme preview path '{preview_raw}' is outside the theme directory"
        ))
    })?;

    // Check every component for symlinks before canonicalizing so broken or escaping symlinks
    // always produce 403 Forbidden rather than 404 Not Found.
    let mut current = theme_dir.clone();
    for component in preview_rel.components() {
        current.push(component);
        let meta = fs::symlink_metadata(&current).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!("theme '{short}' preview image not found"))
            } else {
                AppError::internal(format!("failed to inspect theme preview path: {err}"))
            }
        })?;
        if meta.file_type().is_symlink() {
            return Err(AppError::forbidden(
                "theme preview path must not contain or point to a symlink",
            ));
        }
    }

    let canonical_theme_dir = fs::canonicalize(&theme_dir).map_err(|err| {
        AppError::internal(format!(
            "failed to canonicalize theme directory '{}': {err}",
            theme_dir.display()
        ))
    })?;
    let canonical_target = fs::canonicalize(&current).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found(format!("theme '{short}' preview image not found"))
        } else {
            AppError::internal(format!("failed to canonicalize theme preview path: {err}"))
        }
    })?;

    if !canonical_target.starts_with(&canonical_theme_dir) {
        return Err(AppError::forbidden(
            "theme preview path escapes theme directory",
        ));
    }

    let target_meta = fs::metadata(&canonical_target)
        .map_err(|err| AppError::internal(format!("failed to stat theme preview file: {err}")))?;
    if !target_meta.is_file() {
        return Err(AppError::not_found(format!(
            "theme '{short}' preview is not a regular file"
        )));
    }

    Ok(canonical_target)
}

/// Validates, extracts, and installs a `.zip` theme package into `themes_dir/{short}`.
///
/// When `allow_overwrite` is `false` and `themes_dir/{short}` already exists, the temporary
/// staging directory is cleaned up automatically and [`ThemeInstallOutcome::AlreadyExists`] is
/// returned without modifying the existing theme.
pub fn install_theme_zip_checked(
    themes_dir: &Path,
    zip_bytes: &[u8],
    limits: ArchiveLimits,
    allow_overwrite: bool,
) -> Result<ThemeInstallOutcome, AppError> {
    if zip_bytes.len() > limits.max_zip_bytes {
        return Err(AppError::payload_too_large(format!(
            "uploaded zip archive is {} bytes, exceeding limit of {} bytes",
            zip_bytes.len(),
            limits.max_zip_bytes
        )));
    }

    let staging_root = themes_dir.join(STAGING_DIR_NAME);
    fs::create_dir_all(&staging_root).map_err(|err| {
        AppError::internal(format!(
            "failed to create staging directory '{}': {err}",
            staging_root.display()
        ))
    })?;

    let mut archive = ZipArchive::new(Cursor::new(zip_bytes))
        .map_err(|err| AppError::bad_request(format!("invalid zip archive: {err}")))?;

    // Pass 1: strictly inspect every entry for symlinks, traversal, macOS junk,
    // single-top-level-folder unwrapping, duplicate/conflicting paths, and file count limits.
    let inspected = inspect_archive_entries(&mut archive)?;
    let planned = plan_extraction(inspected, limits.max_files)?;

    // Pass 2: extract into a fresh same-filesystem staging directory while counting actual bytes.
    let stage_dir = tempfile::Builder::new()
        .prefix("stage-")
        .tempdir_in(&staging_root)
        .map_err(|err| {
            AppError::internal(format!(
                "failed to create temporary staging directory in '{}': {err}",
                staging_root.display()
            ))
        })?;

    extract_planned_entries(&mut archive, &planned, stage_dir.path(), limits)?;

    // Validate the extracted theme directory before touching the live themes directory.
    let resolved = load_and_validate_theme(stage_dir.path(), None, false)?;
    let short = resolved.entry.short.clone();
    let target = themes_dir.join(&short);

    let target_exists = match fs::symlink_metadata(&target) {
        Ok(_) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            return Err(AppError::internal(format!(
                "failed to inspect target theme directory '{}': {err}",
                target.display()
            )));
        }
    };

    if !allow_overwrite && target_exists {
        let existing = load_and_validate_theme(&target, Some(&short), false)
            .ok()
            .map(|r| Box::new(r.entry));
        // `stage_dir` drops here, cleaning up the temporary directory without touching `target`.
        return Ok(ThemeInstallOutcome::AlreadyExists {
            existing,
            incoming: resolved.entry,
        });
    }

    // Atomically move the validated staging directory into `themes_dir/{short}`.
    let staged_path = stage_dir.keep();
    let mut cleanup = CleanupPath::new(staged_path.clone());

    let updated = install_staged_dir(&staging_root, &staged_path, &target, &short)?;
    cleanup.disarm();

    Ok(ThemeInstallOutcome::Installed {
        entry: resolved.entry,
        updated,
    })
}

/// Deletes `themes_dir/{short}` after validating `short`, returning `Ok(true)` if removed or
/// `Ok(false)` if absent. Rejects `"default"` with `400 Bad Request`.
pub fn delete_theme_dir(themes_dir: &Path, short: &str) -> Result<bool, AppError> {
    if short == BUILTIN_DEFAULT_THEME {
        return Err(AppError::bad_request(
            "built-in 'default' theme cannot be deleted",
        ));
    }

    validate_theme_short(short)?;

    let target = themes_dir.join(short);
    let meta = match fs::symlink_metadata(&target) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(AppError::internal(format!(
                "failed to inspect theme directory '{}': {err}",
                target.display()
            )));
        }
    };

    let staging_root = themes_dir.join(STAGING_DIR_NAME);
    fs::create_dir_all(&staging_root).map_err(|err| {
        AppError::internal(format!(
            "failed to create staging directory '{}': {err}",
            staging_root.display()
        ))
    })?;

    let trash_path = staging_root.join(format!("trash-{short}-{}", Uuid::new_v4()));
    fs::rename(&target, &trash_path).map_err(|err| {
        AppError::internal(format!(
            "failed to move theme directory '{}' to trash: {err}",
            target.display()
        ))
    })?;

    let remove_result = if meta.is_dir() {
        fs::remove_dir_all(&trash_path)
    } else {
        fs::remove_file(&trash_path)
    };

    if let Err(err) = remove_result {
        return Err(AppError::internal(format!(
            "failed to remove trashed theme directory '{}': {err}",
            trash_path.display()
        )));
    }

    Ok(true)
}

/// Validates a theme `url` field (empty, or `http://`/`https://` without control/whitespace chars).
fn validate_theme_url(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.bytes().any(|b| b <= 0x20 || b == 0x7f) {
        return Err(AppError::unprocessable(
            "theme 'url' must not contain whitespace or control characters",
        ));
    }
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .ok_or_else(|| {
            AppError::unprocessable("theme 'url' must be empty or start with http:// or https://")
        })?;
    if rest.is_empty() {
        return Err(AppError::unprocessable(
            "theme 'url' must include a host after the scheme",
        ));
    }
    Ok(trimmed.to_string())
}

/// Detects `web_root` for a theme (`dist/` if `dist/index.html` exists, else `theme_dir` if
/// `index.html` exists, else `None`), rejecting symlinks.
fn detect_web_root(theme_dir: &Path) -> Result<Option<PathBuf>, AppError> {
    let dist_dir = theme_dir.join("dist");
    match fs::symlink_metadata(&dist_dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(AppError::unprocessable(
                    "theme 'dist' directory must not be a symlink",
                ));
            }
            if meta.file_type().is_dir() {
                let dist_index = dist_dir.join("index.html");
                match fs::symlink_metadata(&dist_index) {
                    Ok(idx_meta) => {
                        if idx_meta.file_type().is_symlink() {
                            return Err(AppError::unprocessable(
                                "theme 'dist/index.html' must not be a symlink",
                            ));
                        }
                        if idx_meta.file_type().is_file() {
                            return Ok(Some(dist_dir));
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => {
                        return Err(AppError::unprocessable(format!(
                            "failed to inspect 'dist/index.html': {err}"
                        )));
                    }
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(AppError::unprocessable(format!(
                "failed to inspect 'dist' directory: {err}"
            )));
        }
    }

    let root_index = theme_dir.join("index.html");
    match fs::symlink_metadata(&root_index) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(AppError::unprocessable(
                    "theme 'index.html' must not be a symlink",
                ));
            }
            if meta.file_type().is_file() {
                return Ok(Some(theme_dir.to_path_buf()));
            }
            Ok(None)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(AppError::unprocessable(format!(
            "failed to inspect 'index.html': {err}"
        ))),
    }
}

/// Detects optional `ui.css` inside `theme_dir`, rejecting symlinks.
fn detect_ui_css(theme_dir: &Path) -> Result<Option<PathBuf>, AppError> {
    let ui_css = theme_dir.join("ui.css");
    match fs::symlink_metadata(&ui_css) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(AppError::unprocessable(
                    "theme 'ui.css' must not be a symlink",
                ));
            }
            if meta.file_type().is_file() {
                Ok(Some(ui_css))
            } else {
                Err(AppError::unprocessable("theme 'ui.css' must be a regular file"))
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(AppError::unprocessable(format!(
            "failed to inspect 'ui.css': {err}"
        ))),
    }
}

/// Converts a validated relative `Path` into a `/` separated string for diagnostics.
fn rel_path_to_posix(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(os) => os.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Walks each component of `rel` inside `theme_dir` using `symlink_metadata`, ensuring no
/// component is a symlink and the final target is an existing regular file.
fn ensure_regular_file_within(theme_dir: &Path, rel: &Path, field: &str) -> Result<(), AppError> {
    let rel_display = rel_path_to_posix(rel);
    let mut current = theme_dir.to_path_buf();
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

/// Inspects every entry in the zip archive for symlinks, unsafe paths, and macOS junk.
fn inspect_archive_entries(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
) -> Result<Vec<InspectedEntry>, AppError> {
    let mut entries = Vec::with_capacity(archive.len());

    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|err| {
            AppError::bad_request(format!("failed to read zip entry #{index}: {err}"))
        })?;

        let raw_name = file.name().to_string();

        // Reject symlinks and non-regular special Unix file types (FIFOs, devices, sockets).
        if file.is_symlink() {
            return Err(AppError::bad_request(format!(
                "archive entry '{raw_name}' is a symlink, which is not allowed"
            )));
        }
        if let Some(mode) = file.unix_mode() {
            let ftype = mode & UNIX_FILE_TYPE_MASK;
            if ftype == UNIX_SYMLINK_TYPE {
                return Err(AppError::bad_request(format!(
                    "archive entry '{raw_name}' is a Unix symlink, which is not allowed"
                )));
            }
            if ftype != 0 && ftype != UNIX_REGULAR_FILE_TYPE && ftype != UNIX_DIR_TYPE {
                return Err(AppError::bad_request(format!(
                    "archive entry '{raw_name}' has unsupported special file mode 0o{mode:o}"
                )));
            }
        }

        // Strict Zip Slip defense using `enclosed_name()` plus explicit component validation.
        let enclosed = file.enclosed_name().ok_or_else(|| {
            AppError::bad_request(format!(
                "archive entry '{raw_name}' contains an unsafe or escaping path"
            ))
        })?;

        let mut normalized = PathBuf::new();
        for component in enclosed.components() {
            match component {
                Component::CurDir => continue,
                Component::Normal(seg) => {
                    let seg_str = seg.to_str().ok_or_else(|| {
                        AppError::bad_request(format!(
                            "archive entry '{raw_name}' contains non-UTF-8 path segment"
                        ))
                    })?;
                    if seg_str.contains('\0') {
                        return Err(AppError::bad_request(format!(
                            "archive entry '{raw_name}' contains NUL byte"
                        )));
                    }
                    normalized.push(seg);
                }
                _ => {
                    return Err(AppError::bad_request(format!(
                        "archive entry '{raw_name}' contains disallowed path component"
                    )));
                }
            }
        }

        let is_dir = file.is_dir();
        if normalized.as_os_str().is_empty() {
            if is_dir {
                continue;
            }
            return Err(AppError::bad_request(format!(
                "archive entry '{raw_name}' resolves to an empty file path"
            )));
        }

        if is_ignored_macos_junk(&normalized) {
            continue;
        }

        entries.push(InspectedEntry {
            index,
            path: normalized,
            is_dir,
        });
    }

    Ok(entries)
}

/// Returns `true` for macOS packaging artifacts (`__MACOSX/...`, `.DS_Store`, `._*`).
fn is_ignored_macos_junk(path: &Path) -> bool {
    for component in path.components() {
        let Component::Normal(os) = component else {
            continue;
        };
        let Some(name) = os.to_str() else {
            continue;
        };
        if name == "__MACOSX" || name == ".DS_Store" || name.starts_with("._") {
            return true;
        }
    }
    false
}

/// Detects an optional single-top-level-folder wrapper, checks for duplicates/conflicts,
/// and enforces `max_files`.
fn plan_extraction(
    inspected: Vec<InspectedEntry>,
    max_files: usize,
) -> Result<Vec<PlannedEntry>, AppError> {
    let strip_prefix = detect_single_wrapper_dir(&inspected);

    let mut seen_files: HashSet<PathBuf> = HashSet::new();
    let mut seen_dirs: HashSet<PathBuf> = HashSet::new();
    let mut regular_file_count: usize = 0;
    let mut dir_entry_count: usize = 0;
    let max_dir_entries = max_files.saturating_mul(4).max(128);
    let mut planned = Vec::with_capacity(inspected.len());

    for item in inspected {
        let rel_path = if let Some(ref prefix) = strip_prefix {
            match item.path.strip_prefix(prefix) {
                Ok(stripped) if stripped.as_os_str().is_empty() => continue,
                Ok(stripped) => stripped.to_path_buf(),
                Err(_) => item.path,
            }
        } else {
            item.path
        };

        if item.is_dir {
            if seen_files.contains(&rel_path) {
                return Err(AppError::bad_request(format!(
                    "archive path '{}' conflicts between a file and a directory",
                    rel_path.display()
                )));
            }
            check_ancestors_not_files(&rel_path, &seen_files, &mut seen_dirs)?;
            if seen_dirs.insert(rel_path.clone()) {
                dir_entry_count += 1;
                if dir_entry_count > max_dir_entries {
                    return Err(AppError::payload_too_large(format!(
                        "archive exceeds maximum directory count of {max_dir_entries}"
                    )));
                }
                planned.push(PlannedEntry {
                    index: item.index,
                    rel_path,
                    is_dir: true,
                });
            }
        } else {
            if seen_dirs.contains(&rel_path) {
                return Err(AppError::bad_request(format!(
                    "archive path '{}' conflicts between a file and a directory",
                    rel_path.display()
                )));
            }
            if !seen_files.insert(rel_path.clone()) {
                return Err(AppError::bad_request(format!(
                    "duplicate file path in archive: '{}'",
                    rel_path.display()
                )));
            }
            check_ancestors_not_files(&rel_path, &seen_files, &mut seen_dirs)?;

            regular_file_count += 1;
            if regular_file_count > max_files {
                return Err(AppError::payload_too_large(format!(
                    "archive exceeds maximum file count of {max_files}"
                )));
            }

            planned.push(PlannedEntry {
                index: item.index,
                rel_path,
                is_dir: false,
            });
        }
    }

    Ok(planned)
}

/// Ensures no proper ancestor of `path` is already registered as a regular file and records
/// each ancestor in `seen_dirs`.
fn check_ancestors_not_files(
    path: &Path,
    seen_files: &HashSet<PathBuf>,
    seen_dirs: &mut HashSet<PathBuf>,
) -> Result<(), AppError> {
    let mut ancestor = path.parent();
    while let Some(dir) = ancestor {
        if dir.as_os_str().is_empty() {
            break;
        }
        if seen_files.contains(dir) {
            return Err(AppError::bad_request(format!(
                "archive path '{}' conflicts with file '{}'",
                path.display(),
                dir.display()
            )));
        }
        seen_dirs.insert(dir.to_path_buf());
        ancestor = dir.parent();
    }
    Ok(())
}

/// Detects whether all non-ignored entries are wrapped inside one top-level folder `<top>/...`
/// that contains `<top>/theme.json` while no root-level `theme.json` exists.
fn detect_single_wrapper_dir(entries: &[InspectedEntry]) -> Option<PathBuf> {
    let root_manifest = Path::new("theme.json");
    if entries
        .iter()
        .any(|e| !e.is_dir && e.path == root_manifest)
    {
        return None;
    }

    let mut single_top: Option<&std::ffi::OsStr> = None;
    for entry in entries {
        let mut components = entry.path.components();
        let Some(Component::Normal(first)) = components.next() else {
            return None;
        };

        // A root-level regular file prevents single-folder unwrapping.
        if components.next().is_none() && !entry.is_dir {
            return None;
        }

        match single_top {
            None => single_top = Some(first),
            Some(existing) if existing == first => {}
            Some(_) => return None,
        }
    }

    let top = single_top?;
    let wrapped_manifest = PathBuf::from(top).join("theme.json");
    if entries
        .iter()
        .any(|e| !e.is_dir && e.path == wrapped_manifest)
    {
        Some(PathBuf::from(top))
    } else {
        None
    }
}

/// Extracts planned entries into `staging_dir` while strictly counting actual decompressed bytes.
fn extract_planned_entries(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    planned: &[PlannedEntry],
    staging_dir: &Path,
    limits: ArchiveLimits,
) -> Result<(), AppError> {
    let mut total_uncompressed: u64 = 0;
    let mut buf = vec![0u8; COPY_BUF_SIZE];

    for item in planned {
        let dest = staging_dir.join(&item.rel_path);

        if item.is_dir {
            fs::create_dir_all(&dest).map_err(|err| {
                AppError::internal(format!(
                    "failed to create directory '{}' during extraction: {err}",
                    item.rel_path.display()
                ))
            })?;
            continue;
        }

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                AppError::internal(format!(
                    "failed to create parent directory for '{}': {err}",
                    item.rel_path.display()
                ))
            })?;
        }

        let mut out_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    AppError::bad_request(format!(
                        "duplicate or colliding file path in archive: '{}'",
                        item.rel_path.display()
                    ))
                } else {
                    AppError::internal(format!(
                        "failed to create extracted file '{}': {err}",
                        item.rel_path.display()
                    ))
                }
            })?;

        let mut zip_entry = archive.by_index(item.index).map_err(|err| {
            AppError::bad_request(format!(
                "failed to open zip entry '{}': {err}",
                item.rel_path.display()
            ))
        })?;

        loop {
            let read_bytes = zip_entry.read(&mut buf).map_err(|err| {
                AppError::bad_request(format!(
                    "failed to decompress zip entry '{}': {err}",
                    item.rel_path.display()
                ))
            })?;
            if read_bytes == 0 {
                break;
            }

            total_uncompressed = total_uncompressed.saturating_add(read_bytes as u64);
            if total_uncompressed > limits.max_uncompressed_bytes {
                return Err(AppError::payload_too_large(format!(
                    "archive uncompressed content exceeds limit of {} bytes",
                    limits.max_uncompressed_bytes
                )));
            }

            out_file.write_all(&buf[..read_bytes]).map_err(|err| {
                AppError::internal(format!(
                    "failed to write extracted file '{}': {err}",
                    item.rel_path.display()
                ))
            })?;
        }
    }

    Ok(())
}

/// Atomically installs `staged_path` into `target`, backing up and rolling back if `target` exists.
fn install_staged_dir(
    staging_root: &Path,
    staged_path: &Path,
    target: &Path,
    short: &str,
) -> Result<bool, AppError> {
    let existing_meta = match fs::symlink_metadata(target) {
        Ok(meta) => Some(meta),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(AppError::internal(format!(
                "failed to inspect target theme directory '{}': {err}",
                target.display()
            )));
        }
    };

    if let Some(meta) = existing_meta {
        let backup = staging_root.join(format!("backup-{short}-{}", Uuid::new_v4()));
        fs::rename(target, &backup).map_err(|err| {
            AppError::internal(format!(
                "failed to move existing theme '{}' to backup: {err}",
                target.display()
            ))
        })?;

        if let Err(err) = fs::rename(staged_path, target) {
            if let Err(rollback_err) = fs::rename(&backup, target) {
                error!(
                    theme = short,
                    backup = %backup.display(),
                    target = %target.display(),
                    error = %rollback_err,
                    "failed to roll back previous theme directory after rename failure"
                );
            }
            return Err(AppError::internal(format!(
                "failed to move staged theme '{}' into place: {err}",
                target.display()
            )));
        }

        let cleanup_res = if meta.is_dir() {
            fs::remove_dir_all(&backup)
        } else {
            fs::remove_file(&backup)
        };
        if let Err(err) = cleanup_res {
            warn!(
                theme = short,
                backup = %backup.display(),
                error = %err,
                "failed to clean up backup directory after atomic replace"
            );
        }

        Ok(true)
    } else {
        fs::rename(staged_path, target).map_err(|err| {
            AppError::internal(format!(
                "failed to move staged theme '{}' into place: {err}",
                target.display()
            ))
        })?;

        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    enum TestZipEntry<'a> {
        File(&'a str, &'a [u8]),
        Dir(&'a str),
        Symlink(&'a str, &'a str),
    }

    fn build_zip(entries: &[TestZipEntry<'_>]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut cursor);
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            for entry in entries {
                match *entry {
                    TestZipEntry::File(name, data) => {
                        writer.start_file(name, options).expect("start file");
                        writer.write_all(data).expect("write file");
                    }
                    TestZipEntry::Dir(name) => {
                        writer.add_directory(name, options).expect("add dir");
                    }
                    TestZipEntry::Symlink(name, target) => {
                        writer
                            .add_symlink(name, target, options)
                            .expect("add symlink");
                    }
                }
            }
            writer.finish().expect("finish zip");
        }
        cursor.into_inner()
    }

    fn write_file(dir: &Path, rel: &str, content: &[u8]) {
        let full = dir.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(full, content).expect("write test file");
    }

    fn staging_is_empty(themes_dir: &Path) -> bool {
        let staging = themes_dir.join(STAGING_DIR_NAME);
        if !staging.exists() {
            return true;
        }
        fs::read_dir(staging)
            .expect("read staging dir")
            .next()
            .is_none()
    }

    #[test]
    fn validate_theme_short_accepts_valid_and_rejects_default_and_invalid() {
        for valid in ["a", "0", "nord", "nord-minimal", "theme-v2-2026", &"a".repeat(64)] {
            assert!(
                validate_theme_short(valid).is_ok(),
                "expected '{valid}' to be valid"
            );
        }

        let err = validate_theme_short("default").unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");
        assert!(err.to_string().contains("reserved"));

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
        ] {
            let err = validate_theme_short(invalid).unwrap_err();
            assert_eq!(
                err.error_code(),
                "UNPROCESSABLE_ENTITY",
                "expected '{invalid}' to fail"
            );
        }
    }

    #[test]
    fn loads_full_shell_theme_at_root() {
        let temp = TempDir::new().unwrap();
        let theme_dir = temp.path().join("paper-shell");
        write_file(
            &theme_dir,
            "theme.json",
            br#"{
                "short": "paper-shell",
                "name": "  Paper Shell  ",
                "version": "1.0.0",
                "description": "Minimal paper shell",
                "author": "Alice",
                "url": "https://example.com/paper",
                "preview": "assets/preview.png"
            }"#,
        );
        write_file(&theme_dir, "index.html", b"<!doctype html><h1>Paper</h1>");
        write_file(&theme_dir, "ui.css", b":root { --bg: #fff; }");
        write_file(&theme_dir, "assets/preview.png", b"PNGDATA");

        let resolved = load_and_validate_theme(&theme_dir, Some("paper-shell"), true).unwrap();
        assert_eq!(resolved.entry.short, "paper-shell");
        assert_eq!(resolved.entry.name, "Paper Shell");
        assert_eq!(resolved.entry.version, "1.0.0");
        assert_eq!(resolved.entry.description, "Minimal paper shell");
        assert_eq!(resolved.entry.author, "Alice");
        assert_eq!(resolved.entry.url, "https://example.com/paper");
        assert!(!resolved.entry.builtin);
        assert!(resolved.entry.active);
        assert!(resolved.entry.has_custom_shell);
        assert!(resolved.entry.has_ui_css);
        assert_eq!(
            resolved.entry.preview_url.as_deref(),
            Some("/api/admin/themes/paper-shell/preview")
        );
        assert_eq!(resolved.root_dir, theme_dir);
        assert_eq!(resolved.web_root.as_deref(), Some(theme_dir.as_path()));
        assert_eq!(
            resolved.ui_css_path.as_deref(),
            Some(theme_dir.join("ui.css").as_path())
        );

        let serialized = serde_json::to_value(&resolved.entry).unwrap();
        assert_eq!(serialized["hasCustomShell"], true);
        assert_eq!(serialized["hasUiCss"], true);
        assert_eq!(
            serialized["previewUrl"],
            "/api/admin/themes/paper-shell/preview"
        );
    }

    #[test]
    fn loads_dist_shell_theme_preferring_dist_over_root_index() {
        let temp = TempDir::new().unwrap();
        let theme_dir = temp.path().join("vite-theme");
        write_file(
            &theme_dir,
            "theme.json",
            br#"{"short": "vite-theme", "name": "Vite Theme"}"#,
        );
        write_file(&theme_dir, "index.html", b"<h1>Dev root</h1>");
        write_file(&theme_dir, "dist/index.html", b"<h1>Built dist</h1>");

        let resolved = load_and_validate_theme(&theme_dir, Some("vite-theme"), false).unwrap();
        assert_eq!(resolved.entry.version, "0.1.0");
        assert!(resolved.entry.has_custom_shell);
        assert!(!resolved.entry.has_ui_css);
        assert_eq!(resolved.entry.preview_url, None);
        assert_eq!(
            resolved.web_root.as_deref(),
            Some(theme_dir.join("dist").as_path())
        );
        assert_eq!(resolved.ui_css_path, None);
    }

    #[test]
    fn loads_skin_only_ui_css_theme() {
        let temp = TempDir::new().unwrap();
        let theme_dir = temp.path().join("nord-skin");
        write_file(
            &theme_dir,
            "theme.json",
            br#"{"short": "nord-skin", "name": "Nord Skin"}"#,
        );
        write_file(&theme_dir, "ui.css", b":root { --accent: #88c0d0; }");

        let resolved = load_and_validate_theme(&theme_dir, Some("nord-skin"), false).unwrap();
        assert!(!resolved.entry.has_custom_shell);
        assert!(resolved.entry.has_ui_css);
        assert_eq!(resolved.web_root, None);
        assert_eq!(
            resolved.ui_css_path.as_deref(),
            Some(theme_dir.join("ui.css").as_path())
        );
    }

    #[test]
    fn rejects_empty_theme_with_neither_index_nor_ui_css_and_invalid_fields() {
        let temp = TempDir::new().unwrap();

        // Neither index.html nor ui.css
        let empty = temp.path().join("empty-theme");
        write_file(
            &empty,
            "theme.json",
            br#"{"short": "empty-theme", "name": "Empty"}"#,
        );
        let err = load_and_validate_theme(&empty, None, false).unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");

        // Reserved short: "default"
        let reserved = temp.path().join("default");
        write_file(
            &reserved,
            "theme.json",
            br#"{"short": "default", "name": "Fake Default"}"#,
        );
        write_file(&reserved, "ui.css", b"body {}");
        let err = load_and_validate_theme(&reserved, None, false).unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");

        // Unsafe URL scheme
        let bad_url = temp.path().join("bad-url");
        write_file(
            &bad_url,
            "theme.json",
            br#"{"short": "bad-url", "name": "Bad URL", "url": "javascript:alert(1)"}"#,
        );
        write_file(&bad_url, "ui.css", b"body {}");
        let err = load_and_validate_theme(&bad_url, None, false).unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");

        // Mismatched expected_short
        let mismatch = temp.path().join("real-dir");
        write_file(
            &mismatch,
            "theme.json",
            br#"{"short": "other-short", "name": "Mismatch"}"#,
        );
        write_file(&mismatch, "ui.css", b"body {}");
        let err = load_and_validate_theme(&mismatch, Some("real-dir"), false).unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");
    }

    #[test]
    fn list_themes_and_resolve_active_theme_handle_sorting_fallback_and_broken_dirs() {
        let temp = TempDir::new().unwrap();
        let themes_dir = temp.path().join("themes");

        // Missing themes_dir still returns built-in default marked active
        let list_missing = list_themes(&themes_dir, "default");
        assert_eq!(list_missing.len(), 1);
        assert_eq!(list_missing[0].short, "default");
        assert!(list_missing[0].builtin);
        assert!(list_missing[0].active);
        assert!(resolve_active_theme(&themes_dir, "default").is_none());

        // Populate multiple themes, a hidden .staging dir, and a broken theme dir
        write_file(
            &themes_dir.join(".staging"),
            "theme.json",
            br#"{"short": "hidden", "name": "Hidden"}"#,
        );
        write_file(&themes_dir.join("broken"), "theme.json", b"{not-json");

        write_file(
            &themes_dir.join("zeta"),
            "theme.json",
            br#"{"short": "zeta", "name": "Zeta Theme"}"#,
        );
        write_file(&themes_dir.join("zeta"), "ui.css", b"body {}");

        write_file(
            &themes_dir.join("alpha"),
            "theme.json",
            br#"{"short": "alpha", "name": "alpha theme"}"#,
        );
        write_file(&themes_dir.join("alpha"), "index.html", b"<h1>Alpha</h1>");

        // Active theme found -> default is inactive, alpha is active
        let listed = list_themes(&themes_dir, "alpha");
        let shorts: Vec<&str> = listed.iter().map(|t| t.short.as_str()).collect();
        assert_eq!(shorts, vec!["default", "alpha", "zeta"]);
        assert!(!listed[0].active);
        assert!(listed[1].active);
        assert!(!listed[2].active);

        let active = resolve_active_theme(&themes_dir, "alpha").expect("resolve alpha");
        assert_eq!(active.entry.short, "alpha");
        assert!(active.entry.active);

        // Missing active_short -> default falls back to active
        let fallback_list = list_themes(&themes_dir, "nonexistent");
        assert!(fallback_list[0].active);
        assert!(!fallback_list[1].active);
        assert!(!fallback_list[2].active);
        assert!(resolve_active_theme(&themes_dir, "nonexistent").is_none());
        assert!(resolve_active_theme(&themes_dir, "broken").is_none());
    }

    #[test]
    fn install_theme_zip_handles_wrapper_conflict_overwrite_and_delete() {
        let temp = TempDir::new().unwrap();
        let themes_dir = temp.path().join("themes");

        let zip_v1 = build_zip(&[
            TestZipEntry::Dir("my-theme-pkg/"),
            TestZipEntry::File(
                "my-theme-pkg/theme.json",
                br#"{"short": "ocean", "name": "Ocean", "version": "1.0.0"}"#,
            ),
            TestZipEntry::File("my-theme-pkg/ui.css", b":root { --bg: #001; }"),
            TestZipEntry::File("__MACOSX/._theme.json", b"junk"),
            TestZipEntry::File("my-theme-pkg/.DS_Store", b"junk"),
        ]);

        let outcome =
            install_theme_zip_checked(&themes_dir, &zip_v1, ArchiveLimits::default(), false)
                .expect("initial install");
        match outcome {
            ThemeInstallOutcome::Installed { entry, updated } => {
                assert!(!updated);
                assert_eq!(entry.short, "ocean");
                assert_eq!(entry.version, "1.0.0");
                assert!(entry.has_ui_css);
                assert!(!entry.has_custom_shell);
            }
            other => panic!("expected Installed, got {other:?}"),
        }
        assert!(staging_is_empty(&themes_dir));

        // Second upload with allow_overwrite = false returns AlreadyExists without modifying disk
        let zip_v2 = build_zip(&[
            TestZipEntry::File(
                "theme.json",
                br#"{"short": "ocean", "name": "Ocean v2", "version": "2.0.0"}"#,
            ),
            TestZipEntry::File("dist/index.html", b"<h1>Ocean v2</h1>"),
        ]);

        let conflict =
            install_theme_zip_checked(&themes_dir, &zip_v2, ArchiveLimits::default(), false)
                .expect("conflict outcome");
        match conflict {
            ThemeInstallOutcome::AlreadyExists { existing, incoming } => {
                assert_eq!(existing.as_deref().map(|e| e.version.as_str()), Some("1.0.0"));
                assert_eq!(incoming.version, "2.0.0");
            }
            other => panic!("expected AlreadyExists, got {other:?}"),
        }
        assert!(staging_is_empty(&themes_dir));
        assert_eq!(
            resolve_active_theme(&themes_dir, "ocean")
                .unwrap()
                .entry
                .version,
            "1.0.0"
        );

        // Confirmed overwrite replaces atomically
        let replaced =
            install_theme_zip_checked(&themes_dir, &zip_v2, ArchiveLimits::default(), true)
                .expect("overwrite install");
        match replaced {
            ThemeInstallOutcome::Installed { entry, updated } => {
                assert!(updated);
                assert_eq!(entry.version, "2.0.0");
                assert!(entry.has_custom_shell);
                assert!(!entry.has_ui_css);
            }
            other => panic!("expected Installed, got {other:?}"),
        }
        assert!(staging_is_empty(&themes_dir));
        assert_eq!(
            resolve_active_theme(&themes_dir, "ocean")
                .unwrap()
                .entry
                .version,
            "2.0.0"
        );

        // delete_theme_dir rejects "default" with 400 Bad Request
        let err = delete_theme_dir(&themes_dir, "default").unwrap_err();
        assert_eq!(err.error_code(), "BAD_REQUEST");

        // delete_theme_dir removes installed theme and returns false when absent
        assert!(delete_theme_dir(&themes_dir, "ocean").unwrap());
        assert!(!delete_theme_dir(&themes_dir, "ocean").unwrap());
        assert!(staging_is_empty(&themes_dir));
    }

    #[test]
    fn install_theme_zip_rejects_zip_slip_symlinks_default_short_and_empty_theme() {
        let temp = TempDir::new().unwrap();
        let themes_dir = temp.path().join("themes");

        // Zip Slip traversal
        let zip_slip = build_zip(&[
            TestZipEntry::File("../evil.txt", b"boom"),
            TestZipEntry::File(
                "theme.json",
                br#"{"short": "slip", "name": "Slip"}"#,
            ),
            TestZipEntry::File("ui.css", b"body {}"),
        ]);
        let err =
            install_theme_zip_checked(&themes_dir, &zip_slip, ArchiveLimits::default(), true)
                .unwrap_err();
        assert_eq!(err.error_code(), "BAD_REQUEST");

        // Symlink inside archive
        let zip_sym = build_zip(&[
            TestZipEntry::File("theme.json", br#"{"short": "sym", "name": "Sym"}"#),
            TestZipEntry::Symlink("ui.css", "/etc/passwd"),
        ]);
        let err =
            install_theme_zip_checked(&themes_dir, &zip_sym, ArchiveLimits::default(), true)
                .unwrap_err();
        assert_eq!(err.error_code(), "BAD_REQUEST");

        // Reserved "default" short inside zip
        let zip_default = build_zip(&[
            TestZipEntry::File(
                "theme.json",
                br#"{"short": "default", "name": "Default"}"#,
            ),
            TestZipEntry::File("ui.css", b"body {}"),
        ]);
        let err =
            install_theme_zip_checked(&themes_dir, &zip_default, ArchiveLimits::default(), true)
                .unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");

        // Theme with neither index.html nor ui.css
        let zip_empty = build_zip(&[TestZipEntry::File(
            "theme.json",
            br#"{"short": "empty", "name": "Empty"}"#,
        )]);
        let err =
            install_theme_zip_checked(&themes_dir, &zip_empty, ArchiveLimits::default(), true)
                .unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");

        assert!(staging_is_empty(&themes_dir));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_theme_preview_returns_canonical_path_and_blocks_symlink_escape() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let themes_dir = temp.path().join("themes");
        let outside = temp.path().join("secret.png");
        fs::write(&outside, b"SECRET").unwrap();

        // Valid preview
        let ok_dir = themes_dir.join("ok-theme");
        write_file(
            &ok_dir,
            "theme.json",
            br#"{"short": "ok-theme", "name": "OK", "preview": "img/preview.png"}"#,
        );
        write_file(&ok_dir, "ui.css", b"body {}");
        write_file(&ok_dir, "img/preview.png", b"PNG");

        let resolved = resolve_theme_preview(&themes_dir, "ok-theme").expect("valid preview");
        assert_eq!(
            resolved,
            fs::canonicalize(ok_dir.join("img/preview.png")).unwrap()
        );

        // Missing theme -> 404
        let err = resolve_theme_preview(&themes_dir, "missing").unwrap_err();
        assert_eq!(err.error_code(), "NOT_FOUND");

        // Missing preview field -> 404
        let no_prev = themes_dir.join("no-prev");
        write_file(
            &no_prev,
            "theme.json",
            br#"{"short": "no-prev", "name": "No Prev"}"#,
        );
        write_file(&no_prev, "ui.css", b"body {}");
        let err = resolve_theme_preview(&themes_dir, "no-prev").unwrap_err();
        assert_eq!(err.error_code(), "NOT_FOUND");

        // Symlink escape on preview -> 403 Forbidden
        let sym_dir = themes_dir.join("sym-prev");
        write_file(
            &sym_dir,
            "theme.json",
            br#"{"short": "sym-prev", "name": "Sym Prev", "preview": "preview.png"}"#,
        );
        write_file(&sym_dir, "ui.css", b"body {}");
        symlink(&outside, sym_dir.join("preview.png")).unwrap();

        let err = resolve_theme_preview(&themes_dir, "sym-prev").unwrap_err();
        assert_eq!(err.error_code(), "FORBIDDEN");
    }
}
