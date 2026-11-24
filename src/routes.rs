//! Axum HTTP router, API endpoints, static tool hosting, and embedded frontend serving.

use std::sync::Arc;

use axum::Router;

use crate::config::Config;
use crate::registry::ToolRegistry;

/// Shared application state injected into Axum route handlers.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Thread-safe in-memory tool registry.
    pub registry: Arc<ToolRegistry>,
    /// Parsed runtime server configuration.
    pub config: Arc<Config>,
}

/// Builds the full ToolStow Axum [`Router`].
#[allow(unused_variables)]
pub fn build_router(state: AppState) -> Router {
    todo!("implemented by http module")
}
