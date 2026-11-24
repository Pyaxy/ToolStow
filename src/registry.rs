//! In-memory tool registry, directory scanning, and filesystem watcher.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Context;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{debug, warn};

use crate::manifest::{self, ToolEntry};

/// Debounce window applied to bursty filesystem events before rescanning `tools_dir`.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// Internal signal sent to the watcher debounce thread.
#[derive(Debug)]
enum WatchSignal {
    Event,
    Shutdown,
}

/// Handle keeping the background filesystem watcher alive until dropped.
pub struct WatcherHandle {
    watcher: Option<RecommendedWatcher>,
    shutdown_tx: Option<mpsc::Sender<WatchSignal>>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for WatcherHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WatcherHandle")
            .field("active", &self.watcher.is_some())
            .finish()
    }
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        // Stop producing new events first, then wake and join the debounce thread cleanly.
        drop(self.watcher.take());
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(WatchSignal::Shutdown);
        }
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

/// Thread-safe in-memory registry of validated tools backed by a `tools/` directory.
#[derive(Debug)]
pub struct ToolRegistry {
    tools_dir: PathBuf,
    entries: Arc<RwLock<HashMap<String, ToolEntry>>>,
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl ToolRegistry {
    /// Creates an empty [`ToolRegistry`] bound to `tools_dir`.
    pub fn new(tools_dir: PathBuf) -> Self {
        Self {
            tools_dir,
            entries: Arc::new(RwLock::new(HashMap::new())),
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Returns the root `tools/` directory path managed by this registry.
    pub fn tools_dir(&self) -> &Path {
        &self.tools_dir
    }

    /// Scans `tools_dir` for non-hidden tool subdirectories and rebuilds the in-memory index.
    ///
    /// Invalid or mismatched tool directories are logged with [`tracing::warn!`] and skipped
    /// so a single broken tool never blocks the rest of the registry.
    pub fn scan_all(&self) -> anyhow::Result<usize> {
        fs::create_dir_all(&self.tools_dir).with_context(|| {
            format!(
                "failed to ensure tools directory '{}'",
                self.tools_dir.display()
            )
        })?;

        let read_dir = fs::read_dir(&self.tools_dir).with_context(|| {
            format!(
                "failed to read tools directory '{}'",
                self.tools_dir.display()
            )
        })?;

        let mut next_entries = HashMap::new();

        for dir_entry_res in read_dir {
            let dir_entry = match dir_entry_res {
                Ok(entry) => entry,
                Err(err) => {
                    warn!(
                        tools_dir = %self.tools_dir.display(),
                        error = %err,
                        "failed to read entry in tools directory; skipping"
                    );
                    continue;
                }
            };

            let file_name = dir_entry.file_name();
            let Some(dir_name) = file_name.to_str() else {
                warn!(
                    path = %dir_entry.path().display(),
                    "skipping tool entry with non-UTF-8 directory name"
                );
                continue;
            };

            // Ignore hidden files and system directories (e.g. `.staging`, `.DS_Store`).
            if dir_name.starts_with('.') {
                continue;
            }

            let file_type = match dir_entry.file_type() {
                Ok(ft) => ft,
                Err(err) => {
                    warn!(
                        dir = %dir_name,
                        error = %err,
                        "failed to inspect directory entry type; skipping"
                    );
                    continue;
                }
            };

            if file_type.is_symlink() {
                warn!(
                    dir = %dir_name,
                    "skipping symlinked entry in tools directory"
                );
                continue;
            }

            if !file_type.is_dir() {
                continue;
            }

            let path = dir_entry.path();
            match manifest::load_and_validate_manifest(&path, Some(dir_name)) {
                Ok(entry) => {
                    next_entries.insert(entry.id.clone(), entry);
                }
                Err(err) => {
                    warn!(
                        dir = %dir_name,
                        path = %path.display(),
                        error = %err,
                        "skipping invalid tool directory"
                    );
                }
            }
        }

        let count = next_entries.len();
        {
            let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
            *guard = next_entries;
        }

        Ok(count)
    }

    /// Lists registered non-draft tools filtered by optional keyword (`q`), `category`, and `tag`.
    ///
    /// Delegates to [`ToolRegistry::list_filtered`] with `include_drafts = false`.
    pub fn list(
        &self,
        q: Option<&str>,
        category: Option<&str>,
        tag: Option<&str>,
    ) -> Vec<ToolEntry> {
        self.list_filtered(q, category, tag, false)
    }

    /// Lists registered tools filtered by optional keyword (`q`), `category`, `tag`, and draft visibility.
    ///
    /// When `include_drafts` is `false`, entries with `draft == true` are excluded.
    /// Results are deterministically sorted by `(category.to_lowercase(), name.to_lowercase(), id)`.
    pub fn list_filtered(
        &self,
        q: Option<&str>,
        category: Option<&str>,
        tag: Option<&str>,
        include_drafts: bool,
    ) -> Vec<ToolEntry> {
        let q_norm = q
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);
        let category_norm = category
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);
        let tag_norm = tag
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);

        let mut results: Vec<ToolEntry> = {
            let guard = self.entries.read().unwrap_or_else(|e| e.into_inner());
            guard
                .values()
                .filter(|entry| {
                    if !include_drafts && entry.draft {
                        return false;
                    }

                    if let Some(ref expected_cat) = category_norm
                        && entry.category.trim().to_lowercase() != *expected_cat
                    {
                        return false;
                    }

                    if let Some(ref expected_tag) = tag_norm
                        && !entry
                            .tags
                            .iter()
                            .any(|t| t.trim().to_lowercase() == *expected_tag)
                    {
                        return false;
                    }

                    if let Some(ref query) = q_norm {
                        let matches = entry.name.to_lowercase().contains(query)
                            || entry.description.to_lowercase().contains(query)
                            || entry.id.to_lowercase().contains(query)
                            || entry.tags.iter().any(|t| t.to_lowercase().contains(query));
                        if !matches {
                            return false;
                        }
                    }

                    true
                })
                .cloned()
                .collect()
        };

        results.sort_by(|a, b| {
            a.category
                .to_lowercase()
                .cmp(&b.category.to_lowercase())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.id.cmp(&b.id))
        });

        results
    }

    /// Returns a cloned [`ToolEntry`] by its `id`, if registered.
    pub fn get(&self, id: &str) -> Option<ToolEntry> {
        let guard = self.entries.read().unwrap_or_else(|e| e.into_inner());
        guard.get(id).cloned()
    }

    /// Inserts or replaces a validated [`ToolEntry`] in the in-memory index.
    pub fn upsert(&self, entry: ToolEntry) {
        let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
        guard.insert(entry.id.clone(), entry);
    }

    /// Removes a tool by `id` from the in-memory index, returning the previous entry if present.
    pub fn remove(&self, id: &str) -> Option<ToolEntry> {
        let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
        guard.remove(id)
    }

    /// Returns the shared async mutex serializing disk mutations (upload / delete) on `tools_dir`.
    pub fn write_lock(&self) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(&self.write_lock)
    }

    /// Starts a debounced filesystem watcher on `tools_dir` that hot-reloads the registry.
    pub fn start_watcher(self: &Arc<Self>) -> anyhow::Result<WatcherHandle> {
        fs::create_dir_all(&self.tools_dir).with_context(|| {
            format!(
                "failed to ensure tools directory '{}' before watching",
                self.tools_dir.display()
            )
        })?;

        let raw_root = self.tools_dir.clone();
        let canonical_root = fs::canonicalize(&self.tools_dir).unwrap_or_else(|_| raw_root.clone());

        let (tx, rx) = mpsc::channel::<WatchSignal>();
        let event_tx = tx.clone();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| match res {
            Ok(event) => {
                if is_relevant_event(&event, &raw_root, &canonical_root) {
                    let _ = event_tx.send(WatchSignal::Event);
                }
            }
            Err(err) => {
                warn!(error = %err, "filesystem watcher reported error");
            }
        })
        .context("failed to initialize filesystem watcher")?;

        watcher
            .watch(&self.tools_dir, RecursiveMode::Recursive)
            .with_context(|| {
                format!(
                    "failed to watch tools directory '{}'",
                    self.tools_dir.display()
                )
            })?;

        let registry = Arc::clone(self);
        let worker = thread::Builder::new()
            .name("toolstow-watcher".to_string())
            .spawn(move || {
                while let Ok(signal) = rx.recv() {
                    match signal {
                        WatchSignal::Shutdown => break,
                        WatchSignal::Event => {
                            // Debounce rapid bursts of filesystem notifications.
                            let mut shutting_down = false;
                            loop {
                                match rx.recv_timeout(WATCH_DEBOUNCE) {
                                    Ok(WatchSignal::Event) => continue,
                                    Ok(WatchSignal::Shutdown) => {
                                        shutting_down = true;
                                        break;
                                    }
                                    Err(RecvTimeoutError::Timeout) => break,
                                    Err(RecvTimeoutError::Disconnected) => {
                                        shutting_down = true;
                                        break;
                                    }
                                }
                            }

                            if shutting_down {
                                break;
                            }

                            match registry.scan_all() {
                                Ok(count) => {
                                    debug!(
                                        count,
                                        "reloaded tool registry after filesystem event"
                                    );
                                }
                                Err(err) => {
                                    warn!(
                                        error = %err,
                                        "failed to rescan tools directory after filesystem event"
                                    );
                                }
                            }
                        }
                    }
                }
            })
            .context("failed to spawn filesystem watcher thread")?;

        Ok(WatcherHandle {
            watcher: Some(watcher),
            shutdown_tx: Some(tx),
            worker: Some(worker),
        })
    }
}

/// Determines whether a `notify` event should trigger a debounced registry rescan.
///
/// Ignores read-only access events and changes strictly inside hidden top-level entries
/// (such as `tools/.staging` or `tools/.DS_Store`).
fn is_relevant_event(event: &Event, raw_root: &Path, canonical_root: &Path) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }

    if event.paths.is_empty() {
        return true;
    }

    event.paths.iter().any(|path| {
        let rel = path
            .strip_prefix(canonical_root)
            .or_else(|_| path.strip_prefix(raw_root));

        match rel {
            Ok(rel_path) => match rel_path.components().next() {
                None => true,
                Some(Component::Normal(first)) => first
                    .to_str()
                    .map(|name| !name.starts_with('.'))
                    .unwrap_or(true),
                Some(_) => true,
            },
            Err(_) => !path.components().any(|c| {
                matches!(c, Component::Normal(seg) if seg.to_str() == Some(".staging"))
            }),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use tempfile::TempDir;

    fn create_tool(tools_dir: &Path, dir_name: &str, manifest_json: &str, html: &str) {
        let dir = tools_dir.join(dir_name);
        fs::create_dir_all(&dir).expect("create tool dir");
        fs::write(dir.join("manifest.json"), manifest_json).expect("write manifest");
        fs::write(dir.join("index.html"), html).expect("write index.html");
    }

    #[test]
    fn scan_all_creates_missing_dir_and_skips_invalid_hidden_and_mismatched_entries() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");

        // Creating missing tools_dir succeeds with 0 tools.
        let registry = ToolRegistry::new(tools_dir.clone());
        assert_eq!(registry.scan_all().unwrap(), 0);
        assert!(tools_dir.is_dir());

        // Valid tool
        create_tool(
            &tools_dir,
            "alpha-tool",
            r#"{"id":"alpha-tool","name":"Alpha Tool","category":"Dev","tags":["cli","ops"]}"#,
            "<h1>Alpha</h1>",
        );

        // Hidden directory (.staging) with a valid manifest inside -> must be ignored
        create_tool(
            &tools_dir,
            ".staging",
            r#"{"id":"hidden-tool","name":"Hidden"}"#,
            "<h1>Hidden</h1>",
        );

        // Mismatched directory name vs manifest.id -> must be skipped
        create_tool(
            &tools_dir,
            "wrong-folder",
            r#"{"id":"right-id","name":"Mismatch"}"#,
            "<h1>Mismatch</h1>",
        );

        // Corrupt JSON manifest -> must be skipped without failing scan
        let corrupt_dir = tools_dir.join("corrupt-tool");
        fs::create_dir_all(&corrupt_dir).unwrap();
        fs::write(corrupt_dir.join("manifest.json"), "{broken json").unwrap();

        // Missing entry HTML -> must be skipped
        let no_entry_dir = tools_dir.join("no-entry");
        fs::create_dir_all(&no_entry_dir).unwrap();
        fs::write(
            no_entry_dir.join("manifest.json"),
            r#"{"id":"no-entry","name":"No Entry"}"#,
        )
        .unwrap();

        // Regular file at root -> must be ignored
        fs::write(tools_dir.join("README.txt"), "hello").unwrap();

        let count = registry.scan_all().unwrap();
        assert_eq!(count, 1);
        assert!(registry.get("alpha-tool").is_some());
        assert!(registry.get("hidden-tool").is_none());
        assert!(registry.get("right-id").is_none());
        assert!(registry.get("corrupt-tool").is_none());
        assert!(registry.get("no-entry").is_none());
    }

    #[test]
    fn list_filters_by_q_category_tag_and_sorts_deterministically() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");
        let registry = ToolRegistry::new(tools_dir.clone());

        create_tool(
            &tools_dir,
            "zeta-tool",
            r#"{"id":"zeta-tool","name":"Zeta Helper","description":"Container helper","category":"Ops","tags":["Docker","CLI"]}"#,
            "z",
        );
        create_tool(
            &tools_dir,
            "beta-tool",
            r#"{"id":"beta-tool","name":"Beta Tool","description":"SVG favicon maker","category":"design","tags":["svg","Icon"]}"#,
            "b",
        );
        create_tool(
            &tools_dir,
            "alpha-b",
            r#"{"id":"alpha-b","name":"Alpha Same","description":"Second alpha","category":"Design","tags":["svg"]}"#,
            "ab",
        );
        create_tool(
            &tools_dir,
            "alpha-a",
            r#"{"id":"alpha-a","name":"alpha same","description":"First alpha","category":"Design","tags":["svg","color"]}"#,
            "aa",
        );

        assert_eq!(registry.scan_all().unwrap(), 4);

        // Deterministic sort: category (case-insensitive), name (case-insensitive), id
        let all_ids: Vec<String> = registry
            .list(None, None, None)
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(all_ids, vec!["alpha-a", "alpha-b", "beta-tool", "zeta-tool"]);

        // Filter by category (trimmed, case-insensitive)
        let design_ids: Vec<String> = registry
            .list(None, Some("  DESIGN "), None)
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(design_ids, vec!["alpha-a", "alpha-b", "beta-tool"]);

        // Filter by tag (trimmed, case-insensitive)
        let docker_ids: Vec<String> = registry
            .list(None, None, Some(" docker "))
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(docker_ids, vec!["zeta-tool"]);

        // Keyword `q` matches name, description, id, and tags (case-insensitive)
        assert_eq!(
            registry
                .list(Some("FAVICON"), None, None)
                .into_iter()
                .map(|e| e.id)
                .collect::<Vec<_>>(),
            vec!["beta-tool"]
        );
        assert_eq!(
            registry
                .list(Some("alpha-b"), None, None)
                .into_iter()
                .map(|e| e.id)
                .collect::<Vec<_>>(),
            vec!["alpha-b"]
        );
        assert_eq!(
            registry
                .list(Some("COLOR"), None, None)
                .into_iter()
                .map(|e| e.id)
                .collect::<Vec<_>>(),
            vec!["alpha-a"]
        );

        // Combined filters + whitespace treated as None
        let combined: Vec<String> = registry
            .list(Some("alpha"), Some("design"), Some("svg"))
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(combined, vec!["alpha-a", "alpha-b"]);
        assert_eq!(registry.list(Some("   "), Some(""), Some("  ")).len(), 4);
    }

    #[test]
    fn upsert_get_and_remove_update_in_memory_index_immediately() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");
        create_tool(
            &tools_dir,
            "quick-tool",
            r#"{"id":"quick-tool","name":"Quick v1"}"#,
            "<h1>v1</h1>",
        );

        let entry = manifest::load_and_validate_manifest(&tools_dir.join("quick-tool"), None).unwrap();
        let registry = ToolRegistry::new(tools_dir);

        assert!(registry.get("quick-tool").is_none());
        registry.upsert(entry.clone());
        assert_eq!(registry.get("quick-tool").unwrap().name, "Quick v1");

        let mut updated = entry;
        updated.name = "Quick v2".to_string();
        registry.upsert(updated);
        assert_eq!(registry.get("quick-tool").unwrap().name, "Quick v2");

        let removed = registry.remove("quick-tool").unwrap();
        assert_eq!(removed.name, "Quick v2");
        assert!(registry.get("quick-tool").is_none());
        assert!(registry.remove("quick-tool").is_none());
    }

    #[test]
    fn start_watcher_hot_reloads_on_add_and_delete() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");
        let registry = Arc::new(ToolRegistry::new(tools_dir.clone()));
        assert_eq!(registry.scan_all().unwrap(), 0);

        let watcher = registry.start_watcher().expect("start watcher");

        // Give OS watcher a short moment to register the directory before mutating it.
        thread::sleep(Duration::from_millis(100));

        // Hot-add a tool directly on disk.
        create_tool(
            &tools_dir,
            "hot-tool",
            r#"{"id":"hot-tool","name":"Hot Added Tool"}"#,
            "<h1>Hot</h1>",
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if registry.get("hot-tool").is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(
            registry.get("hot-tool").is_some(),
            "watcher should have indexed hot-added tool within timeout"
        );

        // Hot-delete the tool directly on disk.
        fs::remove_dir_all(tools_dir.join("hot-tool")).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if registry.get("hot-tool").is_none() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(
            registry.get("hot-tool").is_none(),
            "watcher should have removed hot-deleted tool within timeout"
        );

        // Dropping watcher handle must terminate cleanly without hanging.
        drop(watcher);
    }

    #[test]
    fn is_relevant_event_ignores_staging_hidden_and_access_events() {
        use notify::event::{AccessKind, CreateKind, ModifyKind};

        let root = PathBuf::from("/tmp/toolstow/tools");
        let canonical = PathBuf::from("/private/tmp/toolstow/tools");

        let staging_event = Event {
            kind: EventKind::Create(CreateKind::File),
            paths: vec![canonical.join(".staging/stage-123/index.html")],
            attrs: Default::default(),
        };
        assert!(!is_relevant_event(&staging_event, &root, &canonical));

        let ds_store_event = Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![root.join(".DS_Store")],
            attrs: Default::default(),
        };
        assert!(!is_relevant_event(&ds_store_event, &root, &canonical));

        let access_event = Event {
            kind: EventKind::Access(AccessKind::Read),
            paths: vec![canonical.join("my-tool/index.html")],
            attrs: Default::default(),
        };
        assert!(!is_relevant_event(&access_event, &root, &canonical));

        let tool_event = Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![canonical.join("my-tool/manifest.json")],
            attrs: Default::default(),
        };
        assert!(is_relevant_event(&tool_event, &root, &canonical));
    }

    #[test]
    fn list_and_list_filtered_handle_draft_visibility() {
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");
        let registry = ToolRegistry::new(tools_dir.clone());

        create_tool(
            &tools_dir,
            "public-tool",
            r#"{"id":"public-tool","name":"Public Tool","category":"Dev","tags":["util"]}"#,
            "<h1>Public</h1>",
        );
        create_tool(
            &tools_dir,
            "draft-tool",
            r#"{"id":"draft-tool","name":"Draft Tool","category":"Dev","tags":["util"],"draft":true}"#,
            "<h1>Draft</h1>",
        );

        assert_eq!(registry.scan_all().unwrap(), 2);

        // `get` returns both published and draft entries
        assert!(registry.get("public-tool").is_some());
        assert!(registry.get("draft-tool").is_some_and(|e| e.draft));

        // `list` and `list_filtered(..., false)` exclude drafts
        let public_ids: Vec<String> = registry
            .list(None, None, None)
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(public_ids, vec!["public-tool"]);

        let filtered_no_drafts: Vec<String> = registry
            .list_filtered(Some("tool"), Some("dev"), Some("util"), false)
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(filtered_no_drafts, vec!["public-tool"]);

        // `list_filtered(..., true)` includes drafts in deterministic sort order
        let filtered_with_drafts: Vec<String> = registry
            .list_filtered(Some("tool"), Some("dev"), Some("util"), true)
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(filtered_with_drafts, vec!["draft-tool", "public-tool"]);
    }
}
