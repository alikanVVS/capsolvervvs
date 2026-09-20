use capsolver::{Config, BrowserPool};
use capsolver::api::create_router;
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse()?)
        )
        .init();

    let config = Config::from_env();

    tracing::info!("Starting CAPTCHA Solver Service");
    tracing::info!(
        "Configuration: {} Chrome processes, {} tabs per process",
        config.browser_pool_size,
        config.tabs_per_process
    );

    let pool = BrowserPool::new(config.clone()).await?;
    tracing::info!("Browser pool initialized");

    let router = create_router(pool.clone());
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", config.server_host, config.server_port))
        .await?;

    tracing::info!(
        "Server listening on {}:{}",
        config.server_host,
        config.server_port
    );

    axum::serve(listener, router)
        .await?;

    pool.shutdown().await?;
    tracing::info!("Service shutdown gracefully");

    Ok(())
}
