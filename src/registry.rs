//! In-memory tool registry, directory scanning, and filesystem watcher.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::manifest::ToolEntry;

/// Handle keeping the background filesystem watcher alive until dropped.
#[derive(Debug)]
pub struct WatcherHandle {
    #[allow(dead_code)]
    _private: (),
}

/// Thread-safe in-memory registry of validated tools backed by a `tools/` directory.
#[derive(Debug)]
pub struct ToolRegistry {
    tools_dir: PathBuf,
    #[allow(dead_code)]
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
    #[allow(unused_variables)]
    pub fn scan_all(&self) -> anyhow::Result<usize> {
        todo!("implemented by storage module")
    }

    /// Lists registered tools filtered by optional keyword (`q`), `category`, and `tag`.
    #[allow(unused_variables)]
    pub fn list(
        &self,
        q: Option<&str>,
        category: Option<&str>,
        tag: Option<&str>,
    ) -> Vec<ToolEntry> {
        todo!("implemented by storage module")
    }

    /// Returns a cloned [`ToolEntry`] by its `id`, if registered.
    #[allow(unused_variables)]
    pub fn get(&self, id: &str) -> Option<ToolEntry> {
        todo!("implemented by storage module")
    }

    /// Inserts or replaces a validated [`ToolEntry`] in the in-memory index.
    #[allow(unused_variables)]
    pub fn upsert(&self, entry: ToolEntry) {
        todo!("implemented by storage module")
    }

    /// Removes a tool by `id` from the in-memory index, returning the previous entry if present.
    #[allow(unused_variables)]
    pub fn remove(&self, id: &str) -> Option<ToolEntry> {
        todo!("implemented by storage module")
    }

    /// Returns the shared async mutex serializing disk mutations (upload / delete) on `tools_dir`.
    pub fn write_lock(&self) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(&self.write_lock)
    }

    /// Starts a debounced filesystem watcher on `tools_dir` that hot-reloads the registry.
    pub fn start_watcher(self: &Arc<Self>) -> anyhow::Result<WatcherHandle> {
        todo!("implemented by storage module")
    }
}
