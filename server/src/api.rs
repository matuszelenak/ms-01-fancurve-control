//! HTTP API and static frontend serving.

use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rust_embed::RustEmbed;

use crate::config::Config;
use crate::control::SharedState;

#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct Assets;

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/config", get(get_config).put(put_config))
        .fallback(static_handler)
        .with_state(state)
}

async fn get_status(State(state): State<SharedState>) -> Response {
    Json(state.status.read().await.clone()).into_response()
}

async fn get_config(State(state): State<SharedState>) -> Response {
    Json(state.config.read().await.clone()).into_response()
}

async fn put_config(
    State(state): State<SharedState>,
    Json(new_config): Json<Config>,
) -> Response {
    if let Err(e) = new_config.validate() {
        return (StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}")).into_response();
    }
    let path = Config::path();
    if let Err(e) = new_config.save(&path) {
        tracing::error!("saving config: {e:#}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save config: {e:#}"),
        )
            .into_response();
    }
    *state.config.write().await = new_config.clone();
    tracing::info!("config updated and saved to {}", path.display());
    Json(new_config).into_response()
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], file.data).into_response()
        }
        // SPA fallback: unknown non-API paths serve the app shell.
        None if !path.starts_with("api/") => match Assets::get("index.html") {
            Some(file) => (
                [(header::CONTENT_TYPE, "text/html")],
                file.data,
            )
                .into_response(),
            None => (StatusCode::NOT_FOUND, "frontend not built").into_response(),
        },
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
