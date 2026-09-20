use capsolver::browser::BrowserPool;
use capsolver::config::Config;
use capsolver::solvers::{TurnstileSolver, TurnstileParams};
use std::time::Duration;

#[tokio::test]
async fn test_turnstile_solver_creation() {
    let solver = TurnstileSolver::new();
    assert_eq!(solver.timeout_duration, Duration::from_secs(29));
    assert_eq!(solver.poll_interval, Duration::from_millis(500));
}

#[tokio::test]
async fn test_turnstile_solver_timeout_config() {
    let solver = TurnstileSolver::new()
        .with_timeout(Duration::from_secs(60));

    assert_eq!(solver.timeout_duration, Duration::from_secs(60));
    assert_eq!(solver.poll_interval, Duration::from_millis(500));
}

#[tokio::test]
async fn test_turnstile_solver_poll_interval_config() {
    let solver = TurnstileSolver::new()
        .with_poll_interval(Duration::from_millis(1000));

    assert_eq!(solver.timeout_duration, Duration::from_secs(29));
    assert_eq!(solver.poll_interval, Duration::from_millis(1000));
}

#[tokio::test]
async fn test_turnstile_solver_chained_config() {
    let solver = TurnstileSolver::new()
        .with_timeout(Duration::from_secs(90))
        .with_poll_interval(Duration::from_millis(1000));

    assert_eq!(solver.timeout_duration, Duration::from_secs(90));
    assert_eq!(solver.poll_interval, Duration::from_millis(1000));
}

// Note: Pool integration test requires Chrome to be installed
// This test would spawn actual Chrome processes, so we skip it in CI
// Integration testing should be done manually with: cargo run --example turnstile_solver
#[ignore]
#[tokio::test]
async fn test_pool_and_solver_integration() {
    let config = Config {
        chrome_path: std::env::var("CHROME_PATH")
            .unwrap_or_else(|_| "/usr/bin/google-chrome".to_string()),
        browser_pool_size: 1,
        tabs_per_process: 1,
        server_host: "0.0.0.0".to_string(),
        server_port: 8080,
        log_level: "debug".to_string(),
        headless: true,
        disable_sandbox: true,
        user_agent: None,
        solve_timeout_ms: 29_000,
        load_timeout_ms: 30_000,
        cdp_timeout_ms: 10_000,
        startup_timeout_ms: 20_000,
        request_timeout_ms: 60_000,
        cdp_port_base: 9800,
    };

    let pool = BrowserPool::new(config)
        .await
        .expect("Pool creation should succeed");

    let context = pool.acquire().await.expect("Context acquisition should succeed");

    let stats = pool.get_capacity_stats().await;
    assert_eq!(stats.available, 0, "Should have 0 available after acquire");
    assert_eq!(stats.active, 1, "Should have 1 active after acquire");

    pool.release(context).await.ok();
    pool.shutdown().await.ok();
}

#[tokio::test]
async fn test_turnstile_params_creation() {
    let params = TurnstileParams {
        url: "https://example.com".to_string(),
        sitekey: "1x00000000000000000000AA".to_string(),
        cdata: Some("test_cdata".to_string()),
        action: Some("managed".to_string()),
    };

    assert_eq!(params.url, "https://example.com");
    assert_eq!(params.sitekey, "1x00000000000000000000AA");
    assert_eq!(params.cdata, Some("test_cdata".to_string()));
    assert_eq!(params.action, Some("managed".to_string()));
}

#[tokio::test]
async fn test_turnstile_params_minimal() {
    let params = TurnstileParams {
        url: "https://example.com".to_string(),
        sitekey: "1x00000000000000000000AA".to_string(),
        cdata: None,
        action: None,
    };

    assert_eq!(params.url, "https://example.com");
    assert_eq!(params.sitekey, "1x00000000000000000000AA");
    assert_eq!(params.cdata, None);
    assert_eq!(params.action, None);
}

#[tokio::test]
async fn test_solver_default_creation() {
    let solver = TurnstileSolver::default();
    assert_eq!(solver.timeout_duration, Duration::from_secs(29));
    assert_eq!(solver.poll_interval, Duration::from_millis(500));
}

#[tokio::test]
async fn test_turnstile_error_types() {
    use capsolver::solvers::SolverError;

    let _timeout_err = SolverError::Timeout;
    let _invalid_sitekey_err = SolverError::InvalidSitekey("test".to_string());
    let _config_err = SolverError::ConfigError("test".to_string());

    // These should all be constructible
}
