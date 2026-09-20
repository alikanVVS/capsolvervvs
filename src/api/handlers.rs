use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::browser::{BrowserContext, BrowserPool};
use crate::config::Config;
use crate::solvers::{IuamSolver, TurnstileParams, TurnstileSolver};

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<BrowserPool>,
    pub config: Arc<Config>,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub timestamp: String,
    pub capacity: CapacityInfo,
}

#[derive(Debug, Serialize)]
pub struct CapacityInfo {
    pub total: usize,
    pub available: usize,
    pub active: usize,
    pub processes: usize,
}

#[derive(Debug, Deserialize)]
pub struct TurnstileRequest {
    pub url: String,
    pub sitekey: String,
    #[serde(default)]
    pub cdata: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TurnstileResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub elapsed_ms: u128,
    pub timestamp: String,
}

#[derive(Debug, Deserialize)]
pub struct IuamRequest {
    pub url: String,
    #[serde(default)]
    pub proxy: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IuamResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cf_clearance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cookies: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub elapsed_ms: u128,
    pub timestamp: String,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Returning a slot to the pool must never mask the solve result.
async fn release(pool: &BrowserPool, context: BrowserContext) {
    if let Err(e) = pool.release(context).await {
        tracing::error!(error = %e, "failed to release browser context");
    }
}

pub async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let capacity = state.pool.get_capacity_stats().await;

    Json(HealthResponse {
        status: "healthy".to_string(),
        timestamp: now(),
        capacity: CapacityInfo {
            total: capacity.total,
            available: capacity.available,
            active: capacity.active,
            processes: capacity.processes,
        },
    })
}

pub async fn turnstile_handler(
    State(state): State<AppState>,
    Json(request): Json<TurnstileRequest>,
) -> (StatusCode, Json<TurnstileResponse>) {
    let start = Instant::now();

    let failure = |status: StatusCode, message: String| {
        (
            status,
            Json(TurnstileResponse {
                status: "failed".to_string(),
                token: None,
                error: Some(message),
                elapsed_ms: start.elapsed().as_millis(),
                timestamp: now(),
            }),
        )
    };

    let mut context = match state.pool.acquire().await {
        Ok(context) => context,
        Err(e) => return failure(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
    };

    // The solver serves its stub by intercepting the document request, so the page
    // only needs a blank starting point here.
    if let Err(e) = state
        .pool
        .new_page(&mut context, "about:blank", request.proxy.clone())
        .await
    {
        release(&state.pool, context).await;
        return failure(StatusCode::SERVICE_UNAVAILABLE, e.to_string());
    }

    let solver = TurnstileSolver::new().with_timeout(state.config.solve_timeout_duration());
    let params = TurnstileParams {
        url: request.url,
        sitekey: request.sitekey,
        cdata: request.cdata,
        action: request.action,
    };

    let result = solver.solve_with_context(&mut context, params).await;
    release(&state.pool, context).await;

    match result {
        Ok(token) => (
            StatusCode::OK,
            Json(TurnstileResponse {
                status: "success".to_string(),
                token: Some(token),
                error: None,
                elapsed_ms: start.elapsed().as_millis(),
                timestamp: now(),
            }),
        ),
        Err(e) => failure(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

pub async fn iuam_handler(
    State(state): State<AppState>,
    Json(request): Json<IuamRequest>,
) -> (StatusCode, Json<IuamResponse>) {
    let start = Instant::now();

    let failure = |status: StatusCode, message: String| {
        (
            status,
            Json(IuamResponse {
                status: "failed".to_string(),
                cf_clearance: None,
                user_agent: None,
                ip: None,
                cookies: None,
                error: Some(message),
                elapsed_ms: start.elapsed().as_millis(),
                timestamp: now(),
            }),
        )
    };

    let mut context = match state.pool.acquire().await {
        Ok(context) => context,
        Err(e) => return failure(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
    };

    if let Err(e) = state
        .pool
        .new_page(&mut context, "about:blank", request.proxy.clone())
        .await
    {
        release(&state.pool, context).await;
        return failure(StatusCode::SERVICE_UNAVAILABLE, e.to_string());
    }

    let solver = IuamSolver::new().with_timeout(state.config.solve_timeout_duration());
    let result = solver.solve(&mut context, &request.url).await;
    release(&state.pool, context).await;

    match result {
        Ok(result) => (
            StatusCode::OK,
            Json(IuamResponse {
                status: "success".to_string(),
                cf_clearance: Some(result.cf_clearance),
                user_agent: Some(result.user_agent),
                ip: Some(result.ip),
                cookies: Some(result.cookies),
                error: None,
                elapsed_ms: start.elapsed().as_millis(),
                timestamp: now(),
            }),
        ),
        Err(e) => failure(StatusCode::BAD_REQUEST, e.to_string()),
    }
}
