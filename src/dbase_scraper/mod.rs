mod captcha_solver;
mod session_manager;

use anyhow::{bail, Context, Result};
use captcha_solver::CaptchaSolver;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use session_manager::SessionManager;
use std::fs::File;
use std::time::SystemTime;
use thirtyfour::prelude::*;
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};

/// Represents an address record from DBase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressRecord {
    /// CPF/CNPJ - Brazilian tax identification number
    pub cpf_cnpj: String,
    /// Individual name or company name
    pub nome_razao_social: String,
    /// Street name
    pub logradouro: String,
    /// Street number
    pub numero: String,
    /// Address complement (apartment, suite, etc.)
    pub complemento: String,
    /// Neighborhood
    pub bairro: String,
    /// Postal code (CEP)
    pub cep: String,
}

/// DBase scraper client for dbase.com.br
pub struct DbaseScraper {
    driver: WebDriver,
    base_url: String,
    credentials: Vec<(String, String)>,
}

impl DbaseScraper {
    /// Create a new DBase scraper with multiple credentials and WebDriver URL
    pub async fn new(
        credentials: Vec<(String, String)>,
        webdriver_url: &str,
        headless: bool,
    ) -> Result<Self> {
        let mut caps = DesiredCapabilities::chrome();
        if headless {
            caps.add_chrome_arg("--headless")?;
        }
        caps.add_chrome_arg("--no-sandbox")?;
        caps.add_chrome_arg("--disable-dev-shm-usage")?;
        caps.add_chrome_arg("--disable-gpu")?;
        caps.add_chrome_arg("--window-size=1920,1080")?;

        // Add user agent to appear more like a real browser
        caps.add_chrome_arg("--user-agent=Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")?;

        let driver = WebDriver::new(webdriver_url, caps)
            .await
            .context("Failed to connect to WebDriver")?;

        Ok(Self {
            driver,
            base_url: "https://app.dbase.com.br".to_string(),
            credentials,
        })
    }

    /// Login to DBase website with credential rotation and session persistence
    pub async fn login(&self) -> Result<()> {
        info!("Logging in to DBase...");

        // Try to load saved session first
        let session_manager = SessionManager::new();

        if session_manager.load_session(&self.driver).await? {
            info!("Attempting to use saved session...");

            if session_manager.is_session_valid(&self.driver).await? {
                info!("✅ Using saved session - skipping login!");
                return Ok(());
            } else {
                info!("Saved session expired, performing fresh login...");
                session_manager.clear_session()?;
            }
        }

        // Navigate to login page
        self.driver.goto(&self.base_url).await?;
        sleep(Duration::from_secs(3)).await;

        // Try each credential until one works
        let mut login_successful = false;

        for (idx, (username, password)) in self.credentials.iter().enumerate() {
            info!("Trying credentials #{} (username: {})", idx + 1, username);

            // Find and fill username field
            let username_field = match self.driver.find(By::Name("user")).await {
                Ok(elem) => elem,
                Err(_) => match self.driver.find(By::Css("input[name='user']")).await {
                    Ok(elem) => elem,
                    Err(_) => {
                        warn!("Could not find username field, trying next credentials");
                        continue;
                    }
                },
            };

            username_field.clear().await?;
            username_field.send_keys(username).await?;
            debug!("Filled username field");

            // Find and fill password field
            let password_field = match self.driver.find(By::Name("pass")).await {
                Ok(elem) => elem,
                Err(_) => match self.driver.find(By::Css("input[name='pass']")).await {
                    Ok(elem) => elem,
                    Err(_) => {
                        warn!("Could not find password field, trying next credentials");
                        continue;
                    }
                },
            };

            password_field.clear().await?;
            password_field.send_keys(password).await?;
            debug!("Filled password field");

            // Check if 2Captcha API is available
            let captcha_solver_option = CaptchaSolver::from_env();

            if let Some(solver) = &captcha_solver_option {
                info!("🤖 2Captcha API detected, attempting automatic reCAPTCHA solving...");

                // Get page HTML to extract site key
                let html = self.driver.source().await?;

                if let Some(site_key) = CaptchaSolver::extract_site_key(&html) {
                    info!("Found reCAPTCHA site key: {}", site_key);

                    match solver.solve_recaptcha_v2(&site_key, &self.base_url).await {
                        Ok(solution) => {
                            info!("✅ Got reCAPTCHA solution, injecting into page...");

                            // Inject the solution into the g-recaptcha-response textarea
                            let inject_script = format!(
                                r#"
                                // Set the hidden textarea value
                                var textarea = document.getElementById('g-recaptcha-response');
                                if (textarea) {{
                                    textarea.innerHTML = '{}';
                                    textarea.value = '{}';
                                }}

                                // Try to trigger the callback if it exists
                                if (typeof ___grecaptcha_cfg !== 'undefined') {{
                                    for (var id in ___grecaptcha_cfg.clients) {{
                                        var client = ___grecaptcha_cfg.clients[id];
                                        if (client && typeof client.callback === 'function') {{
                                            try {{
                                                client.callback('{}');
                                            }} catch(e) {{
                                                console.log('Callback error:', e);
                                            }}
                                        }}
                                    }}
                                }}

                                // Alternative: trigger change event on textarea
                                if (textarea) {{
                                    var event = new Event('change', {{ bubbles: true }});
                                    textarea.dispatchEvent(event);
                                }}
                                "#,
                                solution, solution, solution
                            );

                            self.driver.execute(&inject_script, vec![]).await?;
                            sleep(Duration::from_secs(2)).await;

                            info!("✅ reCAPTCHA solution injected successfully!");

                            // Also try to enable the login button directly
                            let enable_button_script = r#"
                                // Find and enable the login button
                                var buttons = document.querySelectorAll("button[type='submit'], input[name='NattLogin']");
                                buttons.forEach(function(btn) {
                                    btn.disabled = false;
                                    btn.removeAttribute('disabled');
                                });
                            "#;
                            self.driver.execute(enable_button_script, vec![]).await.ok();
                            sleep(Duration::from_millis(500)).await;
                        }
                        Err(e) => {
                            warn!("Failed to solve reCAPTCHA automatically: {}", e);
                            info!("Falling back to manual reCAPTCHA completion...");
                        }
                    }
                } else {
                    debug!("No reCAPTCHA site key found in page");
                }
            }

            // Wait for user to complete reCAPTCHA if present (or verify auto-solve worked)
            if captcha_solver_option.is_none() {
                info!("⚠️  If reCAPTCHA appears, please complete it manually...");
                info!("   Waiting up to 5 minutes for reCAPTCHA completion...");
                info!(
                    "   💡 Tip: Set TWOCAPTCHA_API_KEY environment variable for automatic solving!"
                );
            } else {
                info!("   Verifying reCAPTCHA solution...");
            }

            // Wait for login button to become enabled (reCAPTCHA completion)
            let login_button_selector = By::Css("button[type='submit'], input[name='NattLogin']");

            // Poll for enabled button with generous timeout
            let mut captcha_completed = false;
            let max_attempts = if captcha_solver_option.is_some() {
                12
            } else {
                60
            }; // 1 min for auto, 5 min for manual

            for _ in 0..max_attempts {
                if let Ok(button) = self.driver.find(login_button_selector.clone()).await {
                    if let Ok(is_enabled) = button.is_enabled().await {
                        if is_enabled {
                            info!("✅ Login button is now enabled (reCAPTCHA completed or not required)");
                            captcha_completed = true;
                            break;
                        }
                    }
                }
                sleep(Duration::from_secs(5)).await;
            }

            if !captcha_completed {
                warn!("Timeout waiting for reCAPTCHA completion");
                continue;
            }

            // Click login button
            let login_button = self.driver.find(login_button_selector).await?;
            login_button.click().await?;
            debug!("Clicked login button");

            // Wait for navigation after login
            sleep(Duration::from_secs(5)).await;

            // Check if login was successful by looking for CEP search form
            if self
                .driver
                .find(By::Css("input[name='e_cep']"))
                .await
                .is_ok()
            {
                info!("✅ Logged in successfully with credentials #{}", idx + 1);
                login_successful = true;
                break;
            } else {
                warn!(
                    "Login attempt #{} failed, trying next credentials...",
                    idx + 1
                );
                // Navigate back to login page for next attempt
                self.driver.goto(&self.base_url).await?;
                sleep(Duration::from_secs(2)).await;
            }
        }

        if !login_successful {
            bail!("All login attempts failed. Please check credentials and ensure reCAPTCHA was completed.");
        }

        // Save session for future use
        info!("Saving session cookies for future logins...");
        if let Err(e) = session_manager.save_session(&self.driver).await {
            warn!("Failed to save session: {}", e);
        }

        Ok(())
    }

    /// Navigate to CEP search page
    async fn ensure_on_cep_search_page(&self) -> Result<()> {
        // Check if we're already on the CEP search page
        if self
            .driver
            .find(By::Css("input[name='e_cep']"))
            .await
            .is_ok()
        {
            debug!("Already on CEP search page");
            return Ok(());
        }

        // Try to navigate to the search page via direct URL
        let search_url = format!("{}/sistema/consultas/", self.base_url);
        self.driver.goto(&search_url).await?;
        sleep(Duration::from_secs(3)).await;

        // Verify we're on the correct page
        if self
            .driver
            .find(By::Css("input[name='e_cep']"))
            .await
            .is_err()
        {
            bail!("Could not navigate to CEP search page");
        }

        info!("✅ Navigated to CEP search page");
        Ok(())
    }

    /// Search by CEP with range
    pub async fn search_by_cep(
        &self,
        cep: &str,
        numero_inicio: u64,
        numero_fim: u64,
    ) -> Result<Vec<AddressRecord>> {
        info!(
            "Searching DBase for CEP: {} (range: {} - {})",
            cep, numero_inicio, numero_fim
        );

        // Ensure we're on the search page
        self.ensure_on_cep_search_page().await?;

        // Fill CEP field
        let cep_field = self
            .driver
            .find(By::Css("input[name='e_cep']"))
            .await
            .context("Could not find CEP field")?;

        cep_field.clear().await?;
        cep_field.send_keys(cep).await?;
        debug!("Filled CEP: {}", cep);

        // Fill N.Inicio field
        if let Ok(inicio_field) = self.driver.find(By::Css("input[name='n_inicio']")).await {
            inicio_field.clear().await?;
            inicio_field.send_keys(&numero_inicio.to_string()).await?;
            debug!("Filled N.Inicio: {}", numero_inicio);
        }

        // Fill N.Fim field
        if let Ok(fim_field) = self.driver.find(By::Css("input[name='n_fim']")).await {
            fim_field.clear().await?;
            fim_field.send_keys(&numero_fim.to_string()).await?;
            debug!("Filled N.Fim: {}", numero_fim);
        }

        info!("🔍 Form filled, clicking 'Pesquisar' button...");

        // Find and click the search button
        let search_button = self
            .driver
            .find(By::Css(
                "input[name='pesquisar'], button[type='submit'], input[type='submit']",
            ))
            .await
            .context("Could not find search button")?;

        search_button.click().await?;
        info!("✅ Search button clicked, waiting for results...");

        // Wait for results table to appear
        sleep(Duration::from_secs(2)).await;

        // Poll for results table (up to 5 minutes)
        let mut table_found = false;
        for attempt in 1..=60 {
            if attempt % 6 == 0 {
                // Log every 30 seconds
                info!(
                    "   Still waiting for search results... ({}/300 seconds)",
                    attempt * 5
                );
            }

            if self.driver.find(By::Css("table")).await.is_ok() {
                info!("✅ Results table detected!");
                table_found = true;
                break;
            }

            sleep(Duration::from_secs(5)).await;
        }

        if !table_found {
            bail!("Timeout waiting for search results.");
        }

        // Extract data from all pages
        self.extract_all_pages().await
    }

    /// Extract data from all paginated pages
    async fn extract_all_pages(&self) -> Result<Vec<AddressRecord>> {
        let mut all_records = Vec::new();
        let mut page_num = 1;
        const MAX_PAGES: usize = 100;

        loop {
            info!("📊 Extracting data from page {}...", page_num);

            // Get current page HTML
            let html = self.driver.source().await?;
            let page_records = extract_table_data(&html)?;

            if page_records.is_empty() {
                info!("   No data found on page {}, stopping", page_num);
                break;
            }

            info!(
                "   Extracted {} records from page {}",
                page_records.len(),
                page_num
            );
            all_records.extend(page_records);

            // Check for next page button
            if !self.has_next_page().await? {
                info!("✅ No more pages (total pages: {})", page_num);
                break;
            }

            // Click next page
            if !self.click_next_page().await? {
                info!("✅ Reached last page (total pages: {})", page_num);
                break;
            }

            page_num += 1;

            if page_num > MAX_PAGES {
                warn!("⚠️  Reached maximum page limit ({}), stopping", MAX_PAGES);
                break;
            }

            // Wait for page to load
            sleep(Duration::from_millis(1500)).await;
        }

        info!(
            "✅ Total extracted: {} records from {} pages",
            all_records.len(),
            page_num
        );

        Ok(all_records)
    }

    /// Check if next page button exists
    async fn has_next_page(&self) -> Result<bool> {
        // Look for » (next page) button using JavaScript since CSS :has-text() isn't supported
        let script = r#"
            const pagination = document.querySelector('ul.pagination');
            if (!pagination) return false;

            const links = pagination.querySelectorAll('a');
            for (const link of links) {
                if (link.textContent.includes('»')) {
                    const parent = link.parentElement;
                    const isDisabled = parent.classList.contains('disabled');
                    const href = link.getAttribute('href');
                    return !isDisabled && href && href !== '#';
                }
            }
            return false;
        "#;

        if let Ok(result) = self.driver.execute(script, vec![]).await {
            if let Ok(has_next) = result.convert::<bool>() {
                debug!("   Pagination check: has_next = {}", has_next);
                return Ok(has_next);
            }
        }

        Ok(false)
    }

    /// Click next page button
    async fn click_next_page(&self) -> Result<bool> {
        // Use JavaScript to find and click the next button
        let script = r#"
            const pagination = document.querySelector('ul.pagination');
            if (!pagination) return false;

            const links = pagination.querySelectorAll('a');
            for (const link of links) {
                if (link.textContent.includes('»')) {
                    const parent = link.parentElement;
                    if (!parent.classList.contains('disabled')) {
                        link.click();
                        return true;
                    }
                }
            }
            return false;
        "#;

        if let Ok(result) = self.driver.execute(script, vec![]).await {
            if let Ok(clicked) = result.convert::<bool>() {
                if clicked {
                    debug!("   ➡️  Clicked next page button");
                    sleep(Duration::from_secs(2)).await;
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    /// Close the browser
    pub async fn close(self) -> Result<()> {
        self.driver.quit().await?;
        Ok(())
    }
}

/// Extract data from HTML table
fn extract_table_data(html_content: &str) -> Result<Vec<AddressRecord>> {
    let mut records = Vec::new();

    let document = Html::parse_document(html_content);

    // Find table
    let table_selector = Selector::parse("table").unwrap();
    let row_selector = Selector::parse("tr").unwrap();
    let cell_selector = Selector::parse("td").unwrap();

    let table = match document.select(&table_selector).next() {
        Some(t) => t,
        None => {
            debug!("No table found in HTML");
            return Ok(records);
        }
    };

    // Iterate through rows (skip header row)
    let rows: Vec<_> = table.select(&row_selector).collect();

    for (idx, row) in rows.iter().enumerate() {
        if idx == 0 {
            // Skip header row
            continue;
        }

        let cells: Vec<_> = row.select(&cell_selector).collect();

        if cells.len() >= 7 {
            // Extract CPF/CNPJ (might be in <a> tag with 🔍 icon)
            let cpf_cnpj_text = cells[0].text().collect::<String>();
            let cpf_cnpj = cpf_cnpj_text.replace("🔍", "").trim().to_string();

            let record = AddressRecord {
                cpf_cnpj,
                nome_razao_social: cells[1].text().collect::<String>().trim().to_string(),
                logradouro: cells[2].text().collect::<String>().trim().to_string(),
                numero: cells[3].text().collect::<String>().trim().to_string(),
                complemento: cells[4].text().collect::<String>().trim().to_string(),
                bairro: cells[5].text().collect::<String>().trim().to_string(),
                cep: cells[6].text().collect::<String>().trim().to_string(),
            };

            records.push(record);
        }
    }

    Ok(records)
}

/// Export records to CSV file
pub fn export_to_csv(records: &[AddressRecord], filename: &str) -> Result<()> {
    let file = File::create(filename)
        .with_context(|| format!("Failed to create CSV file: {}", filename))?;

    let mut wtr = csv::Writer::from_writer(file);

    // Write header
    wtr.write_record(&[
        "cpf_cnpj",
        "nome_razao_social",
        "logradouro",
        "numero",
        "complemento",
        "bairro",
        "cep",
    ])?;

    // Write records
    for record in records {
        wtr.write_record(&[
            &record.cpf_cnpj,
            &record.nome_razao_social,
            &record.logradouro,
            &record.numero,
            &record.complemento,
            &record.bairro,
            &record.cep,
        ])?;
    }

    wtr.flush()?;
    info!("💾 Exported {} records to {}", records.len(), filename);
    Ok(())
}

/// Generate timestamped filename for CSV export
pub fn generate_csv_filename() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();

    let timestamp = chrono::DateTime::from_timestamp(now.as_secs() as i64, 0)
        .unwrap()
        .format("%Y%m%d_%H%M%S");

    format!("output/dbase_scraped_{}.csv", timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_table_data_empty() {
        let html = "<html><body></body></html>";
        let records = extract_table_data(html).unwrap();
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_extract_table_data_with_data() {
        let html = r#"
        <html>
        <body>
        <table>
            <tr>
                <th>CPF/CNPJ</th>
                <th>Nome/Razão Social</th>
                <th>Logradouro</th>
                <th>Número</th>
                <th>Complemento</th>
                <th>Bairro</th>
                <th>CEP</th>
            </tr>
            <tr>
                <td><a>607.661.908-20 🔍</a></td>
                <td>MARIO CELSO LOPES</td>
                <td>AV HORACIO LAFER</td>
                <td>120</td>
                <td></td>
                <td>ITAIM BIBI</td>
                <td>4538080</td>
            </tr>
        </table>
        </body>
        </html>
        "#;

        let records = extract_table_data(html).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].cpf_cnpj, "607.661.908-20");
        assert_eq!(records[0].nome_razao_social, "MARIO CELSO LOPES");
        assert_eq!(records[0].logradouro, "AV HORACIO LAFER");
        assert_eq!(records[0].numero, "120");
        assert_eq!(records[0].bairro, "ITAIM BIBI");
        assert_eq!(records[0].cep, "4538080");
    }

    #[test]
    fn test_generate_csv_filename() {
        let filename = generate_csv_filename();
        assert!(filename.starts_with("output/dbase_scraped_"));
        assert!(filename.ends_with(".csv"));
    }
}
