use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use crate::browser::BrowserPool;
use crate::solvers::{TurnstileSolver, IuamSolver, TurnstileParams, IuamParams};

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
    pub token: Option<String>,
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
    pub cf_clearance: Option<String>,
    pub user_agent: Option<String>,
    pub ip: Option<String>,
    pub cookies: Option<serde_json::Value>,
    pub error: Option<String>,
    pub elapsed_ms: u128,
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub status: String,
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

pub async fn health_handler(State(pool): State<Arc<BrowserPool>>) -> Json<HealthResponse> {
    let capacity = pool.get_capacity_stats().await;
    let timestamp = chrono::Utc::now().to_rfc3339();

    Json(HealthResponse {
        status: "healthy".to_string(),
        timestamp,
        capacity: CapacityInfo {
            total: capacity.total,
            available: capacity.available,
            active: capacity.active,
            processes: capacity.processes,
        },
    })
}

pub async fn turnstile_handler(
    State(pool): State<Arc<BrowserPool>>,
    Json(req): Json<TurnstileRequest>,
) -> (StatusCode, Json<TurnstileResponse>) {
    let start = Instant::now();
    let timestamp = chrono::Utc::now().to_rfc3339();

    let mut context = match pool.acquire().await {
        Ok(ctx) => ctx,
        Err(e) => {
            let elapsed = start.elapsed().as_millis();
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(TurnstileResponse {
                    status: "failed".to_string(),
                    token: None,
                    error: Some(format!("Pool error: {}", e)),
                    elapsed_ms: elapsed,
                    timestamp,
                }),
            );
        }
    };

    let result = pool
        .new_page(&mut context, &req.url, req.proxy.clone())
        .await;

    if let Err(e) = result {
        let elapsed = start.elapsed().as_millis();
        pool.release(context).await.ok();
        return (
            StatusCode::BAD_REQUEST,
            Json(TurnstileResponse {
                status: "failed".to_string(),
                token: None,
                error: Some(format!("Navigation failed: {}", e)),
                elapsed_ms: elapsed,
                timestamp,
            }),
        );
    }

    let solver = TurnstileSolver::new();
    let params = TurnstileParams {
        url: req.url.clone(),
        sitekey: req.sitekey.clone(),
        cdata: req.cdata.clone(),
        action: req.action.clone(),
    };

    match solver.solve_with_context(&mut context, params).await {
        Ok(token) => {
            let elapsed = start.elapsed().as_millis();
            pool.release(context).await.ok();

            (
                StatusCode::OK,
                Json(TurnstileResponse {
                    status: "success".to_string(),
                    token: Some(token),
                    error: None,
                    elapsed_ms: elapsed,
                    timestamp,
                }),
            )
        }
        Err(e) => {
            let elapsed = start.elapsed().as_millis();
            pool.release(context).await.ok();

            (
                StatusCode::BAD_REQUEST,
                Json(TurnstileResponse {
                    status: "failed".to_string(),
                    token: None,
                    error: Some(e.to_string()),
                    elapsed_ms: elapsed,
                    timestamp,
                }),
            )
        }
    }
}

pub async fn iuam_handler(
    State(pool): State<Arc<BrowserPool>>,
    Json(req): Json<IuamRequest>,
) -> (StatusCode, Json<IuamResponse>) {
    let start = Instant::now();
    let timestamp = chrono::Utc::now().to_rfc3339();

    let mut context = match pool.acquire().await {
        Ok(ctx) => ctx,
        Err(e) => {
            let elapsed = start.elapsed().as_millis();
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(IuamResponse {
                    status: "failed".to_string(),
                    cf_clearance: None,
                    user_agent: None,
                    ip: None,
                    cookies: None,
                    error: Some(format!("Pool error: {}", e)),
                    elapsed_ms: elapsed,
                    timestamp,
                }),
            );
        }
    };

    let result = pool
        .new_page(&mut context, &req.url, req.proxy.clone())
        .await;

    if let Err(e) = result {
        let elapsed = start.elapsed().as_millis();
        pool.release(context).await.ok();
        return (
            StatusCode::BAD_REQUEST,
            Json(IuamResponse {
                status: "failed".to_string(),
                cf_clearance: None,
                user_agent: None,
                ip: None,
                cookies: None,
                error: Some(format!("Navigation failed: {}", e)),
                elapsed_ms: elapsed,
                timestamp,
            }),
        );
    }

    let solver = IuamSolver::new();
    let params = IuamParams {
        url: req.url.clone(),
        proxy: req.proxy.clone(),
    };

    match solver.solve_with_params(&mut context, params).await {
        Ok(result) => {
            let elapsed = start.elapsed().as_millis();
            pool.release(context).await.ok();

            let cookies = serde_json::to_value(&result.cookies).ok();

            (
                StatusCode::OK,
                Json(IuamResponse {
                    status: "success".to_string(),
                    cf_clearance: Some(result.cf_clearance),
                    user_agent: Some(result.user_agent),
                    ip: Some(result.ip),
                    cookies,
                    error: None,
                    elapsed_ms: elapsed,
                    timestamp,
                }),
            )
        }
        Err(e) => {
            let elapsed = start.elapsed().as_millis();
            pool.release(context).await.ok();

            (
                StatusCode::BAD_REQUEST,
                Json(IuamResponse {
                    status: "failed".to_string(),
                    cf_clearance: None,
                    user_agent: None,
                    ip: None,
                    cookies: None,
                    error: Some(e.to_string()),
                    elapsed_ms: elapsed,
                    timestamp,
                }),
            )
        }
    }
}

pub async fn shutdown_handler(State(pool): State<Arc<BrowserPool>>) -> StatusCode {
    let _ = pool.shutdown().await;
    StatusCode::OK
}
