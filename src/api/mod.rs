pub mod handlers;

use axum::{
    http::StatusCode,
    routing::{get, post},
    Router,
};
use std::sync::Arc;

use crate::browser::BrowserPool;
use handlers::{health_handler, turnstile_handler, iuam_handler, shutdown_handler};

pub fn create_router(pool: Arc<BrowserPool>) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/turnstile", post(turnstile_handler))
        .route("/iuam", post(iuam_handler))
        .route("/shutdown", post(shutdown_handler))
        .with_state(pool)
}
