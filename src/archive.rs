//! Safe `.zip` archive unpacking, quota enforcement, and atomic directory installation.

use std::path::Path;

use crate::error::AppError;
use crate::manifest::ToolEntry;

/// Default maximum compressed `.zip` size (50 MiB).
pub const DEFAULT_MAX_ZIP_BYTES: usize = 50 * 1024 * 1024;
/// Default maximum total uncompressed bytes across all entries in a `.zip` (200 MiB).
pub const DEFAULT_MAX_UNCOMPRESSED_BYTES: u64 = 200 * 1024 * 1024;
/// Default maximum number of file entries permitted inside a single `.zip`.
pub const DEFAULT_MAX_FILES: usize = 1_000;

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

/// Validates, extracts, and atomically installs a `.zip` tool package into `tools_dir/{id}`.
#[allow(unused_variables)]
pub fn install_zip_bytes(
    tools_dir: &Path,
    zip_bytes: &[u8],
    limits: ArchiveLimits,
) -> Result<InstallResult, AppError> {
    todo!("implemented by storage module")
}

/// Deletes `tools_dir/{id}` after validating `id`, returning `Ok(true)` if removed or `Ok(false)` if absent.
#[allow(unused_variables)]
pub fn delete_tool_dir(tools_dir: &Path, id: &str) -> Result<bool, AppError> {
    todo!("implemented by storage module")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_limits_default_matches_spec() {
        let limits = ArchiveLimits::default();
        assert_eq!(limits.max_zip_bytes, 50 * 1024 * 1024);
        assert_eq!(limits.max_uncompressed_bytes, 200 * 1024 * 1024);
        assert_eq!(limits.max_files, 1_000);
    }
}
