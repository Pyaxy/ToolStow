//! ToolStow binary entrypoint.

use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

use toolstow::auth::AuthManager;
use toolstow::config::Config;
use toolstow::registry::ToolRegistry;
use toolstow::routes::{AppState, build_router, is_custom_theme_active};
use toolstow::settings::SettingsStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("toolstow=info,tower_http=info")),
        )
        .init();

    let config = Config::from_cli_and_env();

    fs::create_dir_all(&config.data_dir).with_context(|| {
        format!(
            "failed to create data directory '{}'",
            config.data_dir.display()
        )
    })?;
    fs::create_dir_all(&config.tools_dir).with_context(|| {
        format!(
            "failed to create tools directory '{}'",
            config.tools_dir.display()
        )
    })?;

    let settings = SettingsStore::load(config.settings_path());
    let auth = AuthManager::new(
        config.admin_key.clone(),
        config.api_key.clone(),
        config.cookie_secure,
    );
    auth.warn_weak_keys();

    let registry = Arc::new(ToolRegistry::new(config.tools_dir.clone()));
    let loaded_count = registry
        .scan_all()
        .context("failed to perform initial scan of tools directory")?;
    let _watcher = registry
        .start_watcher()
        .context("failed to start tools directory watcher")?;

    let listen_addr = config.listen_addr();
    let theme_mode = if is_custom_theme_active(&config.theme_dir) {
        "custom"
    } else {
        "embedded"
    };
    let admin_enabled = auth.is_admin_enabled();
    let api_enabled = auth.is_api_enabled();

    let config = Arc::new(config);
    let state = AppState {
        registry,
        config: Arc::clone(&config),
        settings,
        auth,
    };
    let app = build_router(state);

    let listener = TcpListener::bind(&listen_addr)
        .await
        .with_context(|| format!("failed to bind TCP listener on {listen_addr}"))?;
    let local_addr = listener
        .local_addr()
        .context("failed to read bound listener address")?;

    info!(
        url = %format!("http://{local_addr}"),
        data_dir = %config.data_dir.display(),
        tools_dir = %config.tools_dir.display(),
        theme_dir = %config.theme_dir.display(),
        theme_mode,
        tools_loaded = loaded_count,
        admin_panel_enabled = admin_enabled,
        ai_bearer_api_enabled = api_enabled,
        include_drafts = config.include_drafts,
        cookie_secure = config.cookie_secure,
        max_upload_mb = config.max_upload_mb,
        "ToolStow server listening"
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
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
