//! End-to-end checks for the CDP transport, against a real browser.
//!
//! These are `#[ignore]`d because they need a Chrome binary. Point `CHROME_PATH`
//! at one and run `cargo test --test cdp_integration_tests -- --ignored`.
//!
//! They exist because the transport previously dropped the socket's write half and
//! silently discarded every command: the code compiled, unit tests passed, and
//! nothing worked at runtime. Each test here asserts on a value that can only come
//! back from the browser.

use capsolver::browser::BrowserPool;
use capsolver::config::Config;
use serde_json::Value;

fn test_config() -> Config {
    Config {
        chrome_path: std::env::var("CHROME_PATH")
            .unwrap_or_else(|_| "/usr/bin/google-chrome".to_string()),
        browser_pool_size: 1,
        tabs_per_process: 2,
        server_host: "127.0.0.1".to_string(),
        server_port: 0,
        log_level: "debug".to_string(),
        headless: true,
        disable_sandbox: true,
        user_agent: None,
        solve_timeout_ms: 29_000,
        load_timeout_ms: 30_000,
        cdp_timeout_ms: 10_000,
        startup_timeout_ms: 20_000,
        request_timeout_ms: 60_000,
        cdp_port_base: 9400,
    }
}

fn script_value(response: &Value) -> Option<&Value> {
    response.get("result").and_then(|r| r.get("value"))
}

#[ignore]
#[tokio::test]
async fn test_evaluate_script_returns_a_real_value() {
    let pool = BrowserPool::new(test_config()).await.expect("pool");
    let mut context = pool.acquire().await.expect("acquire");
    pool.new_page(&mut context, "about:blank", None)
        .await
        .expect("new_page");

    let session = context.cdp_session.clone().expect("session");

    let response = session.evaluate_script("6 * 7").await.expect("evaluate");
    assert_eq!(
        script_value(&response).and_then(Value::as_i64),
        Some(42),
        "the browser must actually compute this"
    );

    // A promise must be awaited rather than returned as a handle.
    let response = session
        .evaluate_script("Promise.resolve('resolved')")
        .await
        .expect("evaluate promise");
    assert_eq!(
        script_value(&response).and_then(Value::as_str),
        Some("resolved")
    );

    pool.release(context).await.ok();
    pool.shutdown().await.ok();
}

#[ignore]
#[tokio::test]
async fn test_script_exception_is_reported_as_an_error() {
    let pool = BrowserPool::new(test_config()).await.expect("pool");
    let mut context = pool.acquire().await.expect("acquire");
    pool.new_page(&mut context, "about:blank", None)
        .await
        .expect("new_page");

    let session = context.cdp_session.clone().expect("session");
    let result = session.evaluate_script("throw new Error('boom')").await;

    let Err(error) = result else {
        panic!("a thrown exception must surface as Err, not a silent success");
    };
    assert!(error.to_string().contains("boom"), "got: {}", error);

    pool.release(context).await.ok();
    pool.shutdown().await.ok();
}

#[ignore]
#[tokio::test]
async fn test_user_agent_and_cookies_round_trip() {
    let pool = BrowserPool::new(test_config()).await.expect("pool");
    let mut context = pool.acquire().await.expect("acquire");
    pool.new_page(&mut context, "about:blank", None)
        .await
        .expect("new_page");

    let session = context.cdp_session.clone().expect("session");

    let user_agent = session.user_agent().await.expect("user agent");
    assert!(
        user_agent.contains("Mozilla"),
        "expected a browser UA, got: {}",
        user_agent
    );

    // Network.getAllCookies only answers once the Network domain is enabled.
    session.get_cookies().await.expect("cookies");

    pool.release(context).await.ok();
    pool.shutdown().await.ok();
}

/// The Turnstile solver serves its stub by intercepting the document request, so the
/// widget sees the real origin instead of the `null` a `data:` URL would give it.
/// Interception happens before the request leaves the browser, so this needs no network.
#[ignore]
#[tokio::test]
async fn test_fetch_interception_serves_html_at_the_target_origin() {
    use serde_json::json;

    let pool = BrowserPool::new(test_config()).await.expect("pool");
    let mut context = pool.acquire().await.expect("acquire");
    pool.new_page(&mut context, "about:blank", None)
        .await
        .expect("new_page");

    let session = context.cdp_session.clone().expect("session");
    let target = "https://intercepted.example/";
    let html = "<!DOCTYPE html><html><body><div id=\"marker\">served</div></body></html>";

    session
        .enable_fetch(json!([{ "urlPattern": target, "requestStage": "Request" }]))
        .await
        .expect("Fetch.enable");

    let mut events = session.subscribe();
    let interceptor = {
        let session = session.clone();
        tokio::spawn(async move {
            while let Ok(event) = events.recv().await {
                if event.get("method").and_then(Value::as_str) != Some("Fetch.requestPaused") {
                    continue;
                }
                let Some(request_id) = event
                    .get("params")
                    .and_then(|p| p.get("requestId"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                let _ = session
                    .fulfill_request(request_id, html, "text/html; charset=utf-8")
                    .await;
            }
        })
    };

    session.navigate(target).await.expect("navigate");
    session
        .wait_for_navigation(std::time::Duration::from_secs(20))
        .await
        .expect("load");

    let response = session
        .evaluate_script("document.getElementById('marker')?.textContent ?? null")
        .await
        .expect("evaluate");
    assert_eq!(
        script_value(&response).and_then(Value::as_str),
        Some("served"),
        "the intercepted response should have been rendered"
    );

    // The page must believe it is on the target origin, which is what Turnstile checks.
    let response = session
        .evaluate_script("window.location.origin")
        .await
        .expect("evaluate origin");
    assert_eq!(
        script_value(&response).and_then(Value::as_str),
        Some("https://intercepted.example"),
        "origin must be the target, not null"
    );

    interceptor.abort();
    session.disable_fetch().await.ok();
    pool.release(context).await.ok();
    pool.shutdown().await.ok();
}

/// Each acquire gets a fresh browser context, so cookies must not leak between solves.
#[ignore]
#[tokio::test]
async fn test_browser_contexts_are_isolated() {
    let pool = BrowserPool::new(test_config()).await.expect("pool");

    let mut first = pool.acquire().await.expect("acquire first");
    pool.new_page(&mut first, "about:blank", None)
        .await
        .expect("new_page");
    let first_context_id = first.browser_context_id.clone();
    assert!(
        first_context_id.is_some(),
        "new_page must create a real browser context"
    );
    pool.release(first).await.expect("release first");

    let mut second = pool.acquire().await.expect("acquire second");
    pool.new_page(&mut second, "about:blank", None)
        .await
        .expect("new_page");

    assert_ne!(
        first_context_id, second.browser_context_id,
        "a released slot must not reuse the disposed browser context"
    );

    pool.release(second).await.expect("release second");
    pool.shutdown().await.ok();
}
