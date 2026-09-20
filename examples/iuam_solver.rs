use capsolver::browser::BrowserPool;
use capsolver::config::Config;
use capsolver::solvers::{IuamSolver, IuamParams};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config {
        chrome_path: std::env::var("CHROME_PATH")
            .unwrap_or_else(|_| "/usr/bin/google-chrome".to_string()),
        browser_pool_size: 1,
        tabs_per_process: 2,
        server_host: "0.0.0.0".to_string(),
        server_port: 407,
        log_level: "debug".to_string(),
        headless: true,
        disable_sandbox: true,
        user_agent: Some(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".to_string(),
        ),
        solve_timeout_ms: 29_000,
        load_timeout_ms: 30_000,
        cdp_timeout_ms: 10_000,
        startup_timeout_ms: 20_000,
        request_timeout_ms: 60_000,
        cdp_port_base: 9222,
    };

    println!("🚀 Initializing browser pool...");
    let pool = BrowserPool::new(config).await?;

    println!("📋 Acquiring browser context...");
    let mut context = pool.acquire().await?;
    println!("✅ Context acquired: {}", context.context_id);

    println!("🌐 Creating page...");
    let _page = pool
        .new_page(&mut context, "about:blank", None)
        .await?;
    println!("✅ Page created");

    println!("\n🔍 Setting up IUAM solver...");
    let solver = IuamSolver::new()
        .with_timeout(Duration::from_secs(29))
        .with_poll_interval(Duration::from_millis(1000));

    println!("⏱️  Solving IUAM challenge...");
    println!("   - URL: https://example.com");
    println!("   - Timeout: 29 seconds");
    println!("   - Poll interval: 1000ms");

    let params = IuamParams {
        url: "https://example.com".to_string(),
        proxy: None,
    };

    match solver.solve_with_params(&mut context, params).await {
        Ok(result) => {
            println!("\n✅ SUCCESS!");
            println!("   cf_clearance: {}", &result.cf_clearance[..30.min(result.cf_clearance.len())]);
            println!("   User-Agent: {}", result.user_agent);
            println!("   IP Address: {}", result.ip);
            println!("   Cookies: {} total", result.cookies.len());

            println!("\n📍 Cookie Details:");
            for (name, value) in result.cookies.iter().take(5) {
                let display_value = if value.len() > 50 {
                    format!("{}...", &value[..47])
                } else {
                    value.clone()
                };
                println!("   - {}: {}", name, display_value);
            }

            if result.cookies.len() > 5 {
                println!("   ... and {} more cookies", result.cookies.len() - 5);
            }
        }
        Err(e) => {
            println!("\n❌ FAILED!");
            println!("   Error: {}", e);
            println!("   Error type: {:?}", e);
        }
    }

    println!("\n🔄 Releasing context...");
    pool.release(context).await?;
    println!("✅ Context released");

    let stats = pool.get_capacity_stats().await;
    println!(
        "\n📊 Final Pool Stats: {} available, {} active",
        stats.available, stats.active
    );

    println!("\n🛑 Shutting down pool...");
    pool.shutdown().await?;
    println!("✅ Pool shutdown complete");

    Ok(())
}
