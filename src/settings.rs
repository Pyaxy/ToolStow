//! File-backed site settings persistence (`<data_dir>/settings.json`).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use tracing::warn;

use crate::error::AppError;

/// Default site title shown in the public frontend shell and document title.
pub const DEFAULT_SITE_TITLE: &str = "ToolStow";
/// Default site description shown in the public frontend shell.
pub const DEFAULT_SITE_DESCRIPTION: &str = "Personal Micro-App Toolbox";
/// Default active theme short name (built-in theme).
pub const DEFAULT_ACTIVE_THEME: &str = "default";
/// Maximum allowed character length for `siteTitle`.
pub const MAX_SITE_TITLE_LEN: usize = 80;
/// Maximum allowed character length for `siteDescription`.
pub const MAX_SITE_DESCRIPTION_LEN: usize = 240;

fn default_site_title() -> String {
    DEFAULT_SITE_TITLE.to_string()
}

fn default_site_description() -> String {
    DEFAULT_SITE_DESCRIPTION.to_string()
}

fn default_active_theme() -> String {
    DEFAULT_ACTIVE_THEME.to_string()
}

/// Site-wide presentation settings persisted in `<data_dir>/settings.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteSettings {
    /// Human-readable site title (1..=80 characters).
    #[serde(default = "default_site_title")]
    pub site_title: String,
    /// Short site subtitle / description (0..=240 characters).
    #[serde(default = "default_site_description")]
    pub site_description: String,
    /// Active theme short name (`"default"` for built-in theme, or installed theme short).
    #[serde(default = "default_active_theme")]
    pub active_theme: String,
}

impl Default for SiteSettings {
    fn default() -> Self {
        Self {
            site_title: default_site_title(),
            site_description: default_site_description(),
            active_theme: default_active_theme(),
        }
    }
}

/// Partial or full update payload for [`SiteSettings`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteSettingsUpdate {
    /// Optional updated site title (trimmed, non-empty, max 80 chars).
    #[serde(default)]
    pub site_title: Option<String>,
    /// Optional updated site description (trimmed, max 240 chars).
    #[serde(default)]
    pub site_description: Option<String>,
}

/// Validates and normalizes `site_title`.
fn validate_site_title(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::bad_request("siteTitle must not be empty"));
    }
    if trimmed.chars().count() > MAX_SITE_TITLE_LEN {
        return Err(AppError::bad_request(format!(
            "siteTitle must be at most {MAX_SITE_TITLE_LEN} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Validates and normalizes `site_description`.
fn validate_site_description(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.chars().count() > MAX_SITE_DESCRIPTION_LEN {
        return Err(AppError::bad_request(format!(
            "siteDescription must be at most {MAX_SITE_DESCRIPTION_LEN} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Validates `active_theme` (`"default"` or a valid theme short identifier).
fn validate_active_theme(raw: &str) -> Result<String, AppError> {
    if raw == DEFAULT_ACTIVE_THEME {
        return Ok(DEFAULT_ACTIVE_THEME.to_string());
    }
    crate::themes::validate_theme_short(raw)?;
    Ok(raw.to_string())
}

/// Thread-safe, atomically persisted store for [`SiteSettings`].
#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
    settings: Arc<RwLock<SiteSettings>>,
    write_lock: Arc<Mutex<()>>,
}

impl SettingsStore {
    /// Loads [`SiteSettings`] from `path`.
    ///
    /// - If `path` does not exist, initializes with [`SiteSettings::default()`].
    /// - If `path` cannot be read or contains invalid JSON, logs a warning and falls back to defaults.
    /// - Individual invalid fields inside otherwise valid JSON fall back to their defaults with a warning.
    pub fn load(path: PathBuf) -> Self {
        let initial = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<SiteSettings>(&bytes) {
                Ok(parsed) => sanitize_loaded_settings(&path, parsed),
                Err(err) => {
                    warn!(
                        path = %path.display(),
                        error = %err,
                        "failed to parse settings.json; falling back to defaults"
                    );
                    SiteSettings::default()
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => SiteSettings::default(),
            Err(err) => {
                warn!(
                    path = %path.display(),
                    error = %err,
                    "failed to read settings.json; falling back to defaults"
                );
                SiteSettings::default()
            }
        };

        Self {
            path,
            settings: Arc::new(RwLock::new(initial)),
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Returns the filesystem path of the backing `settings.json` file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a cloned snapshot of the current [`SiteSettings`].
    pub fn get(&self) -> SiteSettings {
        let guard = self.settings.read().unwrap_or_else(|e| e.into_inner());
        guard.clone()
    }

    /// Validates `update`, atomically persists the resulting [`SiteSettings`] to disk
    /// via a temp file in the parent directory + rename, and updates the in-memory cache.
    pub fn update(&self, update: SiteSettingsUpdate) -> Result<SiteSettings, AppError> {
        let _write_guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());

        let mut next = self.get();

        if let Some(title) = update.site_title {
            next.site_title = validate_site_title(&title)?;
        }
        if let Some(description) = update.site_description {
            next.site_description = validate_site_description(&description)?;
        }

        self.persist_settings(&next)?;

        {
            let mut guard = self.settings.write().unwrap_or_else(|e| e.into_inner());
            *guard = next.clone();
        }

        Ok(next)
    }

    /// Validates `short` (`"default"` or a valid theme short), atomically persists the updated
    /// [`SiteSettings`] to disk, and updates the in-memory cache.
    pub fn set_active_theme(&self, short: String) -> Result<SiteSettings, AppError> {
        let _write_guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());

        let validated = validate_active_theme(&short)?;
        let mut next = self.get();
        next.active_theme = validated;

        self.persist_settings(&next)?;

        {
            let mut guard = self.settings.write().unwrap_or_else(|e| e.into_inner());
            *guard = next.clone();
        }

        Ok(next)
    }

    /// Atomically writes `next` as pretty-printed JSON to `self.path` via a same-directory temp file.
    fn persist_settings(&self, next: &SiteSettings) -> Result<(), AppError> {
        let parent_dir = match self.path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };

        fs::create_dir_all(parent_dir).map_err(|err| {
            AppError::internal(format!(
                "failed to create settings directory '{}': {err}",
                parent_dir.display()
            ))
        })?;

        let mut json_bytes = serde_json::to_vec_pretty(next)
            .map_err(|err| AppError::internal(format!("failed to serialize settings: {err}")))?;
        json_bytes.push(b'\n');

        let mut tmp = NamedTempFile::new_in(parent_dir).map_err(|err| {
            AppError::internal(format!(
                "failed to create temporary settings file in '{}': {err}",
                parent_dir.display()
            ))
        })?;

        tmp.write_all(&json_bytes).map_err(|err| {
            AppError::internal(format!("failed to write temporary settings file: {err}"))
        })?;
        tmp.flush().map_err(|err| {
            AppError::internal(format!("failed to flush temporary settings file: {err}"))
        })?;

        tmp.persist(&self.path).map_err(|err| {
            AppError::internal(format!(
                "failed to persist settings file '{}': {}",
                self.path.display(),
                err.error
            ))
        })?;

        Ok(())
    }
}

/// Sanitizes loaded settings fields so out-of-range values on disk degrade gracefully to defaults.
fn sanitize_loaded_settings(path: &Path, loaded: SiteSettings) -> SiteSettings {
    let defaults = SiteSettings::default();

    let site_title = match validate_site_title(&loaded.site_title) {
        Ok(v) => v,
        Err(err) => {
            warn!(path = %path.display(), error = %err, "invalid siteTitle in settings.json; using default");
            defaults.site_title
        }
    };

    let site_description = match validate_site_description(&loaded.site_description) {
        Ok(v) => v,
        Err(err) => {
            warn!(path = %path.display(), error = %err, "invalid siteDescription in settings.json; using default");
            defaults.site_description
        }
    };

    let active_theme = match validate_active_theme(&loaded.active_theme) {
        Ok(v) => v,
        Err(err) => {
            warn!(path = %path.display(), error = %err, "invalid activeTheme in settings.json; using default");
            defaults.active_theme
        }
    };

    SiteSettings {
        site_title,
        site_description,
        active_theme,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn loads_defaults_when_missing_or_corrupt() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");

        // Missing file -> defaults
        let store = SettingsStore::load(path.clone());
        assert_eq!(store.get(), SiteSettings::default());
        assert_eq!(store.get().site_title, "ToolStow");
        assert_eq!(store.get().site_description, "Personal Micro-App Toolbox");

        // Corrupt JSON -> logs warning and falls back to defaults
        fs::write(&path, "{not valid json").unwrap();
        let store2 = SettingsStore::load(path);
        assert_eq!(store2.get(), SiteSettings::default());
    }

    #[test]
    fn updates_atomically_persists_pretty_json_and_reloads() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested/data/settings.json");
        let store = SettingsStore::load(path.clone());

        // Partial update: title only
        let updated = store
            .update(SiteSettingsUpdate {
                site_title: Some("  My Toolbox  ".to_string()),
                site_description: None,
            })
            .expect("valid partial update");

        assert_eq!(updated.site_title, "My Toolbox");
        assert_eq!(updated.site_description, DEFAULT_SITE_DESCRIPTION);
        assert_eq!(store.get(), updated);

        // Verify persisted JSON uses camelCase keys
        let disk_raw = fs::read_to_string(&path).unwrap();
        let disk_json: serde_json::Value = serde_json::from_str(&disk_raw).unwrap();
        assert_eq!(disk_json["siteTitle"], "My Toolbox");
        assert_eq!(disk_json["siteDescription"], DEFAULT_SITE_DESCRIPTION);

        // Partial update: description can be cleared
        let updated2 = store
            .update(SiteSettingsUpdate {
                site_title: None,
                site_description: Some("   ".to_string()),
            })
            .expect("valid second update");
        assert_eq!(updated2.site_title, "My Toolbox");
        assert_eq!(updated2.site_description, "");

        // Reload from disk confirms persistence
        let reloaded = SettingsStore::load(path);
        assert_eq!(reloaded.get(), updated2);
    }

    #[test]
    fn rejects_invalid_updates_without_mutating_state() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");
        let store = SettingsStore::load(path.clone());

        // Empty title
        let err = store
            .update(SiteSettingsUpdate {
                site_title: Some("   ".to_string()),
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err.error_code(), "BAD_REQUEST");

        // Title > 80 chars
        let err = store
            .update(SiteSettingsUpdate {
                site_title: Some("a".repeat(81)),
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err.error_code(), "BAD_REQUEST");

        // Description > 240 chars
        let err = store
            .update(SiteSettingsUpdate {
                site_description: Some("d".repeat(241)),
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err.error_code(), "BAD_REQUEST");

        // State and disk remain untouched
        assert_eq!(store.get(), SiteSettings::default());
        assert!(!path.exists());
    }

    #[test]
    fn loads_legacy_settings_json_without_active_theme_and_sanitizes_invalid_active_theme() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");

        // Legacy settings.json without activeTheme loads seamlessly with "default"
        fs::write(
            &path,
            r##"{"siteTitle": "Legacy", "siteDescription": "Old", "defaultAccent": "#111"}"##,
        )
        .unwrap();
        let store = SettingsStore::load(path.clone());
        let loaded = store.get();
        assert_eq!(loaded.site_title, "Legacy");
        assert_eq!(loaded.active_theme, DEFAULT_ACTIVE_THEME);

        // Invalid activeTheme on disk falls back to "default"
        fs::write(
            &path,
            r#"{"siteTitle": "Legacy", "activeTheme": "../escape"}"#,
        )
        .unwrap();
        let store2 = SettingsStore::load(path);
        assert_eq!(store2.get().active_theme, DEFAULT_ACTIVE_THEME);
    }

    #[test]
    fn set_active_theme_validates_persists_and_preserves_other_fields() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested/settings.json");
        let store = SettingsStore::load(path.clone());

        // Invalid theme shorts are rejected without mutating state or disk
        for bad in ["", "../evil", "Upper", "double--dash", "-lead", "trail-", " space"] {
            let err = store.set_active_theme(bad.to_string()).unwrap_err();
            assert_eq!(err.error_code(), "UNPROCESSABLE_ENTITY", "expected '{bad}' to fail");
        }
        assert_eq!(store.get().active_theme, DEFAULT_ACTIVE_THEME);
        assert!(!path.exists());

        // Valid installed theme short persists as camelCase activeTheme
        let updated = store
            .set_active_theme("nord-minimal".to_string())
            .expect("valid theme short");
        assert_eq!(updated.active_theme, "nord-minimal");
        assert_eq!(store.get().active_theme, "nord-minimal");

        let disk_raw = fs::read_to_string(&path).unwrap();
        let disk_json: serde_json::Value = serde_json::from_str(&disk_raw).unwrap();
        assert_eq!(disk_json["activeTheme"], "nord-minimal");

        // Regular settings update preserves active_theme
        let updated2 = store
            .update(SiteSettingsUpdate {
                site_title: Some("Custom Title".to_string()),
                ..Default::default()
            })
            .expect("update title");
        assert_eq!(updated2.site_title, "Custom Title");
        assert_eq!(updated2.active_theme, "nord-minimal");

        // Reload from disk preserves active_theme
        let reloaded = SettingsStore::load(path.clone());
        assert_eq!(reloaded.get().active_theme, "nord-minimal");

        // Switching back to "default" succeeds
        let reset = reloaded
            .set_active_theme(DEFAULT_ACTIVE_THEME.to_string())
            .expect("reset to default");
        assert_eq!(reset.active_theme, DEFAULT_ACTIVE_THEME);
        assert_eq!(SettingsStore::load(path).get().active_theme, DEFAULT_ACTIVE_THEME);
    }
}
