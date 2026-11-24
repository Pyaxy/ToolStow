//! ToolStow binary entrypoint.

use std::fs;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

use toolstow::config::Config;
use toolstow::registry::ToolRegistry;
use toolstow::routes::{AppState, build_router};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("toolstow=info,tower_http=info")),
        )
        .init();

    fs::create_dir_all(&config.tools_dir).with_context(|| {
        format!(
            "failed to create tools directory '{}'",
            config.tools_dir.display()
        )
    })?;

    let registry = Arc::new(ToolRegistry::new(config.tools_dir.clone()));
    let loaded_count = registry
        .scan_all()
        .context("failed to perform initial scan of tools directory")?;
    info!(
        tools_dir = %config.tools_dir.display(),
        count = loaded_count,
        "initial tool scan completed"
    );

    let _watcher = registry
        .start_watcher()
        .context("failed to start tools directory watcher")?;

    let listen_addr = config.listen_addr();
    let state = AppState {
        registry,
        config: Arc::new(config),
    };
    let app = build_router(state);

    let listener = TcpListener::bind(&listen_addr)
        .await
        .with_context(|| format!("failed to bind TCP listener on {listen_addr}"))?;
    let local_addr = listener
        .local_addr()
        .context("failed to read bound listener address")?;

    info!(url = %format!("http://{local_addr}"), "ToolStow server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server exited with error")?;

    info!("ToolStow server shut down gracefully");
    Ok(())
}

/// Resolves when a shutdown signal (`Ctrl+C`, or `SIGTERM` on Unix) is received.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %err, "failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => {
                tracing::error!(error = %err, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received");
}
