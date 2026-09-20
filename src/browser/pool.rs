use crate::browser::cdp::{CdpConnection, CdpSession};
use crate::config::Config;
use crate::error::{PoolError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::{RwLock, Semaphore};

#[derive(Clone, Debug)]
pub struct BrowserContext {
    /// Identifies the pool slot. Stable across acquire/release cycles.
    pub context_id: String,
    pub process_index: usize,
    pub port: u16,
    /// The real CDP browserContextId, created on `new_page` once the proxy is known.
    pub browser_context_id: Option<String>,
    pub cdp_session: Option<Arc<CdpSession>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Page {
    pub page_id: String,
    pub url: String,
    pub context_id: String,
}

struct BrowserProcess {
    index: usize,
    port: u16,
    process: Option<Child>,
    connection: Option<Arc<CdpConnection>>,
    user_data_dir: std::path::PathBuf,
    is_healthy: bool,
}

impl BrowserProcess {
    async fn spawn(index: usize, port: u16, config: &Config) -> Result<Self> {
        let user_data_dir = std::env::temp_dir().join(format!(
            "capsolver-chrome-{}-{}",
            index,
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&user_data_dir)?;

        let mut cmd = Command::new(&config.chrome_path);

        if config.headless {
            cmd.arg("--headless=new");
        }

        cmd.arg(format!("--remote-debugging-port={}", port))
            .arg("--remote-debugging-address=127.0.0.1")
            .arg(format!("--user-data-dir={}", user_data_dir.display()))
            .arg("--disable-gpu")
            // Chrome's default /dev/shm is tiny in containers; without this it crashes under load.
            .arg("--disable-dev-shm-usage")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-default-apps")
            .arg("--disable-background-networking")
            .arg("--disable-backgrounding-occluded-windows")
            .arg("--disable-renderer-backgrounding")
            .arg("--window-size=1920,1080")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // Without this a panic leaves orphaned Chrome processes behind.
            .kill_on_drop(true);

        if config.disable_sandbox {
            cmd.arg("--no-sandbox");
        }

        if let Some(user_agent) = &config.user_agent {
            cmd.arg(format!("--user-agent={}", user_agent));
        }

        let spawned = cmd
            .spawn()
            .map_err(|e| PoolError::ProcessSpawnError(format!("{}: {}", config.chrome_path, e)));

        // Every failure past this point must still clean up the directory above.
        let mut process = match spawned {
            Ok(process) => process,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&user_data_dir);
                return Err(e);
            }
        };

        let started = async {
            let ws_url = Self::wait_for_devtools(port, config.startup_timeout()).await?;
            CdpConnection::connect(&ws_url).await
        }
        .await;

        let connection = match started {
            Ok(connection) => connection,
            Err(e) => {
                let _ = process.kill().await;
                let _ = std::fs::remove_dir_all(&user_data_dir);
                return Err(e);
            }
        };

        Ok(BrowserProcess {
            index,
            port,
            process: Some(process),
            connection: Some(connection),
            user_data_dir,
            is_healthy: true,
        })
    }

    /// Polls the DevTools HTTP endpoint until Chrome reports the browser WebSocket URL.
    async fn wait_for_devtools(port: u16, timeout: Duration) -> Result<String> {
        let client = reqwest::Client::new();
        let endpoint = format!("http://127.0.0.1:{}/json/version", port);
        let deadline = tokio::time::Instant::now() + timeout;
        // Always assigned by the loop body before the deadline check reads it.
        let mut last_error;

        loop {
            match client.get(&endpoint).send().await {
                Ok(response) => match response.json::<Value>().await {
                    Ok(body) => {
                        if let Some(ws_url) = body
                            .get("webSocketDebuggerUrl")
                            .and_then(Value::as_str)
                        {
                            return Ok(ws_url.to_string());
                        }
                        last_error = "devtools response had no webSocketDebuggerUrl".to_string();
                    }
                    Err(e) => last_error = e.to_string(),
                },
                Err(e) => last_error = e.to_string(),
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(PoolError::ProcessSpawnError(format!(
                    "port {}: {}",
                    port, last_error
                )));
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn is_alive(&mut self) -> bool {
        let Some(child) = &mut self.process else {
            return false;
        };

        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) | Err(_) => {
                self.is_healthy = false;
                false
            }
        }
    }

    async fn kill(&mut self) {
        // Ask Chrome to close itself first: SIGKILL on the parent orphans its zygote
        // and renderer children, which then race the directory removal below.
        if let Some(connection) = &self.connection {
            let _ = connection
                .call("Browser.close", json!({}), None, Duration::from_secs(5))
                .await;
        }

        if let Some(mut child) = self.process.take() {
            if tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .is_err()
            {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }

        if let Some(connection) = self.connection.take() {
            connection.close().await;
        }

        let _ = std::fs::remove_dir_all(&self.user_data_dir);
    }
}

pub struct BrowserPool {
    config: Config,
    processes: Arc<RwLock<Vec<BrowserProcess>>>,
    semaphore: Arc<Semaphore>,
    available_contexts: Arc<RwLock<VecDeque<BrowserContext>>>,
    active_contexts: Arc<RwLock<HashMap<String, BrowserContext>>>,
    health_monitor: RwLock<Option<tokio::task::JoinHandle<()>>>,
}

impl BrowserPool {
    pub async fn new(config: Config) -> Result<Arc<Self>> {
        if config.browser_pool_size == 0 || config.tabs_per_process == 0 {
            return Err(PoolError::InvalidConfig(
                "browser_pool_size and tabs_per_process must both be greater than zero".to_string(),
            ));
        }

        let total_capacity = config.browser_pool_size * config.tabs_per_process;
        let semaphore = Arc::new(Semaphore::new(total_capacity));

        let mut processes = Vec::new();
        for index in 0..config.browser_pool_size {
            let port = config.cdp_port_base + index as u16;
            match BrowserProcess::spawn(index, port, &config).await {
                Ok(process) => processes.push(process),
                Err(e) => {
                    // Don't leak the processes that already started.
                    for mut started in processes {
                        started.kill().await;
                    }
                    return Err(e);
                }
            }
        }

        let mut available_contexts = VecDeque::new();
        for process_index in 0..config.browser_pool_size {
            for _ in 0..config.tabs_per_process {
                available_contexts.push_back(BrowserContext {
                    context_id: uuid::Uuid::new_v4().to_string(),
                    process_index,
                    port: config.cdp_port_base + process_index as u16,
                    browser_context_id: None,
                    cdp_session: None,
                });
            }
        }

        let pool = Arc::new(BrowserPool {
            config,
            processes: Arc::new(RwLock::new(processes)),
            semaphore,
            available_contexts: Arc::new(RwLock::new(available_contexts)),
            active_contexts: Arc::new(RwLock::new(HashMap::new())),
            health_monitor: RwLock::new(None),
        });

        pool.spawn_health_monitor().await;
        Ok(pool)
    }

    pub async fn acquire(&self) -> Result<BrowserContext> {
        let permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| PoolError::CapacityExhausted)?;

        // `release` hands the permit back with `add_permits`, so this one must not
        // return itself on drop -- otherwise capacity grows without bound.
        permit.forget();

        let mut context = match self.available_contexts.write().await.pop_front() {
            Some(context) => context,
            None => {
                self.semaphore.add_permits(1);
                return Err(PoolError::CapacityExhausted);
            }
        };

        if let Some(process) = self.processes.read().await.get(context.process_index) {
            context.port = process.port;
        }

        context.browser_context_id = None;
        context.cdp_session = None;

        self.active_contexts
            .write()
            .await
            .insert(context.context_id.clone(), context.clone());

        Ok(context)
    }

    pub async fn release(&self, mut context: BrowserContext) -> Result<()> {
        if let Some(session) = context.cdp_session.take() {
            let _ = session.close().await;
        }

        // Disposing the browser context discards its pages, cookies and storage so the
        // next solve on this slot starts clean.
        if let Some(browser_context_id) = context.browser_context_id.take() {
            if let Some(connection) = self.connection_for(context.process_index).await {
                let _ = connection
                    .call(
                        "Target.disposeBrowserContext",
                        json!({ "browserContextId": browser_context_id }),
                        None,
                        self.config.cdp_timeout_duration(),
                    )
                    .await;
            }
        }

        self.active_contexts
            .write()
            .await
            .remove(&context.context_id);
        self.available_contexts.write().await.push_back(context);
        self.semaphore.add_permits(1);

        Ok(())
    }

    async fn connection_for(&self, process_index: usize) -> Option<Arc<CdpConnection>> {
        self.processes
            .read()
            .await
            .get(process_index)
            .and_then(|process| process.connection.clone())
    }

    /// Creates an isolated browser context, opens a target in it, attaches, and navigates.
    pub async fn new_page(
        &self,
        context: &mut BrowserContext,
        url: &str,
        proxy: Option<String>,
    ) -> Result<Page> {
        let connection = self
            .connection_for(context.process_index)
            .await
            .ok_or_else(|| {
                PoolError::ProcessCrashed(format!(
                    "process {} has no live CDP connection",
                    context.process_index
                ))
            })?;

        let cdp_timeout = self.config.cdp_timeout_duration();

        let mut create_context_params = json!({ "disposeOnDetach": false });
        if let Some(proxy) = proxy.as_deref() {
            // A proxy can only be bound when the browser context is created.
            create_context_params["proxyServer"] = json!(proxy);
        }

        let browser_context = connection
            .call(
                "Target.createBrowserContext",
                create_context_params,
                None,
                cdp_timeout,
            )
            .await?;

        let browser_context_id = browser_context
            .get("browserContextId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                PoolError::CdpProtocolError("createBrowserContext returned no id".to_string())
            })?
            .to_string();
        context.browser_context_id = Some(browser_context_id.clone());

        let target = connection
            .call(
                "Target.createTarget",
                json!({ "url": "about:blank", "browserContextId": browser_context_id }),
                None,
                cdp_timeout,
            )
            .await?;

        let target_id = target
            .get("targetId")
            .and_then(Value::as_str)
            .ok_or_else(|| PoolError::CdpProtocolError("createTarget returned no id".to_string()))?
            .to_string();

        let attached = connection
            .call(
                "Target.attachToTarget",
                json!({ "targetId": target_id, "flatten": true }),
                None,
                cdp_timeout,
            )
            .await?;

        let session_id = attached
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                PoolError::CdpProtocolError("attachToTarget returned no sessionId".to_string())
            })?
            .to_string();

        let session = CdpSession::from_attached(
            Arc::clone(&connection),
            session_id,
            target_id,
            cdp_timeout,
        );

        session.enable_domains().await?;
        session.set_viewport(1920, 1080).await?;
        session.navigate(url).await?;
        session
            .wait_for_navigation(self.config.load_timeout_duration())
            .await?;

        context.cdp_session = Some(session);

        Ok(Page {
            page_id: uuid::Uuid::new_v4().to_string(),
            url: url.to_string(),
            context_id: context.context_id.clone(),
        })
    }

    pub async fn get_capacity_stats(&self) -> CapacityStats {
        let available = self.available_contexts.read().await.len();
        let active = self.active_contexts.read().await.len();

        CapacityStats {
            total: available + active,
            available,
            active,
            processes: self.processes.read().await.len(),
        }
    }

    async fn spawn_health_monitor(&self) {
        let processes = Arc::clone(&self.processes);
        let config = self.config.clone();

        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;

                // Find the dead ones under a read lock so acquires aren't blocked
                // while Chrome restarts.
                let dead: Vec<(usize, u16)> = {
                    let mut procs = processes.write().await;
                    procs
                        .iter_mut()
                        .filter(|process| process.process.is_some())
                        .filter_map(|process| {
                            (!process.is_alive()).then_some((process.index, process.port))
                        })
                        .collect()
                };

                for (index, port) in dead {
                    tracing::warn!(index, port, "chrome process died, restarting");

                    match BrowserProcess::spawn(index, port, &config).await {
                        Ok(replacement) => {
                            let mut procs = processes.write().await;
                            if let Some(slot) = procs.iter_mut().find(|p| p.index == index) {
                                slot.kill().await;
                                *slot = replacement;
                            }
                            tracing::info!(index, port, "chrome process restarted");
                        }
                        Err(e) => tracing::error!(index, port, error = %e, "restart failed"),
                    }
                }
            }
        });

        *self.health_monitor.write().await = Some(handle);
    }

    pub async fn shutdown(&self) -> Result<()> {
        if let Some(monitor) = self.health_monitor.write().await.take() {
            monitor.abort();
        }

        let mut processes = self.processes.write().await;
        for process in processes.iter_mut() {
            process.kill().await;
        }
        processes.clear();

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CapacityStats {
    pub total: usize,
    pub available: usize,
    pub active: usize,
    pub processes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            chrome_path: std::env::var("CHROME_PATH")
                .unwrap_or_else(|_| "/usr/bin/google-chrome".to_string()),
            browser_pool_size: 2,
            tabs_per_process: 3,
            server_host: "0.0.0.0".to_string(),
            server_port: 8080,
            log_level: "info".to_string(),
            headless: true,
            disable_sandbox: true,
            user_agent: None,
            solve_timeout_ms: 29_000,
            load_timeout_ms: 30_000,
            cdp_timeout_ms: 10_000,
            startup_timeout_ms: 20_000,
            request_timeout_ms: 60_000,
            cdp_port_base: 9600,
        }
    }

    #[tokio::test]
    async fn test_rejects_zero_capacity() {
        let mut config = test_config();
        config.tabs_per_process = 0;

        assert!(BrowserPool::new(config).await.is_err());
    }

    #[test]
    fn test_capacity_math() {
        let config = test_config();
        assert_eq!(config.browser_pool_size * config.tabs_per_process, 6);
    }

    // Needs a real Chrome binary, so it stays out of the default run.
    #[ignore]
    #[tokio::test]
    async fn test_capacity_stats() {
        let pool = BrowserPool::new(test_config()).await.unwrap();
        let stats = pool.get_capacity_stats().await;

        assert_eq!(stats.total, 6);
        assert_eq!(stats.available, 6);
        assert_eq!(stats.active, 0);

        pool.shutdown().await.unwrap();
    }

    #[ignore]
    #[tokio::test]
    async fn test_acquire_release_keeps_capacity_constant() {
        let pool = BrowserPool::new(test_config()).await.unwrap();

        let context = pool.acquire().await.unwrap();
        assert_eq!(pool.get_capacity_stats().await.active, 1);

        pool.release(context).await.unwrap();
        let stats = pool.get_capacity_stats().await;
        assert_eq!(stats.active, 0);
        assert_eq!(stats.available, 6);

        pool.shutdown().await.unwrap();
    }
}
