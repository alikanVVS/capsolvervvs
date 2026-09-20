use thiserror::Error;

#[derive(Error, Debug)]
pub enum SolverError {
    #[error("Pool error: {0}")]
    PoolError(String),

    #[error("CDP error: {0}")]
    CdpError(String),

    #[error("Timeout waiting for token")]
    Timeout,

    #[error("Invalid sitekey: {0}")]
    InvalidSitekey(String),

    #[error("Turnstile not loaded on page")]
    TurnstileNotLoaded,

    #[error("Failed to get token: {0}")]
    TokenExtractionFailed(String),

    #[error("Page navigation failed: {0}")]
    NavigationFailed(String),

    #[error("Script injection failed: {0}")]
    ScriptInjectionFailed(String),

    #[error("Invalid response from Turnstile API")]
    InvalidApiResponse,

    #[error("Challenge failed: {0}")]
    ChallengeFailed(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

pub type Result<T> = std::result::Result<T, SolverError>;
