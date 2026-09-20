use capsolver::solvers::{IuamSolver, IuamParams, IuamResult};
use std::collections::HashMap;
use std::time::Duration;

#[tokio::test]
async fn test_iuam_solver_creation() {
    let solver = IuamSolver::new();
    assert_eq!(solver.timeout_duration, Duration::from_secs(29));
    assert_eq!(solver.poll_interval, Duration::from_millis(500));
}

#[tokio::test]
async fn test_iuam_solver_with_timeout() {
    let solver = IuamSolver::new().with_timeout(Duration::from_secs(60));
    assert_eq!(solver.timeout_duration, Duration::from_secs(60));
}

#[tokio::test]
async fn test_iuam_solver_with_poll_interval() {
    let solver = IuamSolver::new()
        .with_poll_interval(Duration::from_millis(1000));
    assert_eq!(solver.poll_interval, Duration::from_millis(1000));
}

#[tokio::test]
async fn test_iuam_solver_chained_config() {
    let solver = IuamSolver::new()
        .with_timeout(Duration::from_secs(90))
        .with_poll_interval(Duration::from_millis(1000));

    assert_eq!(solver.timeout_duration, Duration::from_secs(90));
    assert_eq!(solver.poll_interval, Duration::from_millis(1000));
}

#[tokio::test]
async fn test_iuam_result_creation() {
    let mut cookies = HashMap::new();
    cookies.insert("cf_clearance".to_string(), "test_clearance_value".to_string());
    cookies.insert("__cfruid".to_string(), "test_cfruid".to_string());

    let result = IuamResult {
        cf_clearance: "test_clearance_value".to_string(),
        user_agent: "Mozilla/5.0".to_string(),
        cookies,
        ip: "192.168.1.1".to_string(),
    };

    assert_eq!(result.cf_clearance, "test_clearance_value");
    assert_eq!(result.user_agent, "Mozilla/5.0");
    assert_eq!(result.ip, "192.168.1.1");
    assert_eq!(result.cookies.len(), 2);
    assert_eq!(
        result.cookies.get("cf_clearance"),
        Some(&"test_clearance_value".to_string())
    );
}

#[tokio::test]
async fn test_iuam_result_with_many_cookies() {
    let mut cookies = HashMap::new();
    for i in 0..10 {
        cookies.insert(
            format!("cookie_{}", i),
            format!("value_{}", i),
        );
    }

    let result = IuamResult {
        cf_clearance: "clearance".to_string(),
        user_agent: "Mozilla/5.0".to_string(),
        cookies,
        ip: "10.0.0.1".to_string(),
    };

    assert_eq!(result.cookies.len(), 10);
}

#[tokio::test]
async fn test_iuam_params_creation() {
    let params = IuamParams {
        url: "https://example.com".to_string(),
        proxy: Some("http://proxy:8080".to_string()),
    };

    assert_eq!(params.url, "https://example.com");
    assert_eq!(params.proxy, Some("http://proxy:8080".to_string()));
}

#[tokio::test]
async fn test_iuam_params_without_proxy() {
    let params = IuamParams {
        url: "https://example.com".to_string(),
        proxy: None,
    };

    assert_eq!(params.url, "https://example.com");
    assert!(params.proxy.is_none());
}

#[tokio::test]
async fn test_iuam_solver_default() {
    let solver = IuamSolver::default();
    assert_eq!(solver.timeout_duration, Duration::from_secs(29));
    assert_eq!(solver.poll_interval, Duration::from_millis(500));
}

#[tokio::test]
async fn test_iuam_result_clone() {
    let mut cookies = HashMap::new();
    cookies.insert("test".to_string(), "value".to_string());

    let result1 = IuamResult {
        cf_clearance: "clearance1".to_string(),
        user_agent: "Mozilla/5.0".to_string(),
        cookies,
        ip: "192.168.1.1".to_string(),
    };

    let result2 = result1.clone();

    assert_eq!(result1.cf_clearance, result2.cf_clearance);
    assert_eq!(result1.user_agent, result2.user_agent);
    assert_eq!(result1.ip, result2.ip);
    assert_eq!(result1.cookies, result2.cookies);
}

#[tokio::test]
async fn test_iuam_multiple_solver_instances() {
    let solver1 = IuamSolver::new().with_timeout(Duration::from_secs(30));
    let solver2 = IuamSolver::new().with_timeout(Duration::from_secs(60));
    let solver3 = IuamSolver::new().with_timeout(Duration::from_secs(90));

    assert_eq!(solver1.timeout_duration, Duration::from_secs(30));
    assert_eq!(solver2.timeout_duration, Duration::from_secs(60));
    assert_eq!(solver3.timeout_duration, Duration::from_secs(90));

    assert_eq!(solver1.poll_interval, Duration::from_millis(500));
    assert_eq!(solver2.poll_interval, Duration::from_millis(500));
    assert_eq!(solver3.poll_interval, Duration::from_millis(500));
}

// Note: Full integration test requires Chrome and real Cloudflare challenge
// This test would spawn Chrome processes, so we skip it in CI
#[ignore]
#[tokio::test]
async fn test_iuam_full_integration() {
    use capsolver::browser::BrowserPool;
    use capsolver::config::Config;

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
        cdp_port_base: 9700,
    };

    let pool = BrowserPool::new(config)
        .await
        .expect("Pool creation should succeed");

    pool.shutdown().await.ok();
}
