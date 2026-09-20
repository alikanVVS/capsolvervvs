use super::error::{Result, SolverError};
use super::utils::{origin_of, validate_url};
use crate::browser::{BrowserContext, CdpSession};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

pub struct IuamSolver {
    pub timeout_duration: Duration,
    pub poll_interval: Duration,
}

#[derive(Debug, Clone)]
pub struct IuamResult {
    pub cf_clearance: String,
    pub user_agent: String,
    pub cookies: HashMap<String, String>,
    pub ip: String,
}

#[derive(Debug, Clone)]
pub struct IuamParams {
    pub url: String,
    pub proxy: Option<String>,
}

impl IuamSolver {
    pub fn new() -> Self {
        IuamSolver {
            timeout_duration: Duration::from_secs(29),
            poll_interval: Duration::from_millis(500),
        }
    }

    pub fn with_timeout(mut self, duration: Duration) -> Self {
        self.timeout_duration = duration;
        self
    }

    pub fn with_poll_interval(mut self, duration: Duration) -> Self {
        self.poll_interval = duration;
        self
    }

    pub async fn solve(&self, context: &mut BrowserContext, url: &str) -> Result<IuamResult> {
        validate_url(url)?;

        let session = context
            .cdp_session
            .as_ref()
            .ok_or_else(|| SolverError::CdpError("No CDP session in context".to_string()))?
            .clone();

        session
            .navigate(url)
            .await
            .map_err(|e| SolverError::NavigationFailed(e.to_string()))?;

        session
            .wait_for_navigation(self.timeout_duration)
            .await
            .map_err(|e| SolverError::NavigationFailed(e.to_string()))?;

        self.wait_for_cf_clearance(&session, url).await
    }

    /// The proxy is bound when the browser context is created, so it is applied by
    /// `BrowserPool::new_page` before this runs.
    pub async fn solve_with_params(
        &self,
        context: &mut BrowserContext,
        params: IuamParams,
    ) -> Result<IuamResult> {
        self.solve(context, &params.url).await
    }

    async fn wait_for_cf_clearance(
        &self,
        session: &Arc<CdpSession>,
        url: &str,
    ) -> Result<IuamResult> {
        let origin = origin_of(url)?;
        let deadline = tokio::time::Instant::now() + self.timeout_duration;

        loop {
            let cookies = session
                .get_cookies_for(&[url, &origin])
                .await
                .map_err(|e| SolverError::CdpError(e.to_string()))?;

            let mut cookie_map = HashMap::new();
            let mut cf_clearance = None;

            for cookie in cookies {
                let (Some(name), Some(value)) = (
                    cookie.get("name").and_then(Value::as_str),
                    cookie.get("value").and_then(Value::as_str),
                ) else {
                    continue;
                };

                if name == "cf_clearance" && !value.is_empty() {
                    cf_clearance = Some(value.to_string());
                }
                cookie_map.insert(name.to_string(), value.to_string());
            }

            if let Some(clearance) = cf_clearance {
                return Ok(IuamResult {
                    cf_clearance: clearance,
                    user_agent: session
                        .user_agent()
                        .await
                        .map_err(|e| SolverError::CdpError(e.to_string()))?,
                    cookies: cookie_map,
                    ip: self.get_ip_address(session).await,
                });
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(SolverError::Timeout);
            }

            sleep(self.poll_interval).await;
        }
    }

    /// Runs inside the page so the address reflects the context's proxy, not the host's.
    /// Never fails the solve: the clearance cookie is the actual deliverable.
    async fn get_ip_address(&self, session: &Arc<CdpSession>) -> String {
        // `mode: 'no-cors'` would make the response opaque and unreadable; ipify sends CORS headers.
        let script = r#"
            (async () => {
                try {
                    const response = await fetch('https://api.ipify.org?format=json');
                    if (!response.ok) return 'unknown';
                    const data = await response.json();
                    return data.ip || 'unknown';
                } catch (e) {
                    return 'unknown';
                }
            })()
        "#;

        match session.evaluate_script(script).await {
            Ok(response) => response
                .get("result")
                .and_then(|r| r.get("value"))
                .and_then(Value::as_str)
                .filter(|ip| !ip.is_empty())
                .unwrap_or("unknown")
                .to_string(),
            Err(e) => {
                tracing::warn!(error = %e, "IP lookup failed");
                "unknown".to_string()
            }
        }
    }
}

impl Default for IuamSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iuam_solver_creation() {
        let solver = IuamSolver::new();
        assert_eq!(solver.timeout_duration, Duration::from_secs(29));
        assert_eq!(solver.poll_interval, Duration::from_millis(500));
    }

    #[test]
    fn test_iuam_solver_with_timeout() {
        let solver = IuamSolver::new().with_timeout(Duration::from_secs(60));
        assert_eq!(solver.timeout_duration, Duration::from_secs(60));
    }

    #[test]
    fn test_iuam_solver_with_poll_interval() {
        let solver = IuamSolver::new().with_poll_interval(Duration::from_millis(1000));
        assert_eq!(solver.poll_interval, Duration::from_millis(1000));
    }

    #[test]
    fn test_iuam_solver_chained_config() {
        let solver = IuamSolver::new()
            .with_timeout(Duration::from_secs(90))
            .with_poll_interval(Duration::from_millis(1000));

        assert_eq!(solver.timeout_duration, Duration::from_secs(90));
        assert_eq!(solver.poll_interval, Duration::from_millis(1000));
    }

    #[test]
    fn test_iuam_result_creation() {
        let mut cookies = HashMap::new();
        cookies.insert("test".to_string(), "value".to_string());

        let result = IuamResult {
            cf_clearance: "test_token".to_string(),
            user_agent: "Mozilla/5.0".to_string(),
            cookies,
            ip: "192.168.1.1".to_string(),
        };

        assert_eq!(result.cf_clearance, "test_token");
        assert_eq!(result.user_agent, "Mozilla/5.0");
        assert_eq!(result.ip, "192.168.1.1");
        assert_eq!(result.cookies.len(), 1);
    }

    #[test]
    fn test_iuam_params_creation() {
        let params = IuamParams {
            url: "https://example.com".to_string(),
            proxy: Some("http://proxy:8080".to_string()),
        };

        assert_eq!(params.url, "https://example.com");
        assert!(params.proxy.is_some());
    }

    #[test]
    fn test_iuam_solver_default() {
        let solver = IuamSolver::default();
        assert_eq!(solver.timeout_duration, Duration::from_secs(29));
    }
}
