use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thirtyfour::prelude::*;
use tracing::{debug, info};

/// Represents a browser cookie for session persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieData {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub secure: bool,
    pub http_only: bool,
}

/// Session manager for persisting and restoring browser sessions
pub struct SessionManager {
    session_file: PathBuf,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new() -> Self {
        let session_file = PathBuf::from("dbase_session.json");
        Self { session_file }
    }

    /// Save cookies from current browser session
    pub async fn save_session(&self, driver: &WebDriver) -> Result<()> {
        info!("Saving session cookies...");

        let cookies = driver.get_all_cookies().await?;

        let cookie_data: Vec<CookieData> = cookies
            .iter()
            .map(|cookie| CookieData {
                name: cookie.name().to_string(),
                value: cookie.value().to_string(),
                domain: cookie.domain().map(|s| s.to_string()),
                path: cookie.path().map(|s| s.to_string()),
                secure: cookie.secure().unwrap_or(false),
                http_only: cookie.http_only().unwrap_or(false),
            })
            .collect();

        let json = serde_json::to_string_pretty(&cookie_data)?;
        fs::write(&self.session_file, json).context("Failed to write session file")?;

        info!(
            "✅ Saved {} cookies to {:?}",
            cookie_data.len(),
            self.session_file
        );
        Ok(())
    }

    /// Load cookies from saved session
    pub async fn load_session(&self, driver: &WebDriver) -> Result<bool> {
        if !self.session_file.exists() {
            debug!("No saved session found at {:?}", self.session_file);
            return Ok(false);
        }

        info!("Loading saved session from {:?}...", self.session_file);

        let json = fs::read_to_string(&self.session_file).context("Failed to read session file")?;

        let cookie_data: Vec<CookieData> =
            serde_json::from_str(&json).context("Failed to parse session file")?;

        // Navigate to domain first (required for setting cookies)
        driver.goto("https://app.dbase.com.br").await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Add each cookie
        for cookie_data in cookie_data {
            // Build cookie with required fields
            let mut cookie_builder =
                Cookie::new(cookie_data.name.clone(), cookie_data.value.clone());

            // Add optional fields if present
            if let Some(ref domain) = cookie_data.domain {
                cookie_builder.set_domain(domain.clone());
            }
            if let Some(ref path) = cookie_data.path {
                cookie_builder.set_path(path.clone());
            }
            cookie_builder.set_secure(cookie_data.secure);
            cookie_builder.set_http_only(cookie_data.http_only);

            if let Err(e) = driver.add_cookie(cookie_builder).await {
                debug!("Failed to add cookie {}: {}", cookie_data.name, e);
            }
        }

        info!("✅ Loaded session");
        Ok(true)
    }

    /// Check if session is still valid
    pub async fn is_session_valid(&self, driver: &WebDriver) -> Result<bool> {
        // Navigate to the app and check if we're logged in
        driver
            .goto("https://app.dbase.com.br/sistema/consultas/")
            .await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

        // Check if we can find the CEP search form (indicates logged in)
        let is_valid = driver.find(By::Css("input[name='e_cep']")).await.is_ok();

        if is_valid {
            info!("✅ Saved session is still valid!");
        } else {
            info!("⚠️  Saved session has expired");
        }

        Ok(is_valid)
    }

    /// Clear saved session
    pub fn clear_session(&self) -> Result<()> {
        if self.session_file.exists() {
            fs::remove_file(&self.session_file).context("Failed to delete session file")?;
            info!("🗑️  Cleared saved session");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_manager_new() {
        let manager = SessionManager::new();
        assert_eq!(manager.session_file, PathBuf::from("dbase_session.json"));
    }
}
