use crate::error::{PoolError, Result};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures::StreamExt;
use std::collections::HashMap;

static MESSAGE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_message_id() -> u64 {
    MESSAGE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug)]
pub struct CdpSession {
    ws_url: String,
    session_id: String,
    target_id: String,
    message_id: Arc<AtomicU64>,
    pending_responses: Arc<RwLock<HashMap<u64, tokio::sync::oneshot::Sender<Value>>>>,
    send_task: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
}

impl CdpSession {
    pub async fn connect(ws_url: String, target_id: String) -> Result<Arc<Self>> {
        let session = Arc::new(CdpSession {
            ws_url: ws_url.clone(),
            session_id: uuid::Uuid::new_v4().to_string(),
            target_id: target_id.clone(),
            message_id: Arc::new(AtomicU64::new(1)),
            pending_responses: Arc::new(RwLock::new(HashMap::new())),
            send_task: Arc::new(RwLock::new(None)),
        });

        session.create_target_session().await?;
        Ok(session)
    }

    async fn create_target_session(&self) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.ws_url)
            .await
            .map_err(|e| PoolError::CdpConnectionError(e.to_string()))?;

        let (_write, mut read) = ws_stream.split();

        let pending = Arc::clone(&self.pending_responses);

        let read_task = tokio::spawn(async move {
            while let Some(msg_result) = read.next().await {
                match msg_result {
                    Ok(Message::Text(text)) => {
                        if let Ok(value) = serde_json::from_str::<Value>(&text) {
                            if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
                                let mut pending_lock = pending.write().await;
                                if let Some(tx) = pending_lock.remove(&id) {
                                    let _ = tx.send(value.clone());
                                }
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        });

        let _ = self.send_task.write().await.insert(read_task);
        Ok(())
    }

    pub async fn navigate(&self, url: &str) -> Result<String> {
        let id = next_message_id();
        let command = json!({
            "id": id,
            "method": "Page.navigate",
            "params": {
                "url": url
            }
        });

        self.send_cdp_command(&command).await?;

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_responses.write().await.insert(id, tx);

        tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            rx,
        )
        .await
        .map_err(|_| PoolError::Timeout("navigate".to_string()))?
        .map_err(|_| PoolError::CdpConnectionError("channel closed".to_string()))?;

        Ok(url.to_string())
    }

    pub async fn evaluate_script(&self, script: &str) -> Result<Value> {
        let id = next_message_id();
        let command = json!({
            "id": id,
            "method": "Runtime.evaluate",
            "params": {
                "expression": script,
                "returnByValue": true
            }
        });

        self.send_cdp_command(&command).await?;

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_responses.write().await.insert(id, tx);

        let response = tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            rx,
        )
        .await
        .map_err(|_| PoolError::Timeout("evaluate_script".to_string()))?
        .map_err(|_| PoolError::CdpConnectionError("channel closed".to_string()))?;

        Ok(response)
    }

    pub async fn query_selector(&self, selector: &str) -> Result<Option<String>> {
        let script = format!(
            "document.querySelector('{}')?.outerHTML",
            selector.replace('\'', "\\'")
        );

        let response = self.evaluate_script(&script).await?;

        if let Some(result) = response.get("result").and_then(|r| r.get("value")) {
            Ok(result.as_str().map(|s| s.to_string()))
        } else {
            Ok(None)
        }
    }

    pub async fn get_cookies(&self) -> Result<Vec<Value>> {
        let id = next_message_id();
        let command = json!({
            "id": id,
            "method": "Network.getAllCookies"
        });

        self.send_cdp_command(&command).await?;

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_responses.write().await.insert(id, tx);

        let response = tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            rx,
        )
        .await
        .map_err(|_| PoolError::Timeout("get_cookies".to_string()))?
        .map_err(|_| PoolError::CdpConnectionError("channel closed".to_string()))?;

        if let Some(cookies) = response.get("result").and_then(|r| r.get("cookies")).and_then(|c| c.as_array()) {
            Ok(cookies.clone())
        } else {
            Ok(vec![])
        }
    }

    pub async fn set_viewport(&self, width: u32, height: u32) -> Result<()> {
        let id = next_message_id();
        let command = json!({
            "id": id,
            "method": "Emulation.setDeviceMetricsOverride",
            "params": {
                "width": width,
                "height": height,
                "deviceScaleFactor": 1,
                "mobile": false
            }
        });

        self.send_cdp_command(&command).await?;

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_responses.write().await.insert(id, tx);

        tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            rx,
        )
        .await
        .map_err(|_| PoolError::Timeout("set_viewport".to_string()))?
        .map_err(|_| PoolError::CdpConnectionError("channel closed".to_string()))?;

        Ok(())
    }

    pub async fn wait_for_navigation(&self, timeout_secs: u64) -> Result<()> {
        let script = r#"
            new Promise((resolve) => {
                if (document.readyState === 'complete') {
                    resolve(true);
                } else {
                    window.addEventListener('load', () => resolve(true));
                }
            })
        "#;

        tokio::time::timeout(
            tokio::time::Duration::from_secs(timeout_secs),
            self.evaluate_script(script),
        )
        .await
        .map_err(|_| PoolError::Timeout("wait_for_navigation".to_string()))??;

        Ok(())
    }

    async fn send_cdp_command(&self, _command: &Value) -> Result<()> {
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        self.pending_responses.write().await.clear();
        if let Some(handle) = self.send_task.write().await.take() {
            handle.abort();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_id_increments() {
        let id1 = next_message_id();
        let id2 = next_message_id();
        assert!(id2 > id1);
    }

    #[test]
    fn test_cdp_command_json() {
        let command = json!({
            "id": 1,
            "method": "Page.navigate",
            "params": {"url": "https://example.com"}
        });

        assert_eq!(command["method"], "Page.navigate");
        assert_eq!(command["params"]["url"], "https://example.com");
    }
}
