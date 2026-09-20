use super::error::{Result, SolverError};
use super::utils::{StubPageBuilder, validate_sitekey, validate_url};
use crate::browser::{BrowserContext, CdpSession};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

pub struct TurnstileSolver {
    timeout_duration: Duration,
    poll_interval: Duration,
}

impl TurnstileSolver {
    pub fn new() -> Self {
        TurnstileSolver {
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
        sitekey: &str,
        cdata: Option<&str>,
        action: Option<&str>,
    ) -> Result<String> {
        validate_url(url)?;
        validate_sitekey(sitekey)?;

        let session = self.setup_page(context, url, sitekey, cdata, action).await?;

        self.wait_for_token(&session).await
    }

    async fn setup_page(
        &self,
        context: &mut BrowserContext,
        url: &str,
        sitekey: &str,
        cdata: Option<&str>,
        action: Option<&str>,
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

        let mut stub_builder = StubPageBuilder::new(sitekey.to_string(), url.to_string());

        if let Some(cd) = cdata {
            stub_builder = stub_builder.with_cdata(cd.to_string());
        }

        if let Some(act) = action {
            stub_builder = stub_builder.with_action(act.to_string());
        }

        let stub_html = stub_builder.build();

        let data_uri = format!("data:text/html,{}", urlencoding::encode(&stub_html));

        session
            .navigate(&data_uri)
            .await
            .map_err(|e| SolverError::NavigationFailed(e.to_string()))?;

        session
            .wait_for_navigation(15)
            .await
            .map_err(|e| SolverError::NavigationFailed(e.to_string()))?;

        Ok(session)
    }

    async fn wait_for_token(&self, session: &Arc<CdpSession>) -> Result<String> {
        let mut elapsed = Duration::ZERO;

        loop {
            if elapsed >= self.timeout_duration {
                return Err(SolverError::Timeout);
            }

            let token_result = session
                .evaluate_script("window.getTurnstileToken && window.getTurnstileToken()")
                .await;

            match token_result {
                Ok(response) => {
                    if let Some(value) = response.get("result").and_then(|r| r.get("value")) {
                        if value.is_null() {
                            let error_result = session
                                .evaluate_script("window.getTurnstileError && window.getTurnstileError()")
                                .await;

                            if let Ok(err_resp) = error_result {
                                if let Some(err_val) = err_resp.get("result").and_then(|r| r.get("value")) {
                                    if !err_val.is_null() {
                                        return Err(SolverError::ChallengeFailed(
                                            format!("Turnstile error: {:?}", err_val),
                                        ));
                                    }
                                }
                            }
                        } else if let Some(token_str) = value.as_str() {
                            if !token_str.is_empty() {
                                return Ok(token_str.to_string());
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(SolverError::CdpError(format!(
                        "Failed to evaluate token script: {}",
                        e
                    )));
                }
            }

            sleep(self.poll_interval).await;
            elapsed += self.poll_interval;
        }
    }

    pub async fn solve_with_context(
        &self,
        context: &mut BrowserContext,
        params: TurnstileParams,
    ) -> Result<String> {
        self.solve(
            context,
            &params.url,
            &params.sitekey,
            params.cdata.as_deref(),
            params.action.as_deref(),
        )
        .await
    }
}

pub struct TurnstileParams {
    pub url: String,
    pub sitekey: String,
    pub cdata: Option<String>,
    pub action: Option<String>,
}

impl Default for TurnstileSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_solver_creation() {
        let solver = TurnstileSolver::new();
        assert_eq!(solver.timeout_duration, Duration::from_secs(120));
        assert_eq!(solver.poll_interval, Duration::from_millis(500));
    }

    #[test]
    fn test_solver_with_custom_timeout() {
        let solver = TurnstileSolver::new().with_timeout(Duration::from_secs(60));
        assert_eq!(solver.timeout_duration, Duration::from_secs(60));
    }

    #[test]
    fn test_solver_with_custom_poll_interval() {
        let solver = TurnstileSolver::new().with_poll_interval(Duration::from_millis(1000));
        assert_eq!(solver.poll_interval, Duration::from_millis(1000));
    }

    #[test]
    fn test_turnstile_params() {
        let params = TurnstileParams {
            url: "https://example.com".to_string(),
            sitekey: "1x00000000000000000000AA".to_string(),
            cdata: None,
            action: None,
        };

        assert_eq!(params.url, "https://example.com");
        assert_eq!(params.sitekey, "1x00000000000000000000AA");
    }
}
