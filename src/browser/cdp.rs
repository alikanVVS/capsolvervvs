use crate::error::{PoolError, Result};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async, tungstenite::Message};

static MESSAGE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_message_id() -> u64 {
    MESSAGE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

type CdpReply = std::result::Result<Value, String>;
type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<CdpReply>>>>;

/// Owns the WebSocket to Chrome. One connection is shared by every session that
/// targets the same browser process; per-target routing is done with `sessionId`.
#[derive(Debug)]
pub struct CdpConnection {
    outgoing: mpsc::UnboundedSender<Message>,
    pending: PendingMap,
    events: broadcast::Sender<Value>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl CdpConnection {
    pub async fn connect(ws_url: &str) -> Result<Arc<Self>> {
        let (ws_stream, _) = connect_async(ws_url)
            .await
            .map_err(|e| PoolError::CdpConnectionError(format!("{}: {}", ws_url, e)))?;

        let (mut write, mut read) = ws_stream.split();
        let (outgoing, mut rx) = mpsc::unbounded_channel::<Message>();
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (events, _) = broadcast::channel(512);

        let write_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let read_pending = Arc::clone(&pending);
        let read_events = events.clone();
        let read_task = tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                let text = match msg {
                    Ok(Message::Text(text)) => text,
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(_) => continue,
                };

                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                match value.get("id").and_then(Value::as_u64) {
                    Some(id) => {
                        if let Some(tx) = read_pending.lock().await.remove(&id) {
                            let reply = match value.get("error") {
                                Some(err) => Err(err
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("unknown CDP error")
                                    .to_string()),
                                None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
                            };
                            let _ = tx.send(reply);
                        }
                    }
                    // Anything without an id and with a method is a protocol event.
                    None if value.get("method").is_some() => {
                        let _ = read_events.send(value);
                    }
                    None => {}
                }
            }

            // Wake every caller instead of letting them all sit until their timeout.
            for (_, tx) in read_pending.lock().await.drain() {
                let _ = tx.send(Err("CDP connection closed".to_string()));
            }
        });

        Ok(Arc::new(CdpConnection {
            outgoing,
            pending,
            events,
            tasks: Mutex::new(vec![write_task, read_task]),
        }))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }

    pub async fn call(
        &self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
        timeout: Duration,
    ) -> Result<Value> {
        let id = next_message_id();
        let (tx, rx) = oneshot::channel();

        // Register before sending: a fast reply must not arrive before we can receive it.
        self.pending.lock().await.insert(id, tx);

        let mut message = json!({ "id": id, "method": method, "params": params });
        if let Some(session_id) = session_id {
            message["sessionId"] = json!(session_id);
        }

        if self.outgoing.send(Message::Text(message.to_string())).is_err() {
            self.pending.lock().await.remove(&id);
            return Err(PoolError::CdpConnectionError(
                "CDP writer task is gone".to_string(),
            ));
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(message))) => Err(PoolError::CdpProtocolError(format!(
                "{}: {}",
                method, message
            ))),
            Ok(Err(_)) => Err(PoolError::CdpConnectionError(format!(
                "{}: reply channel dropped",
                method
            ))),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(PoolError::Timeout(method.to_string()))
            }
        }
    }

    pub async fn close(&self) {
        for task in self.tasks.lock().await.drain(..) {
            task.abort();
        }
    }
}

/// A CDP session. Without a `session_id` it addresses the browser endpoint;
/// with one it addresses a single attached page target.
#[derive(Debug)]
pub struct CdpSession {
    connection: Arc<CdpConnection>,
    session_id: Option<String>,
    target_id: Option<String>,
    timeout: Duration,
}

impl CdpSession {
    /// Opens a new browser-level connection.
    pub async fn connect(ws_url: String, timeout: Duration) -> Result<Arc<Self>> {
        let connection = CdpConnection::connect(&ws_url).await?;
        Ok(Arc::new(CdpSession {
            connection,
            session_id: None,
            target_id: None,
            timeout,
        }))
    }

    /// Wraps an already-attached target on an existing connection.
    pub fn from_attached(
        connection: Arc<CdpConnection>,
        session_id: String,
        target_id: String,
        timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(CdpSession {
            connection,
            session_id: Some(session_id),
            target_id: Some(target_id),
            timeout,
        })
    }

    pub fn connection(&self) -> &Arc<CdpConnection> {
        &self.connection
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn target_id(&self) -> Option<&str> {
        self.target_id.as_deref()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.connection.subscribe()
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.connection
            .call(method, params, self.session_id.as_deref(), self.timeout)
            .await
    }

    pub async fn call_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.connection
            .call(method, params, self.session_id.as_deref(), timeout)
            .await
    }

    /// Page/Runtime/Network must be enabled before their commands and events work.
    pub async fn enable_domains(&self) -> Result<()> {
        self.call("Page.enable", json!({})).await?;
        self.call("Runtime.enable", json!({})).await?;
        self.call("Network.enable", json!({})).await?;
        Ok(())
    }

    pub async fn navigate(&self, url: &str) -> Result<String> {
        let result = self.call("Page.navigate", json!({ "url": url })).await?;

        // Page.navigate reports failures in the payload rather than as a protocol error.
        if let Some(error_text) = result.get("errorText").and_then(Value::as_str) {
            if !error_text.is_empty() {
                return Err(PoolError::NavigationFailed(format!(
                    "{}: {}",
                    url, error_text
                )));
            }
        }

        Ok(url.to_string())
    }

    pub async fn evaluate_script(&self, script: &str) -> Result<Value> {
        let result = self
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": script,
                    "returnByValue": true,
                    // Without this a promise resolves to a handle rather than its value.
                    "awaitPromise": true,
                    "userGesture": true,
                }),
            )
            .await?;

        if let Some(details) = result.get("exceptionDetails") {
            let message = details
                .get("exception")
                .and_then(|e| e.get("description"))
                .and_then(Value::as_str)
                .or_else(|| details.get("text").and_then(Value::as_str))
                .unwrap_or("script threw");
            return Err(PoolError::ScriptExecutionFailed(message.to_string()));
        }

        Ok(result)
    }

    /// Adds a script that runs before any page script on every subsequent navigation.
    pub async fn add_init_script(&self, script: &str) -> Result<()> {
        self.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": script }),
        )
        .await?;
        Ok(())
    }

    pub async fn query_selector(&self, selector: &str) -> Result<Option<String>> {
        // Encode as a JS string literal so a selector cannot break out into code.
        let script = format!(
            "document.querySelector({})?.outerHTML ?? null",
            serde_json::to_string(selector)?
        );

        let response = self.evaluate_script(&script).await?;
        Ok(response
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(Value::as_str)
            .map(str::to_string))
    }

    pub async fn get_cookies(&self) -> Result<Vec<Value>> {
        let result = self.call("Network.getAllCookies", json!({})).await?;
        Ok(result
            .get("cookies")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub async fn get_cookies_for(&self, urls: &[&str]) -> Result<Vec<Value>> {
        let result = self
            .call("Network.getCookies", json!({ "urls": urls }))
            .await?;
        Ok(result
            .get("cookies")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub async fn set_viewport(&self, width: u32, height: u32) -> Result<()> {
        self.call(
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": width,
                "height": height,
                "deviceScaleFactor": 1,
                "mobile": false,
            }),
        )
        .await?;
        Ok(())
    }

    pub async fn user_agent(&self) -> Result<String> {
        let response = self.evaluate_script("navigator.userAgent").await?;
        response
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                PoolError::ScriptExecutionFailed("navigator.userAgent was not a string".to_string())
            })
    }

    /// Pauses only the requests matching `patterns` so the rest of the page loads normally.
    pub async fn enable_fetch(&self, patterns: Value) -> Result<()> {
        self.call("Fetch.enable", json!({ "patterns": patterns }))
            .await?;
        Ok(())
    }

    pub async fn disable_fetch(&self) -> Result<()> {
        self.call("Fetch.disable", json!({})).await?;
        Ok(())
    }

    pub async fn fulfill_request(
        &self,
        request_id: &str,
        body: &str,
        content_type: &str,
    ) -> Result<()> {
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(body);

        self.call(
            "Fetch.fulfillRequest",
            json!({
                "requestId": request_id,
                "responseCode": 200,
                "responseHeaders": [{ "name": "Content-Type", "value": content_type }],
                "body": encoded,
            }),
        )
        .await?;
        Ok(())
    }

    pub async fn continue_request(&self, request_id: &str) -> Result<()> {
        self.call("Fetch.continueRequest", json!({ "requestId": request_id }))
            .await?;
        Ok(())
    }

    /// Polls `document.readyState` rather than racing a load event that may already have fired.
    pub async fn wait_for_navigation(&self, timeout: Duration) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if let Ok(response) = self.evaluate_script("document.readyState").await {
                let state = response
                    .get("result")
                    .and_then(|r| r.get("value"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();

                if state == "complete" || state == "interactive" {
                    return Ok(());
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(PoolError::Timeout("wait_for_navigation".to_string()));
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Detaches this page session. The browser connection stays usable.
    pub async fn close(&self) -> Result<()> {
        match &self.session_id {
            Some(session_id) => {
                let _ = self
                    .connection
                    .call(
                        "Target.detachFromTarget",
                        json!({ "sessionId": session_id }),
                        None,
                        self.timeout,
                    )
                    .await;
            }
            None => self.connection.close().await,
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

    /// The selector is interpolated into a script, so it must land as a single
    /// double-quoted JS literal with no way to close it early.
    #[test]
    fn test_selector_is_encoded_as_js_literal() {
        for hostile in [
            r#"a") ; alert(1); //"#,
            r#"a\") ; alert(1); //"#,
            "a\n) ; alert(1); //",
        ] {
            let encoded = serde_json::to_string(hostile).unwrap();

            assert!(encoded.starts_with('"') && encoded.ends_with('"'));
            assert!(
                !encoded.contains('\n'),
                "newlines must be escaped, not literal: {}",
                encoded
            );

            // The only unescaped quotes are the delimiters themselves.
            let unescaped_quotes = encoded
                .char_indices()
                .filter(|(i, c)| {
                    *c == '"'
                        && encoded[..*i].chars().rev().take_while(|p| *p == '\\').count() % 2 == 0
                })
                .count();
            assert_eq!(unescaped_quotes, 2, "literal was breakable: {}", encoded);
        }
    }
}
