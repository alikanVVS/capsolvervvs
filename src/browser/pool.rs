use crate::config::Config;
use crate::error::{PoolError, Result};
use crate::browser::cdp::CdpSession;
use std::sync::Arc;
use tokio::sync::{Semaphore, RwLock};
use tokio::process::{Child, Command};
use std::collections::{VecDeque, HashMap};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct BrowserContext {
    pub context_id: String,
    pub process_index: usize,
    pub port: u16,
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
    cdp_session: Option<Arc<CdpSession>>,
    target_id: String,
    is_healthy: bool,
}

impl BrowserProcess {
    async fn spawn(index: usize, port: u16, config: &Config) -> Result<Self> {
        let mut cmd = Command::new(&config.chrome_path);

        cmd.arg("--headless")
            .arg(format!("--remote-debugging-port={}", port))
            .arg("--disable-gpu")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-default-apps");

        if config.disable_sandbox {
            cmd.arg("--no-sandbox");
        }

        if let Some(ua) = &config.user_agent {
            cmd.arg(format!("--user-agent={}", ua));
        }

        let process = cmd
            .spawn()
            .map_err(|e| PoolError::ProcessSpawnError(e.to_string()))?;

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        Ok(BrowserProcess {
            index,
            port,
            process: Some(process),
            cdp_session: None,
            target_id: String::new(),
            is_healthy: true,
        })
    }

    fn is_alive(&mut self) -> bool {
        if let Some(child) = &mut self.process {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.is_healthy = false;
                    false
                }
                Ok(None) => true,
                Err(_) => {
                    self.is_healthy = false;
                    false
                }
            }
        } else {
            false
        }
    }

    async fn kill(&mut self) -> Result<()> {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill().await;
        }
        if let Some(session) = &self.cdp_session {
            let _ = session.close().await;
        }
        Ok(())
    }
}

pub struct BrowserPool {
    config: Config,
    processes: Arc<RwLock<Vec<BrowserProcess>>>,
    semaphore: Arc<Semaphore>,
    available_contexts: Arc<RwLock<VecDeque<BrowserContext>>>,
    active_contexts: Arc<RwLock<HashMap<String, BrowserContext>>>,
}

impl BrowserPool {
    pub async fn new(config: Config) -> Result<Arc<Self>> {
        let total_capacity = config.browser_pool_size * config.tabs_per_process;
        let semaphore = Arc::new(Semaphore::new(total_capacity));

        let mut processes = Vec::new();

        for i in 0..config.browser_pool_size {
            let port = config.cdp_port_base + i as u16;
            let process = BrowserProcess::spawn(i, port, &config).await?;
            processes.push(process);
        }

        let mut available_contexts = VecDeque::new();
        for process_idx in 0..config.browser_pool_size {
            for _tab_idx in 0..config.tabs_per_process {
                available_contexts.push_back(BrowserContext {
                    context_id: uuid::Uuid::new_v4().to_string(),
                    process_index: process_idx,
                    port: config.cdp_port_base + process_idx as u16,
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
        });

        pool.spawn_health_monitor();
        Ok(pool)
    }

    pub async fn acquire(&self) -> Result<BrowserContext> {
        let _permit = self.semaphore.acquire().await
            .map_err(|_| PoolError::CapacityExhausted)?;

        let mut contexts = self.available_contexts.write().await;

        if contexts.is_empty() {
            return Err(PoolError::CapacityExhausted);
        }

        let mut context = contexts.pop_front().ok_or(PoolError::CapacityExhausted)?;

        let processes = self.processes.read().await;
        if let Some(process) = processes.get(context.process_index) {
            context.port = process.port;
        }

        self.active_contexts
            .write()
            .await
            .insert(context.context_id.clone(), context.clone());

        Ok(context)
    }

    pub async fn release(&self, mut context: BrowserContext) -> Result<()> {
        if let Some(session) = &context.cdp_session {
            let _ = session.close().await;
        }
        context.cdp_session = None;

        self.active_contexts
            .write()
            .await
            .remove(&context.context_id);

        self.available_contexts
            .write()
            .await
            .push_back(context);

        self.semaphore.add_permits(1);
        Ok(())
    }

    pub async fn new_page(&self, context: &mut BrowserContext, url: &str, _proxy: Option<String>) -> Result<Page> {
        let ws_url = format!("ws://localhost:{}/devtools/page/dummy", context.port);

        let cdp_session = CdpSession::connect(ws_url, "target_id".to_string()).await?;
        cdp_session.navigate(url).await?;
        cdp_session.set_viewport(1920, 1080).await?;
        cdp_session.wait_for_navigation(self.config.load_timeout).await?;

        context.cdp_session = Some(cdp_session);

        Ok(Page {
            page_id: uuid::Uuid::new_v4().to_string(),
            url: url.to_string(),
            context_id: context.context_id.clone(),
        })
    }

    pub async fn get_capacity_stats(&self) -> CapacityStats {
        let available = self.available_contexts.read().await.len();
        let active = self.active_contexts.read().await.len();
        let total = available + active;

        CapacityStats {
            total,
            available,
            active,
            processes: self.config.browser_pool_size,
        }
    }

    fn spawn_health_monitor(&self) {
        let processes = Arc::clone(&self.processes);
        let config = self.config.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

                let mut procs = processes.write().await;
                for (idx, process) in procs.iter_mut().enumerate() {
                    if !process.is_alive() {
                        tracing::warn!("Process {} is dead, attempting restart", idx);
                        let _ = process.kill().await;

                        if let Ok(new_process) = BrowserProcess::spawn(idx, config.cdp_port_base + idx as u16, &config).await {
                            *process = new_process;
                        }
                    }
                }
            }
        });
    }

    pub async fn shutdown(&self) -> Result<()> {
        let mut processes = self.processes.write().await;
        for process in processes.iter_mut() {
            let _ = process.kill().await;
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

    #[tokio::test]
    async fn test_pool_creation() {
        let config = Config {
            chrome_path: "/usr/bin/google-chrome".to_string(),
            browser_pool_size: 2,
            tabs_per_process: 3,
            server_host: "0.0.0.0".to_string(),
            server_port: 8080,
            log_level: "info".to_string(),
            headless: true,
            disable_sandbox: false,
            user_agent: None,
            solve_timeout: 120,
            load_timeout: 30,
            cdp_timeout: 10,
            cdp_port_base: 9222,
        };

        let pool = BrowserPool::new(config).await;
        assert!(pool.is_ok());
    }

    #[tokio::test]
    async fn test_capacity_stats() {
        let config = Config {
            chrome_path: "/usr/bin/google-chrome".to_string(),
            browser_pool_size: 2,
            tabs_per_process: 3,
            server_host: "0.0.0.0".to_string(),
            server_port: 8080,
            log_level: "info".to_string(),
            headless: true,
            disable_sandbox: false,
            user_agent: None,
            solve_timeout: 120,
            load_timeout: 30,
            cdp_timeout: 10,
            cdp_port_base: 9222,
        };

        let pool = BrowserPool::new(config).await.unwrap();
        let stats = pool.get_capacity_stats().await;

        assert_eq!(stats.total, 6); // 2 processes * 3 tabs
        assert_eq!(stats.available, 6);
        assert_eq!(stats.active, 0);
    }
}
