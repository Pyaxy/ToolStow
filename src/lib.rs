//! ToolStow core library.
//!
//! Provides configuration parsing, structured HTTP error responses, manifest
//! loading and validation, tool registry management, archive installation, and
//! HTTP routing.

pub mod archive;
pub mod config;
pub mod error;
pub mod manifest;
pub mod registry;
pub mod routes;
