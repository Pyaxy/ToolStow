//! Safe `.zip` archive unpacking, quota enforcement, and atomic directory installation.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use tracing::{error, warn};
use uuid::Uuid;
use zip::ZipArchive;

use crate::error::AppError;
use crate::manifest::{self, RawManifest, ToolEntry, Visibility};

/// Default maximum compressed `.zip` size (50 MiB).
pub const DEFAULT_MAX_ZIP_BYTES: usize = 50 * 1024 * 1024;
/// Default maximum total uncompressed bytes across all entries in a `.zip` (200 MiB).
pub const DEFAULT_MAX_UNCOMPRESSED_BYTES: u64 = 200 * 1024 * 1024;
/// Default maximum number of file entries permitted inside a single `.zip`.
pub const DEFAULT_MAX_FILES: usize = 1_000;

/// Hidden subdirectory under `tools_dir` used for same-filesystem staging, backup, and trash.
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

/// Resource limits enforced while inspecting and extracting uploaded `.zip` archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// Maximum allowed compressed archive size in bytes.
    pub max_zip_bytes: usize,
    /// Maximum allowed total uncompressed bytes across all extracted entries.
    pub max_uncompressed_bytes: u64,
    /// Maximum allowed number of entries inside the archive.
    pub max_files: usize,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_zip_bytes: DEFAULT_MAX_ZIP_BYTES,
            max_uncompressed_bytes: DEFAULT_MAX_UNCOMPRESSED_BYTES,
            max_files: DEFAULT_MAX_FILES,
        }
    }
}

/// Outcome of successfully installing or atomically replacing a tool from a `.zip` archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallResult {
    /// Validated metadata of the installed tool.
    pub entry: ToolEntry,
    /// `true` if an existing tool with the same `id` was atomically replaced; `false` if newly created.
    pub updated: bool,
}

/// Outcome of [`install_zip_bytes_checked`] or [`install_single_html_checked`], distinguishing a
/// completed install from an existing-ID conflict when overwrite is not permitted.
#[derive(Debug, Clone)]
pub enum InstallOutcome {
    /// Package passed validation and was installed or atomically replaced.
    Installed(InstallResult),
    /// A tool with the same `id` already exists and `allow_overwrite` was `false`.
    AlreadyExists {
        /// Currently installed tool metadata (if its existing `manifest.json` is valid).
        existing: Option<Box<ToolEntry>>,
        /// Validated metadata of the newly submitted tool.
        incoming: ToolEntry,
    },
}

/// Input payload for ingesting a standalone single-file HTML tool without a `.zip` archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleHtmlToolInput {
    /// Unique tool identifier (`^[a-z0-9]+(?:-[a-z0-9]+)*$`, 1..=64).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Raw HTML document content for `index.html`.
    pub html: String,
    /// Optional one-line description.
    pub description: Option<String>,
    /// Optional category name.
    pub category: Option<String>,
    /// Optional list of search/filter tags.
    pub tags: Option<Vec<String>>,
    /// Optional Emoji icon.
    pub icon: Option<String>,
    /// Optional explicit visibility state (`None` inherits an existing tool's visibility on
    /// overwrite, or defaults to [`Visibility::Public`] for new tools).
    pub visibility: Option<Visibility>,
    /// When `true`, injects `/ui/v1/toolstow.css` and `/ui/v1/theme.js` into the HTML document.
    pub inject_sdk: bool,
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

/// Validates, extracts, and atomically installs a `.zip` tool package into `tools_dir/{id}`,
/// replacing any existing tool with the same `id`.
pub fn install_zip_bytes(
    tools_dir: &Path,
    zip_bytes: &[u8],
    limits: ArchiveLimits,
) -> Result<InstallResult, AppError> {
    match install_zip_bytes_checked(tools_dir, zip_bytes, limits, true)? {
        InstallOutcome::Installed(result) => Ok(result),
        InstallOutcome::AlreadyExists { incoming, .. } => Err(AppError::conflict(format!(
            "tool '{}' already exists",
            incoming.id
        ))),
    }
}

/// Validates, extracts, and installs a `.zip` tool package into `tools_dir/{id}`.
///
/// When `allow_overwrite` is `false` and `tools_dir/{id}` already exists, the staging directory is
/// cleaned up automatically and [`InstallOutcome::AlreadyExists`] is returned without modifying the
/// existing tool.
///
/// If an existing tool with the same `id` is already installed and the incoming archive's
/// `manifest.json` omits both `visibility` and `draft`, the existing tool's visibility is
/// inherited onto the staged `manifest.json` (both in `AlreadyExists.incoming` and on install).
pub fn install_zip_bytes_checked(
    tools_dir: &Path,
    zip_bytes: &[u8],
    limits: ArchiveLimits,
    allow_overwrite: bool,
) -> Result<InstallOutcome, AppError> {
    install_zip_bytes_checked_with_commit(
        tools_dir,
        zip_bytes,
        limits,
        allow_overwrite,
        |_entry, install_fn| install_fn(),
    )
}

/// Validates, extracts, and installs a `.zip` tool package into `tools_dir/{id}`, invoking
/// `on_commit` around the final atomic directory swap so callers can hold an exclusive lock
/// across both the disk rename and the in-memory registry update without blocking readers during
/// archive extraction and validation.
pub fn install_zip_bytes_checked_with_commit<F>(
    tools_dir: &Path,
    zip_bytes: &[u8],
    limits: ArchiveLimits,
    allow_overwrite: bool,
    on_commit: F,
) -> Result<InstallOutcome, AppError>
where
    F: FnOnce(&ToolEntry, &mut dyn FnMut() -> Result<bool, AppError>) -> Result<bool, AppError>,
{
    if zip_bytes.len() > limits.max_zip_bytes {
        return Err(AppError::payload_too_large(format!(
            "uploaded zip archive is {} bytes, exceeding limit of {} bytes",
            zip_bytes.len(),
            limits.max_zip_bytes
        )));
    }

    let staging_root = tools_dir.join(STAGING_DIR_NAME);
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

    // Validate the extracted tool directory before touching the live tools directory.
    let (mut entry, has_explicit_visibility) =
        manifest::load_and_validate_manifest_with_source(stage_dir.path(), None)?;
    let target = tools_dir.join(&entry.id);
    let existing = if target.exists() {
        manifest::load_and_validate_manifest(&target, Some(&entry.id)).ok()
    } else {
        None
    };

    if !has_explicit_visibility
        && let Some(ref existing_entry) = existing
    {
        write_manifest_visibility_in_dir(stage_dir.path(), existing_entry.visibility)?;
        entry = manifest::load_and_validate_manifest(stage_dir.path(), None)?;
    }

    if !allow_overwrite && target.exists() {
        // `stage_dir` drops here, cleaning up the temporary directory without touching `target`.
        return Ok(InstallOutcome::AlreadyExists {
            existing: existing.map(Box::new),
            incoming: entry,
        });
    }

    // Atomically move the validated staging directory into `tools_dir/{id}`.
    let staged_path = stage_dir.keep();
    let mut cleanup = CleanupPath::new(staged_path.clone());
    let mut backup_to_remove: Option<PathBuf> = None;

    let updated = on_commit(&entry, &mut || {
        let (updated, backup) =
            install_staged_dir_swap(&staging_root, &staged_path, &target, &entry.id)?;
        backup_to_remove = backup;
        Ok(updated)
    })?;
    cleanup.disarm();

    if let Some(backup) = backup_to_remove
        && let Err(err) = fs::remove_dir_all(&backup)
    {
        warn!(
            tool_id = %entry.id,
            backup = %backup.display(),
            error = %err,
            "failed to clean up backup directory after atomic replace"
        );
    }

    Ok(InstallOutcome::Installed(InstallResult { entry, updated }))
}

/// Validates, optionally injects the ToolStow UI SDK, and atomically installs a standalone
/// single-file HTML tool into `tools_dir/{id}`.
///
/// When `allow_overwrite` is `false` and `tools_dir/{id}` already exists, the staging directory is
/// cleaned up automatically and [`InstallOutcome::AlreadyExists`] is returned without modifying the
/// existing tool.
///
/// When `input.visibility` is `None` and an existing valid tool is already installed at
/// `tools_dir/{id}`, the existing tool's visibility is inherited; otherwise it defaults to
/// [`Visibility::Public`].
pub fn install_single_html_checked(
    tools_dir: &Path,
    input: SingleHtmlToolInput,
    limits: ArchiveLimits,
    allow_overwrite: bool,
) -> Result<InstallOutcome, AppError> {
    install_single_html_checked_with_commit(
        tools_dir,
        input,
        limits,
        allow_overwrite,
        |_entry, install_fn| install_fn(),
    )
}

/// Validates, optionally injects the ToolStow UI SDK, and atomically installs a standalone
/// single-file HTML tool into `tools_dir/{id}`, invoking `on_commit` around the final atomic
/// directory swap so callers can serialize disk swap + registry update under an exclusive lock.
pub fn install_single_html_checked_with_commit<F>(
    tools_dir: &Path,
    input: SingleHtmlToolInput,
    limits: ArchiveLimits,
    allow_overwrite: bool,
    on_commit: F,
) -> Result<InstallOutcome, AppError>
where
    F: FnOnce(&ToolEntry, &mut dyn FnMut() -> Result<bool, AppError>) -> Result<bool, AppError>,
{
    manifest::validate_id(&input.id)?;

    let trimmed_name = input.name.trim();
    if trimmed_name.is_empty() {
        return Err(AppError::unprocessable("tool 'name' must not be empty"));
    }

    if input.html.len() > limits.max_zip_bytes {
        return Err(AppError::payload_too_large(format!(
            "uploaded HTML document is {} bytes, exceeding limit of {} bytes",
            input.html.len(),
            limits.max_zip_bytes
        )));
    }

    if input.html.trim().is_empty() {
        return Err(AppError::unprocessable("HTML content must not be empty"));
    }

    let final_html = if input.inject_sdk {
        inject_toolstow_sdk(&input.html)
    } else {
        input.html
    };

    let theme_aware = scan_html(&final_html).has_theme_js;

    let staging_root = tools_dir.join(STAGING_DIR_NAME);
    fs::create_dir_all(&staging_root).map_err(|err| {
        AppError::internal(format!(
            "failed to create staging directory '{}': {err}",
            staging_root.display()
        ))
    })?;

    let stage_dir = tempfile::Builder::new()
        .prefix("stage-html-")
        .tempdir_in(&staging_root)
        .map_err(|err| {
            AppError::internal(format!(
                "failed to create temporary staging directory in '{}': {err}",
                staging_root.display()
            ))
        })?;

    fs::write(stage_dir.path().join("index.html"), final_html.as_bytes()).map_err(|err| {
        AppError::internal(format!("failed to write staged index.html: {err}"))
    })?;

    let target = tools_dir.join(&input.id);
    let existing = if target.exists() {
        manifest::load_and_validate_manifest(&target, Some(&input.id)).ok()
    } else {
        None
    };

    let effective_visibility = input.visibility.unwrap_or_else(|| {
        existing
            .as_ref()
            .map(|e| e.visibility)
            .unwrap_or(Visibility::Public)
    });

    let raw_manifest = RawManifest {
        id: input.id,
        name: trimmed_name.to_string(),
        version: None,
        description: input.description,
        category: input.category,
        tags: input.tags,
        icon: input.icon,
        entry: Some(manifest::DEFAULT_ENTRY.to_string()),
        runtime: Some(manifest::DEFAULT_RUNTIME.to_string()),
        theme_aware: Some(theme_aware),
        sandbox: None,
        visibility: Some(effective_visibility.as_str().to_string()),
        draft: (effective_visibility == Visibility::Draft).then_some(true),
    };

    let mut manifest_bytes = serde_json::to_vec_pretty(&raw_manifest)
        .map_err(|err| AppError::internal(format!("failed to serialize manifest.json: {err}")))?;
    manifest_bytes.push(b'\n');

    fs::write(stage_dir.path().join("manifest.json"), &manifest_bytes).map_err(|err| {
        AppError::internal(format!("failed to write staged manifest.json: {err}"))
    })?;

    let entry = manifest::load_and_validate_manifest(stage_dir.path(), None)?;

    if !allow_overwrite && target.exists() {
        return Ok(InstallOutcome::AlreadyExists {
            existing: existing.map(Box::new),
            incoming: entry,
        });
    }

    let staged_path = stage_dir.keep();
    let mut cleanup = CleanupPath::new(staged_path.clone());
    let mut backup_to_remove: Option<PathBuf> = None;

    let updated = on_commit(&entry, &mut || {
        let (updated, backup) =
            install_staged_dir_swap(&staging_root, &staged_path, &target, &entry.id)?;
        backup_to_remove = backup;
        Ok(updated)
    })?;
    cleanup.disarm();

    if let Some(backup) = backup_to_remove
        && let Err(err) = fs::remove_dir_all(&backup)
    {
        warn!(
            tool_id = %entry.id,
            backup = %backup.display(),
            error = %err,
            "failed to clean up backup directory after atomic replace"
        );
    }

    Ok(InstallOutcome::Installed(InstallResult { entry, updated }))
}

const TOOLSTOW_CSS_PATH: &str = "/ui/v1/toolstow.css";
const TOOLSTOW_THEME_JS_PATH: &str = "/ui/v1/theme.js";

/// Result of a single-pass, byte-offset-preserving HTML structural scan.
#[derive(Debug, Default)]
struct HtmlScanResult {
    /// True iff a real `<link>` tag loads `/ui/v1/toolstow.css` with `rel` containing `stylesheet`.
    has_toolstow_css: bool,
    /// True iff a real `<script>` tag loads `/ui/v1/theme.js` via `src`.
    has_theme_js: bool,
    /// Start byte offset of the first real `</head>` closing tag.
    head_close_start: Option<usize>,
    /// Exclusive end byte offset of the first real `<head ...>` opening tag.
    head_open_end: Option<usize>,
    /// Start byte offset of the first real `<body ...>` opening tag.
    body_open_start: Option<usize>,
    /// Exclusive end byte offset of a leading `<!doctype ...>` declaration.
    leading_doctype_end: Option<usize>,
}

/// Safely injects missing ToolStow UI stylesheet and theme bridge tags into an HTML document.
fn inject_toolstow_sdk(html: &str) -> String {
    const CSS_TAG: &str = r#"<link rel="stylesheet" href="/ui/v1/toolstow.css">"#;
    const THEME_JS_TAG: &str = r#"<script src="/ui/v1/theme.js"></script>"#;

    let scan = scan_html(html);

    let snippet = match (scan.has_toolstow_css, scan.has_theme_js) {
        (true, true) => return html.to_string(),
        (false, false) => format!("{CSS_TAG}\n{THEME_JS_TAG}"),
        (false, true) => CSS_TAG.to_string(),
        (true, false) => THEME_JS_TAG.to_string(),
    };

    // 1. Insert right before the first real `</head>` if present.
    if let Some(pos) = scan.head_close_start {
        let mut out = String::with_capacity(html.len() + snippet.len() + 2);
        out.push_str(&html[..pos]);
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&snippet);
        out.push('\n');
        out.push_str(&html[pos..]);
        return out;
    }

    // 2. Otherwise insert right after the first real `<head...>` if present.
    if let Some(end) = scan.head_open_end {
        let mut out = String::with_capacity(html.len() + snippet.len() + 2);
        out.push_str(&html[..end]);
        out.push('\n');
        out.push_str(&snippet);
        if !html[end..].starts_with('\n') {
            out.push('\n');
        }
        out.push_str(&html[end..]);
        return out;
    }

    // 3. Otherwise insert right before the first real `<body...>` if present.
    if let Some(start) = scan.body_open_start {
        let mut out = String::with_capacity(html.len() + snippet.len() + 2);
        out.push_str(&html[..start]);
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&snippet);
        out.push('\n');
        out.push_str(&html[start..]);
        return out;
    }

    // 4. Otherwise insert right after a leading `<!doctype ...>` (preserving standards mode),
    //    or prepend directly to the HTML fragment at byte 0.
    if let Some(doctype_end) = scan.leading_doctype_end {
        let mut out = String::with_capacity(html.len() + snippet.len() + 2);
        out.push_str(&html[..doctype_end]);
        out.push('\n');
        out.push_str(&snippet);
        if !html[doctype_end..].starts_with('\n') {
            out.push('\n');
        }
        out.push_str(&html[doctype_end..]);
        return out;
    }

    format!("{snippet}\n{html}")
}

/// Performs a single-pass, comment-, raw-text/RCDATA-, and quoted-attribute-aware HTML scan,
/// returning exact UTF-8 byte offsets and real SDK tag detection.
fn scan_html(html: &str) -> HtmlScanResult {
    let bytes = html.as_bytes();
    let mut result = HtmlScanResult::default();
    let mut i = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };
    let mut seen_element_or_text = false;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            if !bytes[i].is_ascii_whitespace() {
                seen_element_or_text = true;
            }
            i += 1;
            continue;
        }

        // 1. HTML comments: `<!-- ... -->`, including `<!---->`, `<!-->`, `<!--->`, and unclosed to EOF.
        if bytes[i..].starts_with(b"<!--") {
            i = skip_html_comment(bytes, i);
            continue;
        }

        // 2. Leading or inline `<!doctype ...>` declaration.
        if bytes.len().saturating_sub(i) >= 9
            && bytes[i..i + 9].eq_ignore_ascii_case(b"<!doctype")
            && (i + 9 == bytes.len()
                || bytes[i + 9].is_ascii_whitespace()
                || bytes[i + 9] == b'>')
        {
            if let Some(end) = scan_doctype_end(bytes, i + 9) {
                if !seen_element_or_text && result.leading_doctype_end.is_none() {
                    result.leading_doctype_end = Some(end);
                }
                i = end;
            } else {
                i = bytes.len();
            }
            continue;
        }

        // 3. CDATA section `<![CDATA[ ... ]]>`.
        if bytes[i..].starts_with(b"<![CDATA[") {
            seen_element_or_text = true;
            i = skip_cdata(bytes, i);
            continue;
        }

        // 4. Other markup declarations or processing instructions (`<!...>`, `<?...>`).
        if i + 1 < bytes.len() && (bytes[i + 1] == b'!' || bytes[i + 1] == b'?') {
            i = skip_to_gt(bytes, i + 2);
            continue;
        }

        // 5. Closing tags `</name ...>`.
        if i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            if i + 2 < bytes.len() && bytes[i + 2].is_ascii_alphabetic() {
                let name_start = i + 2;
                let mut name_end = name_start;
                while name_end < bytes.len()
                    && !bytes[name_end].is_ascii_whitespace()
                    && bytes[name_end] != b'>'
                    && bytes[name_end] != b'/'
                {
                    name_end += 1;
                }
                let tag_name = &html[name_start..name_end];
                if let Some(tag_end) = scan_tag_rest(html, name_end, |_, _| {}) {
                    seen_element_or_text = true;
                    if tag_name.eq_ignore_ascii_case("head") && result.head_close_start.is_none() {
                        result.head_close_start = Some(i);
                    }
                    i = tag_end;
                } else {
                    i = bytes.len();
                }
                continue;
            }
            seen_element_or_text = true;
            i += 1;
            continue;
        }

        // 6. Opening tags `<name ...>`.
        if i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            let tag_start = i;
            let name_start = i + 1;
            let mut name_end = name_start;
            while name_end < bytes.len()
                && !bytes[name_end].is_ascii_whitespace()
                && bytes[name_end] != b'>'
                && bytes[name_end] != b'/'
            {
                name_end += 1;
            }
            let tag_name = &html[name_start..name_end];

            let is_link = tag_name.eq_ignore_ascii_case("link");
            let is_script = tag_name.eq_ignore_ascii_case("script");
            let mut link_rel: Option<&str> = None;
            let mut link_href: Option<&str> = None;
            let mut script_src: Option<&str> = None;

            let Some(tag_end) = scan_tag_rest(html, name_end, |attr_name, attr_val| {
                if is_link {
                    if link_rel.is_none() && attr_name.eq_ignore_ascii_case("rel") {
                        link_rel = attr_val;
                    } else if link_href.is_none() && attr_name.eq_ignore_ascii_case("href") {
                        link_href = attr_val;
                    }
                } else if is_script
                    && script_src.is_none()
                    && attr_name.eq_ignore_ascii_case("src")
                {
                    script_src = attr_val;
                }
            }) else {
                break;
            };

            seen_element_or_text = true;

            if tag_name.eq_ignore_ascii_case("head") && result.head_open_end.is_none() {
                result.head_open_end = Some(tag_end);
            } else if tag_name.eq_ignore_ascii_case("body") && result.body_open_start.is_none() {
                result.body_open_start = Some(tag_start);
            } else if is_link
                && let (Some(rel), Some(href)) = (link_rel, link_href)
                && rel_has_stylesheet(rel)
                && url_path_equals(href, TOOLSTOW_CSS_PATH)
            {
                result.has_toolstow_css = true;
            } else if is_script
                && let Some(src) = script_src
                && url_path_equals(src, TOOLSTOW_THEME_JS_PATH)
            {
                result.has_theme_js = true;
            }

            if is_raw_text_or_rcdata_tag(tag_name) {
                i = find_raw_text_end(bytes, tag_end, tag_name);
            } else {
                i = tag_end;
            }
            continue;
        }

        // Literal `<` in text content.
        seen_element_or_text = true;
        i += 1;
    }

    result
}

/// Skips an HTML comment starting at `start` (`<!--`), handling `<!---->`, `<!-->`, `<!--->`,
/// and unclosed comments to EOF.
fn skip_html_comment(bytes: &[u8], start: usize) -> usize {
    let mut pos = start + 4;
    if pos >= bytes.len() {
        return bytes.len();
    }
    if bytes[pos] == b'>' {
        return pos + 1;
    }
    if bytes[pos..].starts_with(b"->") {
        return pos + 2;
    }
    while pos < bytes.len() {
        if bytes[pos..].starts_with(b"-->") {
            return pos + 3;
        }
        if bytes[pos..].starts_with(b"--!>") {
            return pos + 4;
        }
        pos += 1;
    }
    bytes.len()
}

/// Scans past a `<!doctype ...>` declaration respecting quoted strings, returning the byte
/// offset immediately after `>`, or `None` if unclosed.
fn scan_doctype_end(bytes: &[u8], mut pos: usize) -> Option<usize> {
    while pos < bytes.len() {
        match bytes[pos] {
            b'>' => return Some(pos + 1),
            b'"' | b'\'' => {
                let quote = bytes[pos];
                pos += 1;
                while pos < bytes.len() && bytes[pos] != quote {
                    pos += 1;
                }
                if pos < bytes.len() {
                    pos += 1;
                }
            }
            _ => pos += 1,
        }
    }
    None
}

/// Skips a `<![CDATA[ ... ]]>` section to EOF if unclosed.
fn skip_cdata(bytes: &[u8], start: usize) -> usize {
    let mut pos = start + 9;
    while pos < bytes.len() {
        if bytes[pos..].starts_with(b"]]>") {
            return pos + 3;
        }
        pos += 1;
    }
    bytes.len()
}

/// Skips forward until immediately after the next `>`, or EOF.
fn skip_to_gt(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() {
        if bytes[pos] == b'>' {
            return pos + 1;
        }
        pos += 1;
    }
    bytes.len()
}

/// Scans attributes and finds the closing `>` of a tag starting after its tag name, respecting
/// single- and double-quoted attribute values.
fn scan_tag_rest<'a>(
    html: &'a str,
    mut pos: usize,
    mut on_attr: impl FnMut(&'a str, Option<&'a str>),
) -> Option<usize> {
    let bytes = html.as_bytes();
    while pos < bytes.len() {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            return None;
        }
        match bytes[pos] {
            b'>' => return Some(pos + 1),
            b'/' => {
                pos += 1;
            }
            _ => {
                let name_start = pos;
                while pos < bytes.len()
                    && !bytes[pos].is_ascii_whitespace()
                    && bytes[pos] != b'='
                    && bytes[pos] != b'>'
                    && bytes[pos] != b'/'
                {
                    pos += 1;
                }
                let attr_name = &html[name_start..pos];

                while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                }

                let attr_value = if pos < bytes.len() && bytes[pos] == b'=' {
                    pos += 1;
                    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                        pos += 1;
                    }
                    if pos < bytes.len() && (bytes[pos] == b'"' || bytes[pos] == b'\'') {
                        let quote = bytes[pos];
                        pos += 1;
                        let val_start = pos;
                        while pos < bytes.len() && bytes[pos] != quote {
                            pos += 1;
                        }
                        let val = &html[val_start..pos];
                        if pos < bytes.len() {
                            pos += 1;
                        }
                        Some(val)
                    } else {
                        let val_start = pos;
                        while pos < bytes.len()
                            && !bytes[pos].is_ascii_whitespace()
                            && bytes[pos] != b'>'
                        {
                            pos += 1;
                        }
                        Some(&html[val_start..pos])
                    }
                } else {
                    None
                };

                if !attr_name.is_empty() {
                    on_attr(attr_name, attr_value);
                }
            }
        }
    }
    None
}

/// Returns `true` for HTML raw-text / RCDATA elements whose bodies must not be parsed as markup.
fn is_raw_text_or_rcdata_tag(tag: &str) -> bool {
    tag.eq_ignore_ascii_case("script")
        || tag.eq_ignore_ascii_case("style")
        || tag.eq_ignore_ascii_case("textarea")
        || tag.eq_ignore_ascii_case("title")
        || tag.eq_ignore_ascii_case("xmp")
}

/// Skips the body of a raw-text / RCDATA element until immediately after its matching `</tag>`
/// closing tag (or EOF if unclosed).
fn find_raw_text_end(bytes: &[u8], mut pos: usize, tag_name: &str) -> usize {
    let tag_bytes = tag_name.as_bytes();
    while pos < bytes.len() {
        if bytes[pos] == b'<'
            && pos + 2 + tag_bytes.len() <= bytes.len()
            && bytes[pos + 1] == b'/'
            && bytes[pos + 2..pos + 2 + tag_bytes.len()].eq_ignore_ascii_case(tag_bytes)
        {
            let after = pos + 2 + tag_bytes.len();
            if after < bytes.len()
                && (bytes[after] == b'>'
                    || bytes[after] == b'/'
                    || bytes[after].is_ascii_whitespace())
            {
                let mut gt = after;
                while gt < bytes.len() && bytes[gt] != b'>' {
                    gt += 1;
                }
                return if gt < bytes.len() {
                    gt + 1
                } else {
                    bytes.len()
                };
            }
        }
        pos += 1;
    }
    bytes.len()
}

/// Returns `true` when an HTML `rel` attribute contains the whitespace-separated token `stylesheet`.
fn rel_has_stylesheet(rel: &str) -> bool {
    rel.split_ascii_whitespace()
        .any(|token| token.eq_ignore_ascii_case("stylesheet"))
}

/// Returns `true` when a URL attribute's trimmed path before `?` or `#` equals `expected`.
fn url_path_equals(raw: &str, expected: &str) -> bool {
    raw.split(['?', '#'])
        .next()
        .is_some_and(|path| path.trim() == expected)
}

/// Deletes `tools_dir/{id}` after validating `id`, returning `Ok(true)` if removed or `Ok(false)` if absent.
pub fn delete_tool_dir(tools_dir: &Path, id: &str) -> Result<bool, AppError> {
    manifest::validate_id(id)?;

    let target = tools_dir.join(id);
    let meta = match fs::symlink_metadata(&target) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(AppError::internal(format!(
                "failed to inspect tool directory '{}': {err}",
                target.display()
            )));
        }
    };

    let staging_root = tools_dir.join(STAGING_DIR_NAME);
    fs::create_dir_all(&staging_root).map_err(|err| {
        AppError::internal(format!(
            "failed to create staging directory '{}': {err}",
            staging_root.display()
        ))
    })?;

    let trash_path = staging_root.join(format!("trash-{id}-{}", Uuid::new_v4()));
    fs::rename(&target, &trash_path).map_err(|err| {
        AppError::internal(format!(
            "failed to move tool directory '{}' to trash: {err}",
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
            "failed to remove trashed tool directory '{}': {err}",
            trash_path.display()
        )));
    }

    Ok(true)
}

/// Applies canonical `"visibility"` and downgrade-safe `"draft"` fields to a `manifest.json`
/// JSON object:
/// - `Visibility::Draft` writes BOTH `"visibility": "draft"` AND `"draft": true` (safe on `<= v0.3.5`).
/// - `Visibility::Public` and `Visibility::Private` write `"visibility"` and remove legacy `"draft"`.
fn apply_visibility_to_manifest_object(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    visibility: Visibility,
) {
    obj.insert(
        "visibility".to_string(),
        serde_json::Value::String(visibility.as_str().to_string()),
    );
    match visibility {
        Visibility::Draft => {
            obj.insert("draft".to_string(), serde_json::Value::Bool(true));
        }
        Visibility::Public | Visibility::Private => {
            obj.remove("draft");
        }
    }
}

/// Atomically rewrites `manifest.json` inside `tool_dir` to record `visibility` (preserving all
/// other JSON keys and applying downgrade-safe `draft` writeback rules).
fn write_manifest_visibility_in_dir(
    tool_dir: &Path,
    visibility: Visibility,
) -> Result<(), AppError> {
    let manifest_path = tool_dir.join("manifest.json");
    let manifest_meta = fs::symlink_metadata(&manifest_path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found(format!(
                "manifest.json not found in '{}'",
                tool_dir.display()
            ))
        } else {
            AppError::internal(format!(
                "failed to inspect manifest '{}': {err}",
                manifest_path.display()
            ))
        }
    })?;
    if manifest_meta.file_type().is_symlink() || !manifest_meta.file_type().is_file() {
        return Err(AppError::unprocessable(
            "manifest.json must be a regular file (symlinks are not allowed)",
        ));
    }

    let raw_bytes = fs::read(&manifest_path).map_err(|err| {
        AppError::internal(format!(
            "failed to read manifest '{}': {err}",
            manifest_path.display()
        ))
    })?;

    let mut value: serde_json::Value = serde_json::from_slice(&raw_bytes)
        .map_err(|err| AppError::unprocessable(format!("invalid manifest.json: {err}")))?;

    let obj = value
        .as_object_mut()
        .ok_or_else(|| AppError::unprocessable("manifest.json must be a JSON object"))?;
    apply_visibility_to_manifest_object(obj, visibility);

    let mut pretty = serde_json::to_vec_pretty(&value)
        .map_err(|err| AppError::internal(format!("failed to serialize manifest.json: {err}")))?;
    pretty.push(b'\n');

    let mut tmp = tempfile::NamedTempFile::new_in(tool_dir).map_err(|err| {
        AppError::internal(format!(
            "failed to create temporary manifest file in '{}': {err}",
            tool_dir.display()
        ))
    })?;
    tmp.write_all(&pretty).map_err(|err| {
        AppError::internal(format!("failed to write temporary manifest.json: {err}"))
    })?;
    tmp.flush().map_err(|err| {
        AppError::internal(format!("failed to flush temporary manifest.json: {err}"))
    })?;
    tmp.persist(&manifest_path).map_err(|err| {
        AppError::internal(format!(
            "failed to atomically replace manifest '{}': {}",
            manifest_path.display(),
            err.error
        ))
    })?;

    Ok(())
}

/// Atomically updates `"visibility"` in `tools_dir/{id}/manifest.json` (writing `"draft": true`
/// alongside `"visibility": "draft"` for downgrade safety, and removing legacy `"draft"` for
/// `"public"` and `"private"`) and returns the reloaded [`ToolEntry`].
///
/// Validates `id`, checks that `manifest.json` exists and is a regular file (returning `404` if
/// missing), preserves all other JSON fields in `manifest.json`, writes atomically via a temporary
/// file in the tool directory + rename, and reloads via [`manifest::load_and_validate_manifest`].
pub fn set_tool_visibility(
    tools_dir: &Path,
    id: &str,
    visibility: Visibility,
) -> Result<ToolEntry, AppError> {
    manifest::validate_id(id)?;

    let tool_dir = tools_dir.join(id);
    let dir_meta = fs::symlink_metadata(&tool_dir).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found(format!("tool '{id}' not found"))
        } else {
            AppError::internal(format!(
                "failed to inspect tool directory '{}': {err}",
                tool_dir.display()
            ))
        }
    })?;
    if dir_meta.file_type().is_symlink() || !dir_meta.file_type().is_dir() {
        return Err(AppError::unprocessable(format!(
            "tool path '{}' must be a regular directory",
            tool_dir.display()
        )));
    }

    write_manifest_visibility_in_dir(&tool_dir, visibility)?;

    manifest::load_and_validate_manifest(&tool_dir, Some(id))
}

/// Legacy compatibility wrapper around [`set_tool_visibility`], mapping `true` to
/// [`Visibility::Draft`] and `false` to [`Visibility::Public`].
pub fn set_tool_draft(tools_dir: &Path, id: &str, draft: bool) -> Result<ToolEntry, AppError> {
    set_tool_visibility(
        tools_dir,
        id,
        if draft {
            Visibility::Draft
        } else {
            Visibility::Public
        },
    )
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
/// that contains `<top>/manifest.json` while no root-level `manifest.json` exists.
fn detect_single_wrapper_dir(entries: &[InspectedEntry]) -> Option<PathBuf> {
    let root_manifest = Path::new("manifest.json");
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
    let wrapped_manifest = PathBuf::from(top).join("manifest.json");
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

/// Atomically swaps `staged_path` into `target`, backing up and rolling back if `target` exists,
/// and returns `(updated, optional_backup_dir)` so callers can remove the backup directory after
/// releasing any short-lived exclusive swap lock.
fn install_staged_dir_swap(
    staging_root: &Path,
    staged_path: &Path,
    target: &Path,
    tool_id: &str,
) -> Result<(bool, Option<PathBuf>), AppError> {
    let target_exists = match fs::symlink_metadata(target) {
        Ok(_) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            return Err(AppError::internal(format!(
                "failed to inspect target tool directory '{}': {err}",
                target.display()
            )));
        }
    };

    if target_exists {
        let backup = staging_root.join(format!("backup-{tool_id}-{}", Uuid::new_v4()));
        fs::rename(target, &backup).map_err(|err| {
            AppError::internal(format!(
                "failed to move existing tool '{}' to backup: {err}",
                target.display()
            ))
        })?;

        if let Err(err) = fs::rename(staged_path, target) {
            if let Err(rollback_err) = fs::rename(&backup, target) {
                error!(
                    tool_id,
                    backup = %backup.display(),
                    target = %target.display(),
                    error = %rollback_err,
                    "failed to roll back previous tool directory after rename failure"
                );
            }
            return Err(AppError::internal(format!(
                "failed to move staged tool '{}' into place: {err}",
                target.display()
            )));
        }

        Ok((true, Some(backup)))
    } else {
        fs::rename(staged_path, target).map_err(|err| {
            AppError::internal(format!(
                "failed to move staged tool '{}' into place: {err}",
                target.display()
            ))
        })?;

        Ok((false, None))
    }
}

/// Atomically installs `staged_path` into `target`, backing up and rolling back if `target` exists.
#[cfg(test)]
fn install_staged_dir(
    staging_root: &Path,
    staged_path: &Path,
    target: &Path,
    tool_id: &str,
) -> Result<bool, AppError> {
    let (updated, backup) = install_staged_dir_swap(staging_root, staged_path, target, tool_id)?;
    if let Some(backup) = backup
        && let Err(err) = fs::remove_dir_all(&backup)
    {
        warn!(
            tool_id,
            backup = %backup.display(),
            error = %err,
            "failed to clean up backup directory after atomic replace"
        );
    }
    Ok(updated)
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
        build_zip_with_method(entries, CompressionMethod::Deflated)
    }

    fn build_zip_with_method(entries: &[TestZipEntry<'_>], method: CompressionMethod) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut cursor);
            let options = SimpleFileOptions::default().compression_method(method);
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

    fn staging_is_empty(tools_dir: &Path) -> bool {
        let staging = tools_dir.join(STAGING_DIR_NAME);
        if !staging.exists() {
            return true;
        }
        fs::read_dir(staging)
            .expect("read staging dir")
            .next()
            .is_none()
    }

    #[test]
    fn archive_limits_default_matches_spec() {
        let limits = ArchiveLimits::default();
        assert_eq!(limits.max_zip_bytes, 50 * 1024 * 1024);
        assert_eq!(limits.max_uncompressed_bytes, 200 * 1024 * 1024);
        assert_eq!(limits.max_files, 1_000);
    }

    #[test]
    fn installs_flat_zip_and_cleans_staging() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let zip = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"hello-tool","name":"Hello Tool","tags":["demo"]}"#,
            ),
            TestZipEntry::File("index.html", b"<h1>Hello</h1>"),
            TestZipEntry::File("assets/app.js", b"console.log('hi');"),
        ]);

        let result = install_zip_bytes(&tools_dir, &zip, ArchiveLimits::default()).unwrap();
        assert!(!result.updated);
        assert_eq!(result.entry.id, "hello-tool");
        assert_eq!(result.entry.name, "Hello Tool");
        assert_eq!(result.entry.entry_url, "/t/hello-tool/index.html");

        assert_eq!(
            fs::read_to_string(tools_dir.join("hello-tool/index.html")).unwrap(),
            "<h1>Hello</h1>"
        );
        assert_eq!(
            fs::read_to_string(tools_dir.join("hello-tool/assets/app.js")).unwrap(),
            "console.log('hi');"
        );
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn unwraps_single_top_level_folder_and_ignores_macos_junk() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let zip = build_zip(&[
            TestZipEntry::Dir("my-wrapped-folder/"),
            TestZipEntry::File(
                "my-wrapped-folder/manifest.json",
                br#"{"id":"wrapped-tool","name":"Wrapped Tool"}"#,
            ),
            TestZipEntry::File("my-wrapped-folder/index.html", b"<p>Wrapped</p>"),
            TestZipEntry::File("my-wrapped-folder/.DS_Store", b"junk"),
            TestZipEntry::File("my-wrapped-folder/._index.html", b"appledouble"),
            TestZipEntry::Dir("__MACOSX/"),
            TestZipEntry::Dir("__MACOSX/my-wrapped-folder/"),
            TestZipEntry::File("__MACOSX/my-wrapped-folder/._manifest.json", b"meta"),
            TestZipEntry::File(".DS_Store", b"root-junk"),
        ]);

        let result = install_zip_bytes(&tools_dir, &zip, ArchiveLimits::default()).unwrap();
        assert!(!result.updated);
        assert_eq!(result.entry.id, "wrapped-tool");
        // Installed under manifest.id, not the wrapper folder name.
        assert!(tools_dir.join("wrapped-tool/index.html").is_file());
        assert!(!tools_dir.join("my-wrapped-folder").exists());
        assert!(!tools_dir.join("wrapped-tool/.DS_Store").exists());
        assert!(!tools_dir.join("wrapped-tool/._index.html").exists());
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn atomic_overwrite_replaces_existing_tool_and_failed_overwrite_preserves_old() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let v1 = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"my-tool","name":"My Tool v1","version":"1.0.0"}"#,
            ),
            TestZipEntry::File("index.html", b"<h1>v1</h1>"),
            TestZipEntry::File("old-only.txt", b"old"),
        ]);
        let res1 = install_zip_bytes(&tools_dir, &v1, ArchiveLimits::default()).unwrap();
        assert!(!res1.updated);
        assert_eq!(res1.entry.version, "1.0.0");

        // Overwrite with valid v2
        let v2 = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"my-tool","name":"My Tool v2","version":"2.0.0"}"#,
            ),
            TestZipEntry::File("index.html", b"<h1>v2</h1>"),
        ]);
        let res2 = install_zip_bytes(&tools_dir, &v2, ArchiveLimits::default()).unwrap();
        assert!(res2.updated);
        assert_eq!(res2.entry.version, "2.0.0");
        assert_eq!(
            fs::read_to_string(tools_dir.join("my-tool/index.html")).unwrap(),
            "<h1>v2</h1>"
        );
        assert!(!tools_dir.join("my-tool/old-only.txt").exists());
        assert!(staging_is_empty(&tools_dir));

        // Attempt broken overwrite (missing entry HTML) -> must fail and preserve v2 intact
        let bad_v3 = build_zip(&[TestZipEntry::File(
            "manifest.json",
            br#"{"id":"my-tool","name":"Broken v3","version":"3.0.0"}"#,
        )]);
        let err = install_zip_bytes(&tools_dir, &bad_v3, ArchiveLimits::default()).unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");
        assert_eq!(
            fs::read_to_string(tools_dir.join("my-tool/index.html")).unwrap(),
            "<h1>v2</h1>"
        );
        let preserved = manifest::load_and_validate_manifest(&tools_dir.join("my-tool"), None).unwrap();
        assert_eq!(preserved.version, "2.0.0");
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn rejects_zip_slip_traversal_entries() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let evil = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"evil-tool","name":"Evil"}"#,
            ),
            TestZipEntry::File("index.html", b"ok"),
            TestZipEntry::File("../escaped.txt", b"pwned"),
        ]);

        let err = install_zip_bytes(&tools_dir, &evil, ArchiveLimits::default()).unwrap_err();
        assert_eq!(err.error_code(), "BAD_REQUEST");
        assert!(!tmp.path().join("escaped.txt").exists());
        assert!(!tools_dir.join("evil-tool").exists());
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn rejects_symlink_entries() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let evil = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"sym-tool","name":"Symlink Tool"}"#,
            ),
            TestZipEntry::File("index.html", b"ok"),
            TestZipEntry::Symlink("leak.txt", "/etc/passwd"),
        ]);

        let err = install_zip_bytes(&tools_dir, &evil, ArchiveLimits::default()).unwrap_err();
        assert_eq!(err.error_code(), "BAD_REQUEST");
        assert!(err.to_string().contains("symlink"));
        assert!(!tools_dir.join("sym-tool").exists());
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn rejects_duplicate_normalized_paths() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let dup = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"dup-tool","name":"Dup Tool"}"#,
            ),
            TestZipEntry::File("index.html", b"first"),
            TestZipEntry::File("./index.html", b"second"),
        ]);

        let err = install_zip_bytes(&tools_dir, &dup, ArchiveLimits::default()).unwrap_err();
        assert_eq!(err.error_code(), "BAD_REQUEST");
        assert!(err.to_string().contains("duplicate"));
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn enforces_max_zip_bytes_max_uncompressed_bytes_and_max_files() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        // 1. max_zip_bytes
        let small_zip = build_zip(&[
            TestZipEntry::File("manifest.json", br#"{"id":"t1","name":"T1"}"#),
            TestZipEntry::File("index.html", b"hello"),
        ]);
        let err = install_zip_bytes(
            &tools_dir,
            &small_zip,
            ArchiveLimits {
                max_zip_bytes: 10,
                ..ArchiveLimits::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "PAYLOAD_TOO_LARGE");

        // 2. max_uncompressed_bytes (highly compressible zip bomb simulation)
        let big_zeros = vec![0u8; 256 * 1024]; // 256 KiB of zeros compresses to < 1 KiB
        let bomb_zip = build_zip(&[
            TestZipEntry::File("manifest.json", br#"{"id":"bomb","name":"Bomb"}"#),
            TestZipEntry::File("index.html", b"ok"),
            TestZipEntry::File("payload.bin", &big_zeros),
        ]);
        assert!(
            bomb_zip.len() < 4096,
            "compressed bomb should be tiny, got {} bytes",
            bomb_zip.len()
        );
        let err = install_zip_bytes(
            &tools_dir,
            &bomb_zip,
            ArchiveLimits {
                max_zip_bytes: 1024 * 1024,
                max_uncompressed_bytes: 64 * 1024,
                max_files: 100,
            },
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "PAYLOAD_TOO_LARGE");
        assert!(!tools_dir.join("bomb").exists());
        assert!(staging_is_empty(&tools_dir));

        // 3. max_files
        let many_files = build_zip(&[
            TestZipEntry::File("manifest.json", br#"{"id":"many","name":"Many"}"#),
            TestZipEntry::File("index.html", b"ok"),
            TestZipEntry::File("a.txt", b"a"),
            TestZipEntry::File("b.txt", b"b"),
        ]);
        let err = install_zip_bytes(
            &tools_dir,
            &many_files,
            ArchiveLimits {
                max_zip_bytes: 1024 * 1024,
                max_uncompressed_bytes: 1024 * 1024,
                max_files: 3,
            },
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "PAYLOAD_TOO_LARGE");
        assert!(!tools_dir.join("many").exists());
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn rejects_invalid_zip_bytes() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");
        let err =
            install_zip_bytes(&tools_dir, b"not a zip archive", ArchiveLimits::default()).unwrap_err();
        assert_eq!(err.error_code(), "BAD_REQUEST");
    }

    #[test]
    fn delete_tool_dir_removes_existing_and_handles_missing_and_invalid_ids() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let zip = build_zip(&[
            TestZipEntry::File("manifest.json", br#"{"id":"del-me","name":"Delete Me"}"#),
            TestZipEntry::File("index.html", b"bye"),
        ]);
        install_zip_bytes(&tools_dir, &zip, ArchiveLimits::default()).unwrap();
        assert!(tools_dir.join("del-me").is_dir());

        // Deleting existing tool returns Ok(true) and cleans up trash.
        assert!(delete_tool_dir(&tools_dir, "del-me").unwrap());
        assert!(!tools_dir.join("del-me").exists());
        assert!(staging_is_empty(&tools_dir));

        // Deleting again returns Ok(false).
        assert!(!delete_tool_dir(&tools_dir, "del-me").unwrap());

        // Invalid / traversal IDs are rejected.
        assert!(delete_tool_dir(&tools_dir, "../escape").is_err());
        assert!(delete_tool_dir(&tools_dir, ".staging").is_err());
    }

    #[test]
    fn installs_dot_slash_prefixed_zip_and_rejects_file_dir_collision() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        // Common `cd my-tool && zip -r ../tool.zip .` output
        let dot_zip = build_zip(&[
            TestZipEntry::Dir("./"),
            TestZipEntry::File(
                "./manifest.json",
                br#"{"id":"dot-tool","name":"Dot Prefixed Tool"}"#,
            ),
            TestZipEntry::File("./index.html", b"<h1>Dot</h1>"),
        ]);
        let res = install_zip_bytes(&tools_dir, &dot_zip, ArchiveLimits::default()).unwrap();
        assert_eq!(res.entry.id, "dot-tool");
        assert!(tools_dir.join("dot-tool/index.html").is_file());

        // File vs directory path collision
        let collision_zip = build_zip(&[
            TestZipEntry::File("manifest.json", br#"{"id":"collide","name":"Collide"}"#),
            TestZipEntry::File("index.html", b"ok"),
            TestZipEntry::File("assets", b"file"),
            TestZipEntry::File("assets/app.js", b"nested"),
        ]);
        let err =
            install_zip_bytes(&tools_dir, &collision_zip, ArchiveLimits::default()).unwrap_err();
        assert_eq!(err.error_code(), "BAD_REQUEST");
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn install_staged_dir_rolls_back_backup_if_swap_fails() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");
        let staging_root = tools_dir.join(STAGING_DIR_NAME);
        let target = tools_dir.join("keep-me");
        fs::create_dir_all(&staging_root).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("index.html"), "original").unwrap();

        // Point staged_path to a non-existent directory so the second rename fails
        let missing_staged = staging_root.join("does-not-exist");
        let err = install_staged_dir(&staging_root, &missing_staged, &target, "keep-me")
            .unwrap_err();
        assert_eq!(err.error_code(), "INTERNAL_ERROR");

        // Original target must have been rolled back from backup intact
        assert_eq!(
            fs::read_to_string(target.join("index.html")).unwrap(),
            "original"
        );
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn set_tool_draft_toggles_flag_preserves_fields_and_rejects_missing_or_invalid_id() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let zip = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"draft-demo","name":"Draft Demo","version":"1.2.0","tags":["alpha"],"customField":"keep"}"#,
            ),
            TestZipEntry::File("index.html", b"<h1>Demo</h1>"),
        ]);
        let installed = install_zip_bytes(&tools_dir, &zip, ArchiveLimits::default()).unwrap();
        assert!(!installed.entry.draft);

        // Toggle draft -> true
        let updated_draft = set_tool_draft(&tools_dir, "draft-demo", true).unwrap();
        assert_eq!(updated_draft.visibility, Visibility::Draft);
        assert!(updated_draft.draft);
        assert_eq!(updated_draft.version, "1.2.0");
        assert_eq!(updated_draft.tags, vec!["alpha"]);

        // Existing fields (including extra JSON fields) remain preserved on disk;
        // draft writes both canonical "visibility": "draft" and downgrade-safe "draft": true.
        let raw: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(tools_dir.join("draft-demo/manifest.json")).unwrap())
                .unwrap();
        assert_eq!(raw["visibility"], "draft");
        assert_eq!(raw["draft"], true);
        assert_eq!(raw["customField"], "keep");

        // Toggle draft -> false
        let republished = set_tool_draft(&tools_dir, "draft-demo", false).unwrap();
        assert_eq!(republished.visibility, Visibility::Public);
        assert!(!republished.draft);
        let raw_pub: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(tools_dir.join("draft-demo/manifest.json")).unwrap())
                .unwrap();
        assert_eq!(raw_pub["visibility"], "public");
        assert!(raw_pub.get("draft").is_none());

        // Missing tool -> 404
        let err = set_tool_draft(&tools_dir, "no-such-tool", true).unwrap_err();
        assert_eq!(err.error_code(), "NOT_FOUND");

        // Invalid ID -> 422
        let err = set_tool_draft(&tools_dir, "../escape", true).unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");
    }

    #[test]
    fn set_tool_visibility_migrates_legacy_draft_and_preserves_other_fields() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let zip = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"legacy-draft","name":"Legacy Draft","draft":true,"version":"3.1.0","extra":{"a":1}}"#,
            ),
            TestZipEntry::File("index.html", b"<h1>Legacy</h1>"),
        ]);
        let installed = install_zip_bytes(&tools_dir, &zip, ArchiveLimits::default()).unwrap();
        assert_eq!(installed.entry.visibility, Visibility::Draft);
        assert!(installed.entry.draft);

        // Migrate legacy `"draft": true` to `"visibility": "private"` and verify `"draft"` is removed
        let private_entry =
            set_tool_visibility(&tools_dir, "legacy-draft", Visibility::Private).unwrap();
        assert_eq!(private_entry.visibility, Visibility::Private);
        assert!(!private_entry.draft);
        assert_eq!(private_entry.version, "3.1.0");

        let raw: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(tools_dir.join("legacy-draft/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(raw["visibility"], "private");
        assert!(raw.get("draft").is_none());
        assert_eq!(raw["extra"]["a"], 1);

        // Transition private -> public -> draft
        let public_entry =
            set_tool_visibility(&tools_dir, "legacy-draft", Visibility::Public).unwrap();
        assert_eq!(public_entry.visibility, Visibility::Public);
        assert!(!public_entry.draft);
        let raw_pub: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(tools_dir.join("legacy-draft/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(raw_pub["visibility"], "public");
        assert!(raw_pub.get("draft").is_none());

        let draft_entry =
            set_tool_visibility(&tools_dir, "legacy-draft", Visibility::Draft).unwrap();
        assert_eq!(draft_entry.visibility, Visibility::Draft);
        assert!(draft_entry.draft);
        let raw_draft: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(tools_dir.join("legacy-draft/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(raw_draft["visibility"], "draft");
        assert_eq!(raw_draft["draft"], true);
    }

    #[test]
    fn install_zip_bytes_checked_returns_already_exists_without_modifying_existing_tool() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let v1_zip = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"dup-tool","name":"Original Tool","version":"1.0.0"}"#,
            ),
            TestZipEntry::File("index.html", b"<h1>v1</h1>"),
        ]);
        let first = install_zip_bytes_checked(&tools_dir, &v1_zip, ArchiveLimits::default(), false)
            .unwrap();
        assert!(matches!(
            first,
            InstallOutcome::Installed(InstallResult { updated: false, .. })
        ));

        let v2_zip = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"dup-tool","name":"Updated Tool","version":"2.0.0","draft":true}"#,
            ),
            TestZipEntry::File("index.html", b"<h1>v2</h1>"),
        ]);
        let conflict =
            install_zip_bytes_checked(&tools_dir, &v2_zip, ArchiveLimits::default(), false)
                .unwrap();
        match conflict {
            InstallOutcome::AlreadyExists { existing, incoming } => {
                let existing = existing.expect("existing manifest should be loaded");
                assert_eq!(existing.id, "dup-tool");
                assert_eq!(existing.version, "1.0.0");
                assert_eq!(incoming.id, "dup-tool");
                assert_eq!(incoming.version, "2.0.0");
                assert!(incoming.draft);
            }
            InstallOutcome::Installed(_) => panic!("expected AlreadyExists when overwrite is false"),
        }

        // Original files remain untouched and staging dir is clean
        assert_eq!(
            fs::read_to_string(tools_dir.join("dup-tool/index.html")).unwrap(),
            "<h1>v1</h1>"
        );
        assert!(staging_is_empty(&tools_dir));

        // Explicit overwrite=true replaces atomically
        let replaced =
            install_zip_bytes_checked(&tools_dir, &v2_zip, ArchiveLimits::default(), true).unwrap();
        assert!(matches!(
            replaced,
            InstallOutcome::Installed(InstallResult { updated: true, .. })
        ));
        assert_eq!(
            fs::read_to_string(tools_dir.join("dup-tool/index.html")).unwrap(),
            "<h1>v2</h1>"
        );
    }

    #[test]
    fn install_single_html_checked_stages_manifest_and_handles_conflicts() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let first = install_single_html_checked(
            &tools_dir,
            SingleHtmlToolInput {
                id: "my-html-tool".to_string(),
                name: "  My HTML Tool  ".to_string(),
                html: "<!doctype html><html><head><title>v1</title></head><body><h1>v1</h1></body></html>".to_string(),
                description: Some("First version".to_string()),
                category: Some("Utilities".to_string()),
                tags: Some(vec!["html".to_string(), " single ".to_string(), "html".to_string()]),
                icon: Some("📄".to_string()),
                visibility: Some(Visibility::Public),
                inject_sdk: false,
            },
            ArchiveLimits::default(),
            false,
        )
        .unwrap();

        match first {
            InstallOutcome::Installed(res) => {
                assert!(!res.updated);
                assert_eq!(res.entry.id, "my-html-tool");
                assert_eq!(res.entry.name, "My HTML Tool");
                assert_eq!(res.entry.description, "First version");
                assert_eq!(res.entry.category, "Utilities");
                assert_eq!(res.entry.tags, vec!["html", "single"]);
                assert_eq!(res.entry.icon, "📄");
                assert_eq!(res.entry.runtime, "static");
                assert_eq!(res.entry.entry, "index.html");
                assert!(!res.entry.theme_aware);
                assert_eq!(res.entry.visibility, Visibility::Public);
                assert!(!res.entry.draft);
            }
            InstallOutcome::AlreadyExists { .. } => panic!("expected first install to succeed"),
        }
        assert!(staging_is_empty(&tools_dir));

        // Conflict when allow_overwrite is false
        let conflict = install_single_html_checked(
            &tools_dir,
            SingleHtmlToolInput {
                id: "my-html-tool".to_string(),
                name: "My HTML Tool v2".to_string(),
                html: "<h1>v2</h1>".to_string(),
                description: Some("Second version".to_string()),
                category: None,
                tags: None,
                icon: None,
                visibility: Some(Visibility::Draft),
                inject_sdk: true,
            },
            ArchiveLimits::default(),
            false,
        )
        .unwrap();

        match conflict {
            InstallOutcome::AlreadyExists { existing, incoming } => {
                let existing = existing.expect("existing tool should load");
                assert_eq!(existing.name, "My HTML Tool");
                assert!(!existing.theme_aware);
                assert_eq!(incoming.name, "My HTML Tool v2");
                assert!(incoming.theme_aware);
                assert_eq!(incoming.visibility, Visibility::Draft);
                assert!(incoming.draft);
            }
            InstallOutcome::Installed(_) => panic!("expected AlreadyExists when overwrite=false"),
        }
        assert!(
            fs::read_to_string(tools_dir.join("my-html-tool/index.html"))
                .unwrap()
                .contains("<h1>v1</h1>")
        );
        assert!(staging_is_empty(&tools_dir));

        // Overwrite when allow_overwrite is true
        let replaced = install_single_html_checked(
            &tools_dir,
            SingleHtmlToolInput {
                id: "my-html-tool".to_string(),
                name: "My HTML Tool v2".to_string(),
                html: "<h1>v2</h1>".to_string(),
                description: None,
                category: None,
                tags: None,
                icon: None,
                visibility: Some(Visibility::Draft),
                inject_sdk: true,
            },
            ArchiveLimits::default(),
            true,
        )
        .unwrap();

        match replaced {
            InstallOutcome::Installed(res) => {
                assert!(res.updated);
                assert_eq!(res.entry.name, "My HTML Tool v2");
                assert!(res.entry.theme_aware);
                assert_eq!(res.entry.visibility, Visibility::Draft);
                assert!(res.entry.draft);
            }
            InstallOutcome::AlreadyExists { .. } => panic!("expected overwrite to succeed"),
        }
        let disk_manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(tools_dir.join("my-html-tool/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(disk_manifest["visibility"], "draft");
        assert_eq!(disk_manifest["draft"], true);
        let disk_html = fs::read_to_string(tools_dir.join("my-html-tool/index.html")).unwrap();
        assert!(disk_html.contains("/ui/v1/toolstow.css"));
        assert!(disk_html.contains("/ui/v1/theme.js"));
        assert!(disk_html.contains("<h1>v2</h1>"));
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn install_single_html_checked_validates_inputs_and_size_limit() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let make_input = |id: &str, name: &str, html: &str| SingleHtmlToolInput {
            id: id.to_string(),
            name: name.to_string(),
            html: html.to_string(),
            description: None,
            category: None,
            tags: None,
            icon: None,
            visibility: Some(Visibility::Public),
            inject_sdk: false,
        };

        // Invalid id -> 422
        let err = install_single_html_checked(
            &tools_dir,
            make_input("Bad_ID", "Tool", "<h1>ok</h1>"),
            ArchiveLimits::default(),
            true,
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");

        // Blank name -> 422
        let err = install_single_html_checked(
            &tools_dir,
            make_input("valid-id", "   ", "<h1>ok</h1>"),
            ArchiveLimits::default(),
            true,
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");

        // Blank HTML -> 422
        let err = install_single_html_checked(
            &tools_dir,
            make_input("valid-id", "Valid", "  \n\t "),
            ArchiveLimits::default(),
            true,
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");

        // Exceeds max_zip_bytes -> 413
        let err = install_single_html_checked(
            &tools_dir,
            make_input("valid-id", "Valid", "<h1>too large payload</h1>"),
            ArchiveLimits {
                max_zip_bytes: 10,
                ..ArchiveLimits::default()
            },
            true,
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "PAYLOAD_TOO_LARGE");

        // Path-like nonexistent icon -> 422 and clean staging
        let err = install_single_html_checked(
            &tools_dir,
            SingleHtmlToolInput {
                icon: Some("assets/missing.svg".to_string()),
                ..make_input("valid-id", "Valid", "<h1>ok</h1>")
            },
            ArchiveLimits::default(),
            true,
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY");
        assert!(!tools_dir.join("valid-id").exists());
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn inject_toolstow_sdk_handles_all_html_structures_and_partial_presence() {
        // 1. Standard document with uppercase </HEAD>
        let with_head = "<!doctype html><HTML><HEAD><title>App</title></HEAD><BODY>Hi</BODY></HTML>";
        let injected = inject_toolstow_sdk(with_head);
        let css_pos = injected.find(r#"<link rel="stylesheet" href="/ui/v1/toolstow.css">"#).unwrap();
        let js_pos = injected.find(r#"<script src="/ui/v1/theme.js"></script>"#).unwrap();
        let close_head_pos = injected.find("</HEAD>").unwrap();
        assert!(css_pos < js_pos && js_pos < close_head_pos);

        // 2. Already links toolstow.css only -> injects only theme.js
        let css_only = "<head><link rel=\"stylesheet\" href=\"/ui/v1/toolstow.css\"></head><body>x</body>";
        let injected = inject_toolstow_sdk(css_only);
        assert_eq!(injected.matches("/ui/v1/toolstow.css").count(), 1);
        assert_eq!(injected.matches("/ui/v1/theme.js").count(), 1);

        // 3. Already links theme.js only -> injects only toolstow.css
        let js_only = "<head><script src=\"/ui/v1/theme.js\"></script></head><body>x</body>";
        let injected = inject_toolstow_sdk(js_only);
        assert_eq!(injected.matches("/ui/v1/toolstow.css").count(), 1);
        assert_eq!(injected.matches("/ui/v1/theme.js").count(), 1);

        // 4. Already links both -> untouched
        let both = "<head><link rel=\"stylesheet\" href=\"/ui/v1/toolstow.css\"><script src=\"/ui/v1/theme.js\"></script></head>";
        assert_eq!(inject_toolstow_sdk(both), both);

        // 5. Opening <head lang="en"> without closing </head> -> after <head ...>
        let open_head_only = "<html lang=\"en\"><head data-x=\"1\"><title>Hi</title><body>Ok</body></html>";
        let injected = inject_toolstow_sdk(open_head_only);
        let head_end = injected.find(r#"<head data-x="1">"#).unwrap() + r#"<head data-x="1">"#.len();
        let css_pos = injected.find("/ui/v1/toolstow.css").unwrap();
        assert!(css_pos >= head_end);

        // 6. No <head>, only <body class="app"> (plus <header> inside) -> before <body>, not matching <header>
        let body_only = "<html><body class=\"app\"><header>Top</header><p>Body</p></body></html>";
        let injected = inject_toolstow_sdk(body_only);
        let css_pos = injected.find("/ui/v1/toolstow.css").unwrap();
        let body_pos = injected.find(r#"<body class="app">"#).unwrap();
        assert!(css_pos < body_pos);

        // 7. Leading doctype without head or body -> after doctype
        let doctype_only = "<!DOCTYPE html><div>Hello</div>";
        let injected = inject_toolstow_sdk(doctype_only);
        assert!(injected.starts_with("<!DOCTYPE html>\n"));
        assert!(injected.contains("/ui/v1/toolstow.css"));
        assert!(injected.contains("/ui/v1/theme.js"));

        // 8. Bare HTML fragment with non-ASCII UTF-8 characters -> prepended cleanly
        let fragment = "<h1>你好工具</h1>";
        let injected = inject_toolstow_sdk(fragment);
        assert!(injected.starts_with(r#"<link rel="stylesheet" href="/ui/v1/toolstow.css">"#));
        assert!(injected.ends_with(fragment));

        // 9. Existing /ui/v1/theme.js with inject_sdk=false still sets theme_aware=true
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");
        let outcome = install_single_html_checked(
            &tools_dir,
            SingleHtmlToolInput {
                id: "already-themed".to_string(),
                name: "Already Themed".to_string(),
                html: "<head><script src=\"/ui/v1/theme.js\"></script></head><body>Hi</body>".to_string(),
                description: None,
                category: None,
                tags: None,
                icon: None,
                visibility: Some(Visibility::Public),
                inject_sdk: false,
            },
            ArchiveLimits::default(),
            true,
        )
        .unwrap();
        match outcome {
            InstallOutcome::Installed(res) => assert!(res.entry.theme_aware),
            InstallOutcome::AlreadyExists { .. } => panic!("expected install"),
        }
    }

    #[test]
    fn html_scanner_skips_comments_raw_text_quoted_attrs_and_checks_real_sdk_tags() {
        // 1. Comment containing `</head>` inside `<head>` -> injected before the REAL `</head>`, outside comment
        let commented_head =
            "<!doctype html><html><head><!----><!-- </head> --><title>Test</title></head><body>Hi</body></html>";
        let injected = inject_toolstow_sdk(commented_head);
        let comment_end = injected.find("<!-- </head> -->").unwrap() + "<!-- </head> -->".len();
        let css_pos = injected
            .find(r#"<link rel="stylesheet" href="/ui/v1/toolstow.css">"#)
            .unwrap();
        let js_pos = injected
            .find(r#"<script src="/ui/v1/theme.js"></script>"#)
            .unwrap();
        let real_head_close = injected.rfind("</head>").unwrap();
        assert!(css_pos > comment_end && css_pos < js_pos && js_pos < real_head_close);

        // 2. Unclosed comment to EOF does not fool tag detection; fragment prepends cleanly
        let unclosed_comment = "<!-- </head> <script src=\"/ui/v1/theme.js\"></script>";
        let scan_unclosed = scan_html(unclosed_comment);
        assert!(!scan_unclosed.has_theme_js);
        assert!(scan_unclosed.head_close_start.is_none());
        let injected_unclosed = inject_toolstow_sdk(unclosed_comment);
        assert!(injected_unclosed.starts_with(r#"<link rel="stylesheet" href="/ui/v1/toolstow.css">"#));
        assert!(scan_html(&injected_unclosed).has_theme_js);

        // 3. `<pre>/ui/v1/theme.js /ui/v1/toolstow.css</pre>` and `<!-- /ui/v1/theme.js -->`:
        //    NOT detected as loaded; theme_aware=false when inject_sdk=false, injected when inject_sdk=true
        let doc_text_and_comment = "<!doctype html><html><head><!-- /ui/v1/theme.js --><title>Docs</title></head><body><pre>/ui/v1/theme.js /ui/v1/toolstow.css</pre></body></html>";
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        let no_inject = install_single_html_checked(
            &tools_dir,
            SingleHtmlToolInput {
                id: "docs-no-inject".to_string(),
                name: "Docs No Inject".to_string(),
                html: doc_text_and_comment.to_string(),
                description: None,
                category: None,
                tags: None,
                icon: None,
                visibility: Some(Visibility::Public),
                inject_sdk: false,
            },
            ArchiveLimits::default(),
            false,
        )
        .unwrap();
        match no_inject {
            InstallOutcome::Installed(res) => assert!(!res.entry.theme_aware),
            InstallOutcome::AlreadyExists { .. } => panic!("expected install"),
        }

        let with_inject = install_single_html_checked(
            &tools_dir,
            SingleHtmlToolInput {
                id: "docs-with-inject".to_string(),
                name: "Docs With Inject".to_string(),
                html: doc_text_and_comment.to_string(),
                description: None,
                category: None,
                tags: None,
                icon: None,
                visibility: Some(Visibility::Public),
                inject_sdk: true,
            },
            ArchiveLimits::default(),
            false,
        )
        .unwrap();
        match with_inject {
            InstallOutcome::Installed(res) => assert!(res.entry.theme_aware),
            InstallOutcome::AlreadyExists { .. } => panic!("expected install"),
        }
        let disk_html =
            fs::read_to_string(tools_dir.join("docs-with-inject/index.html")).unwrap();
        assert!(disk_html.contains(r#"<link rel="stylesheet" href="/ui/v1/toolstow.css">"#));
        assert!(disk_html.contains(r#"<script src="/ui/v1/theme.js"></script>"#));
        assert!(
            disk_html
                .find(r#"<script src="/ui/v1/theme.js"></script>"#)
                .unwrap()
                < disk_html.find("</head>").unwrap()
        );

        // 4. `<script>const x = "</head>";</script>` inside `<head>` -> injected before REAL `</head>`
        let script_with_head_close =
            r#"<!doctype html><html><head><script>const x = "</head>";</script></head><body>Hi</body></html>"#;
        let injected_script = inject_toolstow_sdk(script_with_head_close);
        let script_end = injected_script.find("</script>").unwrap() + "</script>".len();
        let injected_css = injected_script
            .find(r#"<link rel="stylesheet" href="/ui/v1/toolstow.css">"#)
            .unwrap();
        let real_close = injected_script.rfind("</head>").unwrap();
        assert!(injected_css >= script_end && injected_css < real_close);

        // 5. Quoted attributes and raw-text/RCDATA bodies (`style`, `textarea`, `title`, `xmp`)
        let tricky = concat!(
            r#"<html data-a="</head>" data-b='<body>'>"#,
            r#"<head data-x="</head>">"#,
            r#"<title></head></title>"#,
            r#"<style>/* </head> */ body::after { content: "</head>"; }</style>"#,
            r#"</head>"#,
            r#"<body>"#,
            r#"<textarea><script src="/ui/v1/theme.js"></script></textarea>"#,
            r#"<xmp><link rel="stylesheet" href="/ui/v1/toolstow.css"></xmp>"#,
            r#"</body></html>"#
        );
        let scan_tricky = scan_html(tricky);
        assert!(!scan_tricky.has_toolstow_css);
        assert!(!scan_tricky.has_theme_js);
        assert_eq!(scan_tricky.head_close_start, tricky.rfind("</head>"));
        assert_eq!(scan_tricky.body_open_start, tricky.rfind("<body>"));

        // 6. `<link rel="preload" href="/ui/v1/toolstow.css">` does NOT count as CSS loaded;
        //    `<link rel="stylesheet" href="/ui/v1/toolstow.css?v=1">` DOES count.
        let preload_only =
            r#"<head><link rel="preload" as="style" href="/ui/v1/toolstow.css"></head>"#;
        let scan_preload = scan_html(preload_only);
        assert!(!scan_preload.has_toolstow_css);
        let injected_preload = inject_toolstow_sdk(preload_only);
        assert!(injected_preload.contains(r#"<link rel="stylesheet" href="/ui/v1/toolstow.css">"#));
        assert!(injected_preload.contains(r#"<script src="/ui/v1/theme.js"></script>"#));

        let stylesheet_with_query = concat!(
            r#"<head>"#,
            r#"<link rel="alternate StyleSheet" href=" /ui/v1/toolstow.css?v=1 ">"#,
            r#"<script src='/ui/v1/theme.js?v=2#init'></script>"#,
            r#"</head>"#
        );
        let scan_loaded = scan_html(stylesheet_with_query);
        assert!(scan_loaded.has_toolstow_css);
        assert!(scan_loaded.has_theme_js);
        assert_eq!(inject_toolstow_sdk(stylesheet_with_query), stylesheet_with_query);
    }

    #[test]
    fn zip_overwrite_omitting_visibility_preserves_existing_private_and_draft_while_explicit_overrides()
    {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        // 1. Install a private tool
        let v1_private = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"vis-zip","name":"Vis Zip v1","version":"1.0.0","visibility":"private"}"#,
            ),
            TestZipEntry::File("index.html", b"<h1>v1 private</h1>"),
        ]);
        let res1 = install_zip_bytes(&tools_dir, &v1_private, ArchiveLimits::default()).unwrap();
        assert_eq!(res1.entry.visibility, Visibility::Private);

        // 2. Incoming v2 omits both `visibility` and `draft`
        let v2_omitted = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"vis-zip","name":"Vis Zip v2","version":"2.0.0","customMeta":"keep-me"}"#,
            ),
            TestZipEntry::File("index.html", b"<h1>v2</h1>"),
        ]);

        // 2a. allow_overwrite=false -> AlreadyExists reports inherited `private` on incoming
        match install_zip_bytes_checked(&tools_dir, &v2_omitted, ArchiveLimits::default(), false)
            .unwrap()
        {
            InstallOutcome::AlreadyExists { existing, incoming } => {
                assert_eq!(existing.unwrap().visibility, Visibility::Private);
                assert_eq!(incoming.visibility, Visibility::Private);
                assert!(!incoming.draft);
                assert_eq!(incoming.version, "2.0.0");
            }
            InstallOutcome::Installed(_) => panic!("expected AlreadyExists"),
        }

        // 2b. allow_overwrite=true -> installed entry & disk manifest inherit `private` (no legacy draft key)
        match install_zip_bytes_checked(&tools_dir, &v2_omitted, ArchiveLimits::default(), true)
            .unwrap()
        {
            InstallOutcome::Installed(res) => {
                assert!(res.updated);
                assert_eq!(res.entry.visibility, Visibility::Private);
                assert!(!res.entry.draft);
                assert_eq!(res.entry.version, "2.0.0");
            }
            InstallOutcome::AlreadyExists { .. } => panic!("expected Installed"),
        }
        let disk_priv: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(tools_dir.join("vis-zip/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(disk_priv["visibility"], "private");
        assert!(disk_priv.get("draft").is_none());
        assert_eq!(disk_priv["customMeta"], "keep-me");

        // 3. Switch to `draft`, then overwrite with omitted visibility -> inherits `draft` AND writes `"draft": true`
        set_tool_visibility(&tools_dir, "vis-zip", Visibility::Draft).unwrap();
        match install_zip_bytes_checked(&tools_dir, &v2_omitted, ArchiveLimits::default(), false)
            .unwrap()
        {
            InstallOutcome::AlreadyExists { existing, incoming } => {
                assert_eq!(existing.unwrap().visibility, Visibility::Draft);
                assert_eq!(incoming.visibility, Visibility::Draft);
                assert!(incoming.draft);
            }
            InstallOutcome::Installed(_) => panic!("expected AlreadyExists"),
        }
        match install_zip_bytes_checked(&tools_dir, &v2_omitted, ArchiveLimits::default(), true)
            .unwrap()
        {
            InstallOutcome::Installed(res) => {
                assert_eq!(res.entry.visibility, Visibility::Draft);
                assert!(res.entry.draft);
            }
            InstallOutcome::AlreadyExists { .. } => panic!("expected Installed"),
        }
        let disk_draft: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(tools_dir.join("vis-zip/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(disk_draft["visibility"], "draft");
        assert_eq!(disk_draft["draft"], true);
        assert_eq!(disk_draft["customMeta"], "keep-me");

        // 4. Explicit `"visibility": "public"` overrides inherited draft -> becomes public
        let v3_explicit_public = build_zip(&[
            TestZipEntry::File(
                "manifest.json",
                br#"{"id":"vis-zip","name":"Vis Zip v3","version":"3.0.0","visibility":"public"}"#,
            ),
            TestZipEntry::File("index.html", b"<h1>v3 public</h1>"),
        ]);
        let res3 =
            install_zip_bytes(&tools_dir, &v3_explicit_public, ArchiveLimits::default()).unwrap();
        assert_eq!(res3.entry.visibility, Visibility::Public);
        assert!(!res3.entry.draft);
        assert!(staging_is_empty(&tools_dir));
    }

    #[test]
    fn single_html_overwrite_omitting_visibility_inherits_existing_and_defaults_public_for_new() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        // 1. Fresh install with visibility: None -> defaults to Public, omits legacy `draft`
        let fresh = install_single_html_checked(
            &tools_dir,
            SingleHtmlToolInput {
                id: "new-html".to_string(),
                name: "New HTML".to_string(),
                html: "<h1>New</h1>".to_string(),
                description: None,
                category: None,
                tags: None,
                icon: None,
                visibility: None,
                inject_sdk: false,
            },
            ArchiveLimits::default(),
            false,
        )
        .unwrap();
        match fresh {
            InstallOutcome::Installed(res) => {
                assert_eq!(res.entry.visibility, Visibility::Public);
                assert!(!res.entry.draft);
            }
            InstallOutcome::AlreadyExists { .. } => panic!("expected Installed"),
        }
        let disk_new: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(tools_dir.join("new-html/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(disk_new["visibility"], "public");
        assert!(disk_new.get("draft").is_none());

        // 2. Patch to Private, then overwrite with visibility: None -> preserves Private in 409 & 200
        set_tool_visibility(&tools_dir, "new-html", Visibility::Private).unwrap();
        let make_omitted = || SingleHtmlToolInput {
            id: "new-html".to_string(),
            name: "Updated HTML".to_string(),
            html: "<h1>Updated</h1>".to_string(),
            description: None,
            category: None,
            tags: None,
            icon: None,
            visibility: None,
            inject_sdk: false,
        };

        match install_single_html_checked(&tools_dir, make_omitted(), ArchiveLimits::default(), false)
            .unwrap()
        {
            InstallOutcome::AlreadyExists { existing, incoming } => {
                assert_eq!(existing.unwrap().visibility, Visibility::Private);
                assert_eq!(incoming.visibility, Visibility::Private);
            }
            InstallOutcome::Installed(_) => panic!("expected AlreadyExists"),
        }
        match install_single_html_checked(&tools_dir, make_omitted(), ArchiveLimits::default(), true)
            .unwrap()
        {
            InstallOutcome::Installed(res) => {
                assert_eq!(res.entry.visibility, Visibility::Private);
                assert!(!res.entry.draft);
            }
            InstallOutcome::AlreadyExists { .. } => panic!("expected Installed"),
        }
        let disk_priv: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(tools_dir.join("new-html/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(disk_priv["visibility"], "private");
        assert!(disk_priv.get("draft").is_none());

        // 3. Patch to Draft, then overwrite with visibility: None -> preserves Draft AND writes `"draft": true`
        set_tool_visibility(&tools_dir, "new-html", Visibility::Draft).unwrap();
        match install_single_html_checked(&tools_dir, make_omitted(), ArchiveLimits::default(), true)
            .unwrap()
        {
            InstallOutcome::Installed(res) => {
                assert_eq!(res.entry.visibility, Visibility::Draft);
                assert!(res.entry.draft);
            }
            InstallOutcome::AlreadyExists { .. } => panic!("expected Installed"),
        }
        let disk_draft: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(tools_dir.join("new-html/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(disk_draft["visibility"], "draft");
        assert_eq!(disk_draft["draft"], true);
    }
}
