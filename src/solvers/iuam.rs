use super::error::{Result, SolverError};
use super::utils::validate_url;
use crate::browser::{BrowserContext, CdpSession};
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
            timeout_duration: Duration::from_secs(120),
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

    pub async fn solve(
        &self,
        context: &mut BrowserContext,
        url: &str,
        proxy: Option<&str>,
    ) -> Result<IuamResult> {
        validate_url(url)?;

        let session = self.setup_page(context, url, proxy).await?;

        self.wait_for_cf_clearance(&session, url).await
    }

    pub async fn solve_with_params(
        &self,
        context: &mut BrowserContext,
        params: IuamParams,
    ) -> Result<IuamResult> {
        self.solve(context, &params.url, params.proxy.as_deref()).await
    }

    async fn setup_page(
        &self,
        context: &mut BrowserContext,
        url: &str,
        _proxy: Option<&str>,
    ) -> Result<Arc<CdpSession>> {
        if context.cdp_session.is_none() {
            return Err(SolverError::CdpError(
                "No CDP session in context".to_string(),
            ));
        }

        let session = context
            .cdp_session
            .as_ref()
            .ok_or_else(|| SolverError::CdpError("CDP session lost".to_string()))?
            .clone();

        session
            .navigate(url)
            .await
            .map_err(|e| SolverError::NavigationFailed(e.to_string()))?;

        session
            .wait_for_navigation(self.timeout_duration.as_secs())
            .await
            .map_err(|e| SolverError::NavigationFailed(e.to_string()))?;

        Ok(session)
    }

    async fn wait_for_cf_clearance(
        &self,
        session: &Arc<CdpSession>,
        url: &str,
    ) -> Result<IuamResult> {
        let mut elapsed = Duration::ZERO;

        loop {
            if elapsed >= self.timeout_duration {
                return Err(SolverError::Timeout);
            }

            let cookies = session
                .get_cookies()
                .await
                .map_err(|e| SolverError::CdpError(e.to_string()))?;

            let mut cookie_map = HashMap::new();
            let mut cf_clearance = None;

            for cookie in cookies {
                if let Some(name) = cookie.get("name").and_then(|n| n.as_str()) {
                    if let Some(value) = cookie.get("value").and_then(|v| v.as_str()) {
                        cookie_map.insert(name.to_string(), value.to_string());

                        if name == "cf_clearance" {
                            cf_clearance = Some(value.to_string());
                        }
                    }
                }
            }

            if let Some(clearance) = cf_clearance {
                let user_agent = self.get_user_agent(session).await?;
                let ip = self.get_ip_address(session, url).await?;

                return Ok(IuamResult {
                    cf_clearance: clearance,
                    user_agent,
                    cookies: cookie_map,
                    ip,
                });
            }

            sleep(self.poll_interval).await;
            elapsed += self.poll_interval;
        }
    }

    async fn get_user_agent(&self, session: &Arc<CdpSession>) -> Result<String> {
        let result = session
            .evaluate_script("navigator.userAgent")
            .await
            .map_err(|e| SolverError::CdpError(e.to_string()))?;

        if let Some(value) = result.get("result").and_then(|r| r.get("value")) {
            if let Some(ua) = value.as_str() {
                return Ok(ua.to_string());
            }
        }

        Err(SolverError::TokenExtractionFailed(
            "Could not extract User-Agent".to_string(),
        ))
    }

    async fn get_ip_address(&self, session: &Arc<CdpSession>, _url: &str) -> Result<String> {
        let script = r#"
            (async function() {
                try {
                    const response = await fetch('https://api.ipify.org?format=json', {
                        method: 'GET',
                        mode: 'no-cors'
                    });
                    if (!response.ok) {
                        return 'unknown';
                    }
                    const data = await response.json();
                    return data.ip || 'unknown';
                } catch (e) {
                    return 'unknown';
                }
            })()
        "#;

        let result = session
            .evaluate_script(script)
            .await
            .map_err(|e| SolverError::CdpError(format!("Failed to get IP: {}", e)))?;

        if let Some(value) = result.get("result").and_then(|r| r.get("value")) {
            if let Some(ip) = value.as_str() {
                if ip != "unknown" && !ip.is_empty() {
                    return Ok(ip.to_string());
                }
            }
        }

        Ok("unknown".to_string())
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
        assert_eq!(solver.timeout_duration, Duration::from_secs(120));
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
        assert_eq!(solver.timeout_duration, Duration::from_secs(120));
    }
}
