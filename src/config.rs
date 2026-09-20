use std::env;
use std::time::Duration;

/// All timeouts are milliseconds, matching the deployment env vars.
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

    pub solve_timeout_ms: u64,
    pub load_timeout_ms: u64,
    pub cdp_timeout_ms: u64,
    pub startup_timeout_ms: u64,
    pub request_timeout_ms: u64,

    pub cdp_port_base: u16,
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .map(|value| matches!(value.trim().to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Self {
        Config {
            server_host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            // PORT is the conventional name in most container platforms.
            server_port: env::var("SERVER_PORT")
                .or_else(|_| env::var("PORT"))
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(407),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),

            chrome_path: env::var("CHROME_PATH")
                .or_else(|_| env::var("CHROME_BIN"))
                .unwrap_or_else(|_| "/usr/bin/google-chrome".to_string()),
            browser_pool_size: env_parse("BROWSER_POOL_SIZE", 2),
            tabs_per_process: env_parse("TABS_PER_PROCESS", 10),

            headless: env_bool("HEADLESS", true),
            // Chrome cannot sandbox inside most containers, so this defaults on.
            disable_sandbox: env_bool("DISABLE_SANDBOX", true),
            user_agent: env::var("USER_AGENT").ok(),

            solve_timeout_ms: env_parse("SOLVE_TIMEOUT", 29_000),
            load_timeout_ms: env_parse("LOAD_TIMEOUT", 30_000),
            cdp_timeout_ms: env_parse("CDP_TIMEOUT", 10_000),
            startup_timeout_ms: env_parse("STARTUP_TIMEOUT", 20_000),
            request_timeout_ms: env_parse("REQUEST_TIMEOUT", 60_000),

            cdp_port_base: env_parse("CDP_PORT_BASE", 9222),
        }
    }

    pub fn solve_timeout_duration(&self) -> Duration {
        Duration::from_millis(self.solve_timeout_ms)
    }

    pub fn load_timeout_duration(&self) -> Duration {
        Duration::from_millis(self.load_timeout_ms)
    }

    pub fn cdp_timeout_duration(&self) -> Duration {
        Duration::from_millis(self.cdp_timeout_ms)
    }

    pub fn startup_timeout(&self) -> Duration {
        Duration::from_millis(self.startup_timeout_ms)
    }

    pub fn request_timeout(&self) -> Duration {
        Duration::from_millis(self.request_timeout_ms)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::from_env()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeouts_are_milliseconds() {
        let config = Config {
            solve_timeout_ms: 29_000,
            ..Config::from_env()
        };

        assert_eq!(config.solve_timeout_duration(), Duration::from_secs(29));
    }
}
