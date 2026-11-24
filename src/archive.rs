//! Safe `.zip` archive unpacking, quota enforcement, and atomic directory installation.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use tracing::{error, warn};
use uuid::Uuid;
use zip::ZipArchive;

use crate::error::AppError;
use crate::manifest::{self, ToolEntry};

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

/// Validates, extracts, and atomically installs a `.zip` tool package into `tools_dir/{id}`.
pub fn install_zip_bytes(
    tools_dir: &Path,
    zip_bytes: &[u8],
    limits: ArchiveLimits,
) -> Result<InstallResult, AppError> {
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
    let entry = manifest::load_and_validate_manifest(stage_dir.path(), None)?;

    // Atomically move the validated staging directory into `tools_dir/{id}`.
    let staged_path = stage_dir.keep();
    let mut cleanup = CleanupPath::new(staged_path.clone());
    let target = tools_dir.join(&entry.id);

    let updated = install_staged_dir(&staging_root, &staged_path, &target, &entry.id)?;
    cleanup.disarm();

    Ok(InstallResult { entry, updated })
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

/// Atomically installs `staged_path` into `target`, backing up and rolling back if `target` exists.
fn install_staged_dir(
    staging_root: &Path,
    staged_path: &Path,
    target: &Path,
    tool_id: &str,
) -> Result<bool, AppError> {
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

        if let Err(err) = fs::remove_dir_all(&backup) {
            warn!(
                tool_id,
                backup = %backup.display(),
                error = %err,
                "failed to clean up backup directory after atomic replace"
            );
        }

        Ok(true)
    } else {
        fs::rename(staged_path, target).map_err(|err| {
            AppError::internal(format!(
                "failed to move staged tool '{}' into place: {err}",
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
}
