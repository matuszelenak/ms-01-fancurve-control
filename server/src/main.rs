mod api;
mod config;
mod control;
mod hwmon;

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::RwLock;

use crate::config::Config;
use crate::control::{AppState, Status};
use crate::hwmon::Hardware;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fancurve=info".into()),
        )
        .init();

    let config_path = Config::path();
    let snapshot_path = config_path.with_file_name("chip-snapshot.json");
    let hw = Hardware::discover(&snapshot_path)?;

    let config = Config::load_or_default(&config_path)?;
    tracing::info!("config loaded from {}", config_path.display());

    let state = Arc::new(AppState {
        hw,
        config: RwLock::new(config),
        status: RwLock::new(Status::default()),
    });

    // Control loop.
    tokio::spawn(control::run(state.clone()));

    // HTTP server.
    let listen = std::env::var("FANCURVE_LISTEN").unwrap_or_else(|_| "0.0.0.0:8090".into());
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .with_context(|| format!("binding {listen}"))?;
    tracing::info!("listening on http://{listen}");

    axum::serve(listener, api::router(state.clone()))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // On exit, hand the fans back to the hardware's automatic control.
    state.hw.restore_auto();
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received, restoring automatic fan control");
}
