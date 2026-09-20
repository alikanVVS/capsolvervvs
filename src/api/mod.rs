pub mod handlers;

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use crate::browser::BrowserPool;
use crate::config::Config;
use handlers::{health_handler, iuam_handler, turnstile_handler, AppState};

pub use handlers::AppState as ApiState;

pub fn create_router(pool: Arc<BrowserPool>, config: Arc<Config>) -> Router {
    let state = AppState { pool, config };

    Router::new()
        .route("/health", get(health_handler))
        .route("/turnstile", post(turnstile_handler))
        .route("/iuam", post(iuam_handler))
        .layer(TraceLayer::new_for_http())
        // Solve requests are small JSON documents; anything larger is not ours to buffer.
        .layer(RequestBodyLimitLayer::new(64 * 1024))
        .with_state(state)
}
