use capsolver::browser::BrowserPool;
use capsolver::config::Config;

/// Chrome-backed tests are marked `#[ignore]`: they need a real browser binary.
/// Run them with `cargo test -- --ignored` on a machine that has one.
fn test_config(browser_pool_size: usize, tabs_per_process: usize) -> Config {
    Config {
        chrome_path: std::env::var("CHROME_PATH")
            .unwrap_or_else(|_| "/usr/bin/google-chrome".to_string()),
        browser_pool_size,
        tabs_per_process,
        server_host: "127.0.0.1".to_string(),
        server_port: 0,
        log_level: "debug".to_string(),
        headless: true,
        disable_sandbox: true,
        user_agent: None,
        solve_timeout_ms: 29_000,
        load_timeout_ms: 30_000,
        cdp_timeout_ms: 10_000,
        startup_timeout_ms: 20_000,
        request_timeout_ms: 60_000,
        cdp_port_base: 9222,
    }
}

#[tokio::test]
async fn test_zero_pool_size_is_rejected() {
    assert!(BrowserPool::new(test_config(0, 2)).await.is_err());
}

#[tokio::test]
async fn test_zero_tabs_is_rejected() {
    assert!(BrowserPool::new(test_config(1, 0)).await.is_err());
}

#[tokio::test]
async fn test_missing_chrome_binary_is_reported() {
    let mut config = test_config(1, 1);
    config.chrome_path = "/nonexistent/chrome".to_string();

    let Err(error) = BrowserPool::new(config).await else {
        panic!("spawning a missing binary must fail");
    };

    assert!(
        error.to_string().contains("/nonexistent/chrome"),
        "error should name the binary it tried: {}",
        error
    );
}

#[ignore]
#[tokio::test]
async fn test_capacity_stats() {
    let pool = BrowserPool::new(test_config(2, 3))
        .await
        .expect("Pool creation failed");
    let stats = pool.get_capacity_stats().await;

    assert_eq!(stats.total, 6, "Total capacity should be 2*3=6");
    assert_eq!(stats.available, 6);
    assert_eq!(stats.active, 0);
    assert_eq!(stats.processes, 2);

    pool.shutdown().await.ok();
}

#[ignore]
#[tokio::test]
async fn test_context_acquisition() {
    let pool = BrowserPool::new(test_config(1, 2))
        .await
        .expect("Pool creation failed");

    let ctx1 = pool.acquire().await.expect("first acquire");
    let stats = pool.get_capacity_stats().await;
    assert_eq!(stats.available, 1);
    assert_eq!(stats.active, 1);

    let ctx2 = pool.acquire().await.expect("second acquire");
    let stats = pool.get_capacity_stats().await;
    assert_eq!(stats.available, 0);
    assert_eq!(stats.active, 2);

    pool.release(ctx1).await.ok();
    pool.release(ctx2).await.ok();
    pool.shutdown().await.ok();
}

#[ignore]
#[tokio::test]
async fn test_context_release() {
    let pool = BrowserPool::new(test_config(1, 1))
        .await
        .expect("Pool creation failed");

    let ctx = pool.acquire().await.expect("Acquisition failed");
    assert_eq!(pool.get_capacity_stats().await.available, 0);

    pool.release(ctx).await.expect("Release failed");
    assert_eq!(pool.get_capacity_stats().await.available, 1);

    pool.shutdown().await.ok();
}

/// Guards the permit-accounting bug where `acquire` dropped its permit immediately
/// and `release` added a new one, letting capacity grow without bound.
#[ignore]
#[tokio::test]
async fn test_capacity_is_stable_across_cycles() {
    let pool = BrowserPool::new(test_config(1, 2))
        .await
        .expect("Pool creation failed");

    for _ in 0..5 {
        let ctx = pool.acquire().await.expect("acquire");
        pool.release(ctx).await.expect("release");
    }

    let stats = pool.get_capacity_stats().await;
    assert_eq!(stats.total, 2, "capacity must not drift across cycles");
    assert_eq!(stats.available, 2);
    assert_eq!(stats.active, 0);

    // A third concurrent acquire must block rather than over-subscribe the pool.
    let a = pool.acquire().await.expect("acquire a");
    let b = pool.acquire().await.expect("acquire b");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(250), pool.acquire())
            .await
            .is_err(),
        "pool must block once every slot is handed out"
    );

    pool.release(a).await.ok();
    pool.release(b).await.ok();
    pool.shutdown().await.ok();
}
