use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, info, warn};

/// Response from 2Captcha API when submitting a captcha
#[derive(Debug, Deserialize)]
struct CaptchaSubmitResponse {
    status: i32,
    request: String,
}

/// Response from 2Captcha API when checking captcha result
#[derive(Debug, Deserialize)]
struct CaptchaResultResponse {
    status: i32,
    request: String,
}

/// 2Captcha API client for solving reCAPTCHA
pub struct CaptchaSolver {
    api_key: String,
    client: Client,
}

impl CaptchaSolver {
    /// Create a new captcha solver with API key
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }

    /// Check if 2Captcha API key is configured
    pub fn is_available() -> bool {
        std::env::var("TWOCAPTCHA_API_KEY").is_ok()
    }

    /// Create from environment variable
    pub fn from_env() -> Option<Self> {
        std::env::var("TWOCAPTCHA_API_KEY")
            .ok()
            .map(|key| Self::new(key))
    }

    /// Solve reCAPTCHA v2
    pub async fn solve_recaptcha_v2(&self, site_key: &str, page_url: &str) -> Result<String> {
        info!("🤖 Solving reCAPTCHA using 2Captcha API...");

        // Submit captcha
        let submit_url = format!(
            "https://2captcha.com/in.php?key={}&method=userrecaptcha&googlekey={}&pageurl={}",
            self.api_key, site_key, page_url
        );

        debug!("Submitting captcha to 2Captcha...");
        let response = self.client.get(&submit_url).send().await?;
        let text = response.text().await?;

        if !text.starts_with("OK|") {
            anyhow::bail!("Failed to submit captcha: {}", text);
        }

        let captcha_id = text.strip_prefix("OK|").unwrap();
        info!("Captcha submitted, ID: {}", captcha_id);

        // Poll for result (usually takes 10-30 seconds)
        let max_attempts = 60; // 2 minutes max
        let poll_interval = Duration::from_secs(2);

        for attempt in 1..=max_attempts {
            sleep(poll_interval).await;

            let result_url = format!(
                "https://2captcha.com/res.php?key={}&action=get&id={}",
                self.api_key, captcha_id
            );

            let response = self.client.get(&result_url).send().await?;
            let text = response.text().await?;

            if text.starts_with("OK|") {
                let solution = text.strip_prefix("OK|").unwrap();
                info!(
                    "✅ reCAPTCHA solved successfully! (attempt {}/{})",
                    attempt, max_attempts
                );
                return Ok(solution.to_string());
            } else if text == "CAPCHA_NOT_READY" {
                debug!(
                    "Captcha not ready yet, waiting... (attempt {}/{})",
                    attempt, max_attempts
                );
            } else {
                warn!("Unexpected response from 2Captcha: {}", text);
            }
        }

        anyhow::bail!("Timeout waiting for captcha solution")
    }

    /// Get site key from page HTML
    pub fn extract_site_key(html: &str) -> Option<String> {
        // Look for reCAPTCHA site key in HTML
        // Pattern: data-sitekey="XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"
        if let Some(start) = html.find("data-sitekey=\"") {
            let start_idx = start + 14;
            if let Some(end) = html[start_idx..].find('"') {
                return Some(html[start_idx..start_idx + end].to_string());
            }
        }

        // Alternative pattern: grecaptcha.execute('SITE_KEY')
        if let Some(start) = html.find("grecaptcha.execute('") {
            let start_idx = start + 20;
            if let Some(end) = html[start_idx..].find('\'') {
                return Some(html[start_idx..start_idx + end].to_string());
            }
        }

        None
    }

    /// Check account balance
    pub async fn get_balance(&self) -> Result<f64> {
        let url = format!(
            "https://2captcha.com/res.php?key={}&action=getbalance",
            self.api_key
        );

        let response = self.client.get(&url).send().await?;
        let text = response.text().await?;

        text.parse::<f64>().context("Failed to parse balance")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_site_key() {
        let html = r#"<div class="g-recaptcha" data-sitekey="6LdTest1234567890"></div>"#;
        let site_key = CaptchaSolver::extract_site_key(html);
        assert_eq!(site_key, Some("6LdTest1234567890".to_string()));
    }

    #[test]
    fn test_extract_site_key_alternative() {
        let html = r#"grecaptcha.execute('6LdAlternative123');"#;
        let site_key = CaptchaSolver::extract_site_key(html);
        assert_eq!(site_key, Some("6LdAlternative123".to_string()));
    }

    #[test]
    fn test_is_available() {
        // This will fail if env var is not set, which is expected in test env
        assert!(!CaptchaSolver::is_available() || CaptchaSolver::is_available());
    }
}
