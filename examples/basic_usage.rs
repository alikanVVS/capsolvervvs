use capsolver::browser::BrowserPool;
use capsolver::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config {
        chrome_path: std::env::var("CHROME_PATH")
            .unwrap_or_else(|_| "/usr/bin/google-chrome".to_string()),
        browser_pool_size: 2,
        tabs_per_process: 3,
        server_host: "0.0.0.0".to_string(),
        server_port: 407,
        log_level: "debug".to_string(),
        headless: true,
        disable_sandbox: false,
        user_agent: Some("Mozilla/5.0".to_string()),
        solve_timeout_ms: 29_000,
        load_timeout_ms: 30_000,
        cdp_timeout_ms: 10_000,
        startup_timeout_ms: 20_000,
        request_timeout_ms: 60_000,
        cdp_port_base: 9222,
    };

    tracing::info!("Initializing browser pool with {} processes", config.browser_pool_size);

    let pool = BrowserPool::new(config).await?;

    let stats = pool.get_capacity_stats().await;
    println!("Pool Capacity Stats:");
    println!("  Total: {}", stats.total);
    println!("  Available: {}", stats.available);
    println!("  Active: {}", stats.active);
    println!("  Processes: {}", stats.processes);

    let mut context = pool.acquire().await?;
    println!("\nAcquired context: {}", context.context_id);

    let stats = pool.get_capacity_stats().await;
    println!("\nUpdated Pool Stats:");
    println!("  Available: {}", stats.available);
    println!("  Active: {}", stats.active);

    if let Ok(_page) = pool.new_page(&mut context, "https://example.com", None).await {
        println!("\nSuccessfully created page and navigated to https://example.com");
    }

    pool.release(context).await?;
    println!("\nContext released back to pool");

    let stats = pool.get_capacity_stats().await;
    println!("\nFinal Pool Stats:");
    println!("  Available: {}", stats.available);
    println!("  Active: {}", stats.active);

    pool.shutdown().await?;
    println!("\nBrowser pool shutdown gracefully");

    Ok(())
}
