use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub server_host: String,
    pub server_port: u16,
    pub log_level: String,

    pub chrome_path: String,
    pub browser_pool_size: usize,
    pub tabs_per_process: usize,

    pub headless: bool,
    pub disable_sandbox: bool,
    pub user_agent: Option<String>,

    #[allow(dead_code)]
    pub solve_timeout: u64,
    #[allow(dead_code)]
    pub load_timeout: u64,
    #[allow(dead_code)]
    pub cdp_timeout: u64,

    pub cdp_port_base: u16,
}

impl Config {
    pub fn from_env() -> Self {
        Config {
            server_host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            server_port: env::var("SERVER_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8080),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),

            chrome_path: env::var("CHROME_PATH").unwrap_or_else(|_| "/usr/bin/google-chrome".to_string()),
            browser_pool_size: env::var("BROWSER_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4),
            tabs_per_process: env::var("TABS_PER_PROCESS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),

            headless: env::var("HEADLESS")
                .ok()
                .map(|s| s.to_lowercase() == "true")
                .unwrap_or(true),
            disable_sandbox: env::var("DISABLE_SANDBOX")
                .ok()
                .map(|s| s.to_lowercase() == "true")
                .unwrap_or(false),
            user_agent: env::var("USER_AGENT").ok(),

            solve_timeout: env::var("SOLVE_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(120),
            load_timeout: env::var("LOAD_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
            cdp_timeout: env::var("CDP_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),

            cdp_port_base: env::var("CDP_PORT_BASE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(9222),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::from_env()
    }
}
