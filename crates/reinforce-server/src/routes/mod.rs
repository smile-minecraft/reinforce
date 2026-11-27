use axum::Json;
use axum::Router;
use axum::routing::get;
use serde_json::json;
use tower_http::trace::TraceLayer;

use crate::config::AuthMode;
use crate::embed;
use crate::error::ApiError;
use crate::state::AppState;

mod auth;
mod content;
mod export;
mod plugins;
mod proposals;
mod study;
mod sync;

pub fn router() -> Router<AppState> {
    let api = Router::new()
        .route("/meta", get(meta))
        .merge(auth::router())
        .merge(content::router())
        .merge(export::router())
        .merge(plugins::router())
        .merge(proposals::router())
        .merge(study::router())
        .merge(sync::router())
        .fallback(api_not_found);

    Router::new()
        .nest("/api", api)
        .fallback(static_fallback)
        .layer(TraceLayer::new_for_http())
}

async fn meta(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<serde_json::Value> {
    let mode: AuthMode = state.auth_mode;
    Json(json!({
        "version": reinforce_core::VERSION,
        "auth_mode": mode.as_str(),
    }))
}

async fn api_not_found() -> ApiError {
    ApiError::not_found("endpoint")
}

async fn static_fallback(
    axum::extract::State(_state): axum::extract::State<AppState>,
    uri: axum::http::Uri,
) -> axum::response::Response {
    embed::serve_spa(uri.path())
}
