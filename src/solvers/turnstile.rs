use super::error::{Result, SolverError};
use super::utils::{validate_sitekey, validate_url, StubPageBuilder};
use crate::browser::{BrowserContext, CdpSession};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

pub struct TurnstileSolver {
    pub timeout_duration: Duration,
    pub poll_interval: Duration,
}

impl TurnstileSolver {
    pub fn new() -> Self {
        TurnstileSolver {
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

        let session = context
            .cdp_session
            .as_ref()
            .ok_or_else(|| SolverError::CdpError("No CDP session in context".to_string()))?
            .clone();

        let mut stub = StubPageBuilder::new(sitekey.to_string());
        if let Some(cdata) = cdata {
            stub = stub.with_cdata(cdata.to_string());
        }
        if let Some(action) = action {
            stub = stub.with_action(action.to_string());
        }

        let interceptor = self.serve_stub_at(&session, url, stub.build()).await?;
        let outcome = self.navigate_and_wait(&session, url).await;

        interceptor.abort();
        let _ = session.disable_fetch().await;

        outcome
    }

    /// Turnstile validates the embedding origin against the sitekey, so the stub has to be
    /// served *as* the target URL. Intercepting the document request is what makes the
    /// widget see the real origin instead of `null`.
    async fn serve_stub_at(
        &self,
        session: &Arc<CdpSession>,
        url: &str,
        html: String,
    ) -> Result<tokio::task::JoinHandle<()>> {
        session
            .enable_fetch(json!([{ "urlPattern": url, "requestStage": "Request" }]))
            .await
            .map_err(|e| SolverError::CdpError(format!("Fetch.enable failed: {}", e)))?;

        let mut events = session.subscribe();
        let session = Arc::clone(session);
        let expected_session = session.session_id().map(str::to_string);

        Ok(tokio::spawn(async move {
            while let Ok(event) = events.recv().await {
                if event.get("method").and_then(Value::as_str) != Some("Fetch.requestPaused") {
                    continue;
                }

                // With flattened sessions every event names the session it belongs to.
                let event_session = event.get("sessionId").and_then(Value::as_str);
                if expected_session.as_deref() != event_session {
                    continue;
                }

                let Some(params) = event.get("params") else {
                    continue;
                };
                let Some(request_id) = params.get("requestId").and_then(Value::as_str) else {
                    continue;
                };

                let is_document =
                    params.get("resourceType").and_then(Value::as_str) == Some("Document");

                let result = if is_document {
                    session
                        .fulfill_request(request_id, &html, "text/html; charset=utf-8")
                        .await
                } else {
                    session.continue_request(request_id).await
                };

                if let Err(e) = result {
                    tracing::warn!(error = %e, "failed to handle intercepted request");
                }
            }
        }))
    }

    async fn navigate_and_wait(&self, session: &Arc<CdpSession>, url: &str) -> Result<String> {
        session
            .navigate(url)
            .await
            .map_err(|e| SolverError::NavigationFailed(e.to_string()))?;

        session
            .wait_for_navigation(self.timeout_duration)
            .await
            .map_err(|e| SolverError::NavigationFailed(e.to_string()))?;

        self.wait_for_token(session).await
    }

    async fn wait_for_token(&self, session: &Arc<CdpSession>) -> Result<String> {
        let deadline = tokio::time::Instant::now() + self.timeout_duration;

        loop {
            let response = session
                .evaluate_script(
                    "({ token: window.getTurnstileToken ? window.getTurnstileToken() : null, \
                       error: window.getTurnstileError ? window.getTurnstileError() : null })",
                )
                .await
                .map_err(|e| SolverError::CdpError(format!("token poll failed: {}", e)))?;

            let value = response.get("result").and_then(|r| r.get("value"));

            if let Some(value) = value {
                if let Some(token) = value.get("token").and_then(Value::as_str) {
                    if !token.is_empty() {
                        return Ok(token.to_string());
                    }
                }

                if let Some(error) = value.get("error").and_then(Value::as_str) {
                    return Err(SolverError::ChallengeFailed(format!(
                        "Turnstile error {}",
                        error
                    )));
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(SolverError::Timeout);
            }

            sleep(self.poll_interval).await;
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
        assert_eq!(solver.timeout_duration, Duration::from_secs(29));
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
