use super::error::{Result, SolverError};

pub struct StubPageBuilder {
    sitekey: String,
    url: String,
    cdata: Option<String>,
    action: Option<String>,
}

impl StubPageBuilder {
    pub fn new(sitekey: String, url: String) -> Self {
        StubPageBuilder {
            sitekey,
            url,
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
            .map(|c| format!(" data-cData=\"{}\"", escape_html(c)))
            .unwrap_or_default();

        let action_attr = self
            .action
            .as_ref()
            .map(|a| format!(" data-action=\"{}\"", escape_html(a)))
            .unwrap_or_default();

        let sitekey = &self.sitekey;

        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Turnstile Solver</title>
    <script src="https://challenges.cloudflare.com/turnstile/v0/api.js" async defer></script>
    <style>
        body {{
            margin: 0;
            padding: 20px;
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
            background-color: #f5f5f5;
        }}
        .container {{
            max-width: 600px;
            margin: 0 auto;
            background: white;
            padding: 40px;
            border-radius: 8px;
            box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
        }}
        h1 {{
            text-align: center;
            color: #333;
            margin-top: 0;
        }}
        .turnstile-container {{
            display: flex;
            justify-content: center;
            margin: 40px 0;
        }}
        .status {{
            text-align: center;
            padding: 10px;
            margin-top: 20px;
            border-radius: 4px;
            font-size: 14px;
        }}
        .status.waiting {{
            background-color: #e3f2fd;
            color: #1976d2;
        }}
        .status.success {{
            background-color: #e8f5e9;
            color: #388e3c;
        }}
        .status.error {{
            background-color: #ffebee;
            color: #d32f2f;
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>Cloudflare Turnstile</h1>
        <div class="turnstile-container">
            <div class="cf-turnstile"{cdata_attr}{action_attr} data-sitekey="{sitekey}" data-callback="onTurnstileSuccess"></div>
        </div>
        <div id="status" class="status waiting">Waiting for token...</div>
    </div>

    <script>
        window.turnstileToken = null;
        window.turnstileError = null;

        function onTurnstileSuccess(token) {{
            window.turnstileToken = token;
            document.getElementById('status').textContent = 'Token obtained: ' + token.substring(0, 20) + '...';
            document.getElementById('status').className = 'status success';
        }}

        function onTurnstileError(errorCode) {{
            window.turnstileError = errorCode;
            document.getElementById('status').textContent = 'Turnstile error: ' + errorCode;
            document.getElementById('status').className = 'status error';
        }}

        function onTurnstileExpire() {{
            window.turnstileToken = null;
            document.getElementById('status').textContent = 'Token expired';
            document.getElementById('status').className = 'status error';
        }}

        window.checkTurnstileReady = function() {{
            return window.turnstile !== undefined && window.turnstile.isReady !== undefined;
        }};

        window.getTurnstileToken = function() {{
            return window.turnstileToken;
        }};

        window.getTurnstileError = function() {{
            return window.turnstileError;
        }};

        window.resetTurnstile = function() {{
            if (window.turnstile && window.turnstile.reset) {{
                window.turnstile.reset();
                window.turnstileToken = null;
                window.turnstileError = null;
            }}
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

    if !sitekey.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_page_builder() {
        let page = StubPageBuilder::new(
            "1x00000000000000000000AA".to_string(),
            "https://example.com".to_string(),
        )
        .build();

        assert!(page.contains("cf-turnstile"));
        assert!(page.contains("1x00000000000000000000AA"));
        assert!(page.contains("onTurnstileSuccess"));
    }

    #[test]
    fn test_stub_page_with_cdata() {
        let page = StubPageBuilder::new(
            "1x00000000000000000000AA".to_string(),
            "https://example.com".to_string(),
        )
        .with_cdata("test_cdata".to_string())
        .build();

        assert!(page.contains("data-cData=\"test_cdata\""));
    }

    #[test]
    fn test_stub_page_with_action() {
        let page = StubPageBuilder::new(
            "1x00000000000000000000AA".to_string(),
            "https://example.com".to_string(),
        )
        .with_action("test_action".to_string())
        .build();

        assert!(page.contains("data-action=\"test_action\""));
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
}
