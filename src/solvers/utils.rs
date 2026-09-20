use super::error::{Result, SolverError};

pub struct StubPageBuilder {
    sitekey: String,
    cdata: Option<String>,
    action: Option<String>,
}

impl StubPageBuilder {
    pub fn new(sitekey: String) -> Self {
        StubPageBuilder {
            sitekey,
            cdata: None,
            action: None,
        }
    }

    pub fn with_cdata(mut self, cdata: String) -> Self {
        self.cdata = Some(cdata);
        self
    }

    pub fn with_action(mut self, action: String) -> Self {
        self.action = Some(action);
        self
    }

    pub fn build(self) -> String {
        let cdata_attr = self
            .cdata
            .as_ref()
            .map(|c| format!(" data-cdata=\"{}\"", escape_html(c)))
            .unwrap_or_default();

        let action_attr = self
            .action
            .as_ref()
            .map(|a| format!(" data-action=\"{}\"", escape_html(a)))
            .unwrap_or_default();

        let sitekey = escape_html(&self.sitekey);

        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Turnstile</title>
    <script src="https://challenges.cloudflare.com/turnstile/v0/api.js" async defer></script>
</head>
<body>
    <div class="cf-turnstile"
         data-sitekey="{sitekey}"{cdata_attr}{action_attr}
         data-callback="onTurnstileSuccess"
         data-error-callback="onTurnstileError"
         data-expired-callback="onTurnstileExpire"></div>

    <script>
        window.turnstileToken = null;
        window.turnstileError = null;

        function onTurnstileSuccess(token) {{
            window.turnstileToken = token;
        }}

        function onTurnstileError(errorCode) {{
            window.turnstileError = String(errorCode);
        }}

        function onTurnstileExpire() {{
            window.turnstileToken = null;
        }}

        window.getTurnstileToken = function() {{
            return window.turnstileToken;
        }};

        window.getTurnstileError = function() {{
            return window.turnstileError;
        }};
    </script>
</body>
</html>"#
        )
    }
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn validate_sitekey(sitekey: &str) -> Result<()> {
    if sitekey.is_empty() {
        return Err(SolverError::InvalidSitekey("Sitekey is empty".to_string()));
    }

    if sitekey.len() < 20 {
        return Err(SolverError::InvalidSitekey(
            "Sitekey seems invalid (too short)".to_string(),
        ));
    }

    if !sitekey
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(SolverError::InvalidSitekey(
            "Sitekey contains invalid characters".to_string(),
        ));
    }

    Ok(())
}

pub fn validate_url(url: &str) -> Result<()> {
    if url.is_empty() {
        return Err(SolverError::ConfigError("URL is empty".to_string()));
    }

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(SolverError::ConfigError(
            "URL must start with http:// or https://".to_string(),
        ));
    }

    Ok(())
}

/// Scheme + host + port, used to scope cookie lookups and request interception.
pub fn origin_of(url: &str) -> Result<String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| SolverError::ConfigError(format!("URL has no scheme: {}", url)))?;

    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();

    if authority.is_empty() {
        return Err(SolverError::ConfigError(format!(
            "URL has no host: {}",
            url
        )));
    }

    Ok(format!("{}://{}", scheme, authority))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_page_builder() {
        let page = StubPageBuilder::new("1x00000000000000000000AA".to_string()).build();

        assert!(page.contains("cf-turnstile"));
        assert!(page.contains("1x00000000000000000000AA"));
        assert!(page.contains("onTurnstileSuccess"));
    }

    #[test]
    fn test_stub_page_with_cdata() {
        let page = StubPageBuilder::new("1x00000000000000000000AA".to_string())
            .with_cdata("test_cdata".to_string())
            .build();

        assert!(page.contains("data-cdata=\"test_cdata\""));
    }

    #[test]
    fn test_stub_page_with_action() {
        let page = StubPageBuilder::new("1x00000000000000000000AA".to_string())
            .with_action("test_action".to_string())
            .build();

        assert!(page.contains("data-action=\"test_action\""));
    }

    #[test]
    fn test_stub_page_registers_error_callbacks() {
        let page = StubPageBuilder::new("1x00000000000000000000AA".to_string()).build();

        assert!(page.contains("data-error-callback=\"onTurnstileError\""));
        assert!(page.contains("data-expired-callback=\"onTurnstileExpire\""));
    }

    #[test]
    fn test_validate_sitekey_valid() {
        assert!(validate_sitekey("1x00000000000000000000AA").is_ok());
    }

    #[test]
    fn test_validate_sitekey_empty() {
        assert!(validate_sitekey("").is_err());
    }

    #[test]
    fn test_validate_sitekey_too_short() {
        assert!(validate_sitekey("123").is_err());
    }

    #[test]
    fn test_validate_url_valid() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://example.com").is_ok());
    }

    #[test]
    fn test_validate_url_invalid() {
        assert!(validate_url("").is_err());
        assert!(validate_url("example.com").is_err());
    }

    #[test]
    fn test_origin_of() {
        assert_eq!(
            origin_of("https://example.com/a/b?c=d").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            origin_of("http://example.com:8080/x").unwrap(),
            "http://example.com:8080"
        );
        assert!(origin_of("example.com").is_err());
    }
}
