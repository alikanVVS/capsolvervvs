use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::browser::{BrowserPool, CapacityStats};

#[derive(Serialize, Deserialize, Debug)]
pub struct HealthResponse {
    pub status: String,
    pub timestamp: String,
    pub capacity: CapacityStats,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ErrorResponse {
    pub error: String,
    pub code: u16,
}

pub struct AppState {
    pub pool: Arc<BrowserPool>,
}

pub async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let capacity = state.pool.get_capacity_stats().await;
    let timestamp = chrono::Utc::now().to_rfc3339();

    Json(HealthResponse {
        status: "healthy".to_string(),
        timestamp,
        capacity,
    })
}

pub async fn shutdown_handler(State(state): State<Arc<AppState>>) -> StatusCode {
    let _ = state.pool.shutdown().await;
    StatusCode::OK
}

pub fn create_router(pool: Arc<BrowserPool>) -> Router {
    let state = Arc::new(AppState { pool });

    Router::new()
        .route("/health", get(health_handler))
        .route("/shutdown", post(shutdown_handler))
        .with_state(state)
}

use axum::routing::post;
