use thiserror::Error;

#[derive(Error, Debug)]
pub enum PoolError {
    #[error("Pool capacity exhausted")]
    #[allow(dead_code)]
    CapacityExhausted,

    #[error("Browser process crashed: {0}")]
    #[allow(dead_code)]
    ProcessCrashed(String),

    #[error("Failed to spawn Chrome process: {0}")]
    ProcessSpawnError(String),

    #[error("CDP connection error: {0}")]
    #[allow(dead_code)]
    CdpConnectionError(String),

    #[error("CDP protocol error: {0}")]
    #[allow(dead_code)]
    CdpProtocolError(String),

    #[error("WebSocket error: {0}")]
    #[allow(dead_code)]
    WebSocketError(String),

    #[error("Timeout: {0}")]
    #[allow(dead_code)]
    Timeout(String),

    #[error("Navigation failed: {0}")]
    #[allow(dead_code)]
    NavigationFailed(String),

    #[error("Script execution failed: {0}")]
    #[allow(dead_code)]
    ScriptExecutionFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Invalid configuration: {0}")]
    #[allow(dead_code)]
    InvalidConfig(String),

    #[error("Context not found")]
    #[allow(dead_code)]
    ContextNotFound,

    #[error("Page not found")]
    #[allow(dead_code)]
    PageNotFound,
}

pub type Result<T> = std::result::Result<T, PoolError>;
