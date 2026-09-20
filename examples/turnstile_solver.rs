use capsolver::browser::BrowserPool;
use capsolver::config::Config;
use capsolver::solvers::{TurnstileSolver, TurnstileParams};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config {
        chrome_path: "/usr/bin/google-chrome".to_string(),
        browser_pool_size: 1,
        tabs_per_process: 2,
        server_host: "0.0.0.0".to_string(),
        server_port: 8080,
        log_level: "debug".to_string(),
        headless: true,
        disable_sandbox: true,
        user_agent: Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".to_string()),
        solve_timeout: 120,
        load_timeout: 30,
        cdp_timeout: 10,
        cdp_port_base: 9222,
    };

    println!("🚀 Initializing browser pool...");
    let pool = BrowserPool::new(config).await?;

    println!("📋 Acquiring browser context...");
    let mut context = pool.acquire().await?;
    println!("✅ Context acquired: {}", context.context_id);

    println!("🌐 Creating page and loading initial URL...");
    let _page = pool
        .new_page(&mut context, "https://example.com", None)
        .await?;
    println!("✅ Page created and navigated");

    println!("\n🔍 Setting up Turnstile solver...");
    let solver = TurnstileSolver::new();

    println!("⏱️  Solving Turnstile challenge...");
    println!("   - URL: https://example.com");
    println!("   - Sitekey: 1x00000000000000000000AA");
    println!("   - Timeout: 120 seconds");

    let params = TurnstileParams {
        url: "https://example.com".to_string(),
        sitekey: "1x00000000000000000000AA".to_string(),
        cdata: None,
        action: Some("managed".to_string()),
    };

    match solver.solve_with_context(&mut context, params).await {
        Ok(token) => {
            println!("\n✅ SUCCESS!");
            println!("   Token: {}", token);
            println!("   Length: {} characters", token.len());

            if let Some(session) = &context.cdp_session {
                println!("\n📍 Verifying token retrieval...");
                let verify_result = session
                    .evaluate_script("window.getTurnstileToken ? 'Available' : 'Not available'")
                    .await;

                if let Ok(result) = verify_result {
                    println!("   Verification: {:?}", result);
                }
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
