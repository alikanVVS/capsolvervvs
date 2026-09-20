use capsolver::api::create_router;
use capsolver::{BrowserPool, Config};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&config.log_level)),
        )
        .init();

    tracing::info!(
        processes = config.browser_pool_size,
        tabs_per_process = config.tabs_per_process,
        chrome = %config.chrome_path,
        "starting CAPTCHA solver service"
    );

    let pool = BrowserPool::new(config.clone()).await?;
    tracing::info!("browser pool ready");

    let config = Arc::new(config);
    let router = create_router(Arc::clone(&pool), Arc::clone(&config));

    let listener =
        tokio::net::TcpListener::bind((config.server_host.clone(), config.server_port)).await?;
    tracing::info!(
        address = %listener.local_addr()?,
        "listening"
    );

    // Without a shutdown signal `serve` never returns and the pool is never torn down,
    // leaving Chrome processes behind.
    let result = axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    tracing::info!("shutting down browser pool");
    pool.shutdown().await?;

    result?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl+C"),
        _ = terminate => tracing::info!("received SIGTERM"),
    }
}
