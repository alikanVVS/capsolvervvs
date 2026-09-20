use capsolver::browser::BrowserPool;
use capsolver::config::Config;

#[tokio::test]
async fn test_pool_creation() {
    let config = Config {
        chrome_path: "/usr/bin/google-chrome".to_string(),
        browser_pool_size: 1,
        tabs_per_process: 2,
        server_host: "0.0.0.0".to_string(),
        server_port: 8080,
        log_level: "debug".to_string(),
        headless: true,
        disable_sandbox: true,
        user_agent: None,
        solve_timeout: 120,
        load_timeout: 30,
        cdp_timeout: 10,
        cdp_port_base: 9222,
    };

    let pool = BrowserPool::new(config).await;
    assert!(pool.is_ok(), "Pool creation should succeed");
}

#[tokio::test]
async fn test_capacity_stats() {
    let config = Config {
        chrome_path: "/usr/bin/google-chrome".to_string(),
        browser_pool_size: 2,
        tabs_per_process: 3,
        server_host: "0.0.0.0".to_string(),
        server_port: 8080,
        log_level: "info".to_string(),
        headless: true,
        disable_sandbox: true,
        user_agent: None,
        solve_timeout: 120,
        load_timeout: 30,
        cdp_timeout: 10,
        cdp_port_base: 9222,
    };

    let pool = BrowserPool::new(config).await.expect("Pool creation failed");
    let stats = pool.get_capacity_stats().await;

    assert_eq!(stats.total, 6, "Total capacity should be 2*3=6");
    assert_eq!(stats.available, 6, "All contexts should be available initially");
    assert_eq!(stats.active, 0, "No active contexts initially");
    assert_eq!(stats.processes, 2, "Should have 2 processes");

    pool.shutdown().await.ok();
}

#[tokio::test]
async fn test_context_acquisition() {
    let config = Config {
        chrome_path: "/usr/bin/google-chrome".to_string(),
        browser_pool_size: 1,
        tabs_per_process: 2,
        server_host: "0.0.0.0".to_string(),
        server_port: 8080,
        log_level: "info".to_string(),
        headless: true,
        disable_sandbox: true,
        user_agent: None,
        solve_timeout: 120,
        load_timeout: 30,
        cdp_timeout: 10,
        cdp_port_base: 9222,
    };

    let pool = BrowserPool::new(config).await.expect("Pool creation failed");

    let ctx1_result = pool.acquire().await;
    assert!(ctx1_result.is_ok(), "First acquisition should succeed");

    let stats = pool.get_capacity_stats().await;
    assert_eq!(stats.available, 1, "One context should be available after first acquire");
    assert_eq!(stats.active, 1, "One context should be active");

    let ctx2_result = pool.acquire().await;
    assert!(ctx2_result.is_ok(), "Second acquisition should succeed");

    let stats = pool.get_capacity_stats().await;
    assert_eq!(stats.available, 0, "All contexts should be in use");
    assert_eq!(stats.active, 2, "Both contexts should be active");

    pool.shutdown().await.ok();
}

#[tokio::test]
async fn test_context_release() {
    let config = Config {
        chrome_path: "/usr/bin/google-chrome".to_string(),
        browser_pool_size: 1,
        tabs_per_process: 1,
        server_host: "0.0.0.0".to_string(),
        server_port: 8080,
        log_level: "info".to_string(),
        headless: true,
        disable_sandbox: true,
        user_agent: None,
        solve_timeout: 120,
        load_timeout: 30,
        cdp_timeout: 10,
        cdp_port_base: 9222,
    };

    let pool = BrowserPool::new(config).await.expect("Pool creation failed");

    let ctx = pool.acquire().await.expect("Acquisition failed");
    let stats = pool.get_capacity_stats().await;
    assert_eq!(stats.available, 0);

    pool.release(ctx).await.expect("Release failed");
    let stats = pool.get_capacity_stats().await;
    assert_eq!(stats.available, 1, "Context should be available after release");

    pool.shutdown().await.ok();
}
