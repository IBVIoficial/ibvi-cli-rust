use anyhow::{bail, Context, Result};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use thirtyfour::prelude::*;
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};

async fn click_if_present(driver: &WebDriver, by: By) -> bool {
    match driver.find(by).await {
        Ok(elem) => {
            if elem.is_displayed().await.unwrap_or(false) {
                let _ = elem.click().await;
                sleep(Duration::from_millis(300)).await;
                true
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Represents a property record from the Diretrix Consultoria website
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyRecord {
    /// Owner name (Proprietário)
    pub owner: String,
    /// IPTU number
    pub iptu: String,
    /// Street name (Logradouro)
    pub street: String,
    /// Street number
    pub number: String,
    /// Complement (Complemento)
    pub complement: String,
    /// Second complement (Complemento 2)
    pub complement2: String,
    /// Neighborhood (Bairro)
    pub neighborhood: String,
    /// Document numbers from button data attributes
    pub document1: Option<String>,
    pub document2: Option<String>,
}

/// Diretrix scraper client
pub struct DiretrixScraper {
    driver: WebDriver,
    base_url: String,
    username: String,
    password: String,
}

impl DiretrixScraper {
    /// Create a new Diretrix scraper with credentials and WebDriver URL
    pub async fn new(
        username: String,
        password: String,
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

        let driver = WebDriver::new(webdriver_url, caps)
            .await
            .context("Failed to connect to WebDriver")?;

        Ok(Self {
            driver,
            base_url: "https://www.diretrixconsultoria.com.br".to_string(),
            username,
            password,
        })
    }

    /// Login to the Diretrix website
    pub async fn login(&self) -> Result<()> {
        info!("Logging in to Diretrix Consultoria...");

        // Navigate to the base URL
        self.driver.goto(&self.base_url).await?;

        // Wait for page to load
        sleep(Duration::from_secs(3)).await;

        // Find username field (Usuário)
        let username_field = match self
            .driver
            .find(By::XPath(
                "//input[@placeholder='Usuário' or @name='usuario' or contains(@class, 'usuario')]",
            ))
            .await
        {
            Ok(elem) => elem,
            Err(_) => self
                .driver
                .find(By::Css("input[type='text']"))
                .await
                .context("Could not find username field (Usuário)")?,
        };

        username_field.clear().await?;
        username_field.send_keys(&self.username).await?;
        debug!("Filled username field");

        // Find password field (Senha)
        let password_field = match self
            .driver
            .find(By::XPath(
                "//input[@placeholder='Senha' or @name='senha' or @type='password']",
            ))
            .await
        {
            Ok(elem) => elem,
            Err(_) => self
                .driver
                .find(By::Css("input[type='password']"))
                .await
                .context("Could not find password field (Senha)")?,
        };

        password_field.clear().await?;
        password_field.send_keys(&self.password).await?;
        debug!("Filled password field");

        // Find and click Login button
        let login_button = match self
            .driver
            .find(By::XPath(
                "//button[contains(text(), 'Login') or @type='submit']",
            ))
            .await
        {
            Ok(elem) => elem,
            Err(_) => self
                .driver
                .find(By::Css("button[type='submit']"))
                .await
                .context("Could not find Login button")?,
        };

        login_button.click().await?;
        debug!("Clicked login button");

        // Wait for login to complete and dashboard to load
        sleep(Duration::from_secs(5)).await;

        info!("Login completed successfully");

        // Don't navigate directly to avoid 404 errors
        // The ensure_on_search_page method will handle navigation via menu/breadcrumb
        info!("Ready to navigate to search page via menu");
        
        Ok(())
    }

    /// Close the browser
    pub async fn close(self) -> Result<()> {
        self.driver.quit().await?;
        Ok(())
    }

    async fn wait_for_page_ready(driver: &WebDriver) -> Result<()> {
        for _ in 0..10 {
            let state = driver
                .execute("return document.readyState", vec![])
                .await
                .context("Failed to check document.readyState")?;

            let state_str = format!("{:?}", state).to_lowercase();
            if state_str.contains("complete") || state_str.contains("interactive") {
                return Ok(());
            }

            sleep(Duration::from_millis(500)).await;
        }

        Ok(())
    }

    async fn ensure_on_search_page(&self) -> Result<()> {
        self.driver.enter_default_frame().await?;

        for attempt in 1..=4 {
            // First check if we're already on the correct page
            if let Ok(url) = self.driver.current_url().await {
                if url.as_str().contains("/IPTU/PorEndereco") {
                    // Try to find the search field in iframe or main page
                    if let Ok(frame) = self.driver.find(By::Id("iframeConteudo")).await {
                        frame.enter_frame().await?;
                        if self.driver.find(By::Id("txtProcurar")).await.is_ok() {
                            debug!("Already on search page with form in iframe");
                            return Ok(());
                        }
                        self.driver.enter_default_frame().await?;
                    } else if self.driver.find(By::Id("txtProcurar")).await.is_ok() {
                        debug!("Already on search page with form in main frame");
                        return Ok(());
                    }
                }
            }

            if attempt == 1 {
                info!("Navigating to IP-Trix 'Por Endereço' page via breadcrumb/menu...");
            } else {
                warn!(
                    "Retrying navigation to 'Por Endereço' page (attempt {}/4)",
                    attempt
                );
            }

            // Navigate to base URL first to ensure we're on the dashboard
            self.driver.goto(&self.base_url).await?;
            Self::wait_for_page_ready(&self.driver).await?;
            
            // Extended initial wait for dashboard to fully load
            sleep(Duration::from_secs(5)).await;

            let mut navigated = false;

            // Strategy 1: Try breadcrumb navigation (preferred method)
            let breadcrumb_iptrix = By::XPath(
                "//ol[contains(@class,'breadcrumb')]//a[contains(normalize-space(.),'IP-Trix')]",
            );
            let breadcrumb_endereco = By::XPath(
                "//ol[contains(@class,'breadcrumb')]//a[contains(normalize-space(.),'Por Endereço')]",
            );

            // Click IP-Trix in breadcrumb if present
            if click_if_present(&self.driver, breadcrumb_iptrix.clone()).await {
                debug!("Clicked IP-Trix in breadcrumb");
                sleep(Duration::from_millis(800)).await;
                
                // Then click Por Endereço
                if click_if_present(&self.driver, breadcrumb_endereco.clone()).await {
                    debug!("Clicked Por Endereço in breadcrumb");
                    navigated = true;
                }
            }

            // Strategy 2: Try menu navigation via link text
            if !navigated {
                if click_if_present(&self.driver, By::LinkText("IP-Trix")).await {
                    debug!("Clicked IP-Trix menu link");
                    sleep(Duration::from_millis(800)).await;
                    
                    if click_if_present(&self.driver, By::LinkText("Por Endereço")).await {
                        debug!("Clicked Por Endereço submenu");
                        navigated = true;
                    }
                }
            }

            // Strategy 3: Try direct href click (less preferred)
            if !navigated {
                if click_if_present(&self.driver, By::Css("a[href='/IPTU/PorEndereco']")).await {
                    debug!("Clicked direct Por Endereço link");
                    navigated = true;
                }
            }

            // Strategy 4: Try span-based navigation
            if !navigated {
                if click_if_present(
                    &self.driver,
                    By::XPath("//span[contains(.,'IP-TRIX') or contains(.,'IPTRIX')]"),
                )
                .await
                {
                    debug!("Clicked IP-TRIX span element");
                    sleep(Duration::from_millis(800)).await;
                    
                    if click_if_present(&self.driver, By::LinkText("Por Endereço")).await {
                        debug!("Clicked Por Endereço after span click");
                        navigated = true;
                    }
                }
            }

            // Strategy 5: Try consultas menu path
            if !navigated {
                if let Ok(menu) = self
                    .driver
                    .find(By::Css("a[href='/consultas/iptrix']"))
                    .await
                {
                    debug!("Found consultas/iptrix menu link");
                    let _ = menu.click().await;
                    sleep(Duration::from_secs(1)).await;
                    
                    if let Ok(link) = self.driver.find(By::LinkText("Por Endereço")).await {
                        link.click().await.ok();
                        debug!("Clicked Por Endereço from consultas menu");
                        navigated = true;
                    }
                }
            }

            if !navigated {
                debug!("Could not navigate via menu/breadcrumb, will check page state");
            }

            // Wait for navigation to complete
            Self::wait_for_page_ready(&self.driver).await?;
            sleep(Duration::from_secs(3)).await;

            // Check for 404 error in page source
            let page_source = self.driver.source().await.unwrap_or_default();
            let lower_source = page_source.to_lowercase();
            
            if lower_source.contains("http error 404")
                || lower_source.contains("404.0 - not found")
                || lower_source.contains("404 not found")
                || lower_source.contains("página não encontrada")
            {
                warn!("Detected 404 error page, backing out and retrying");
                
                // Navigate back to recover from 404
                let _ = self.driver.back().await;
                sleep(Duration::from_secs(3)).await;
                self.driver.enter_default_frame().await?;
                
                // Continue to next attempt
                continue;
            }

            // Check if we successfully reached the search page
            if let Ok(frame) = self.driver.find(By::Id("iframeConteudo")).await {
                frame.enter_frame().await?;
                
                // Look for the search input field
                if self.driver.find(By::Id("txtProcurar")).await.is_ok() {
                    info!("Successfully reached 'Por Endereço' search page (iframe)");
                    return Ok(());
                }
                
                self.driver.enter_default_frame().await?;
            } else if self.driver.find(By::Id("txtProcurar")).await.is_ok() {
                info!("Successfully reached 'Por Endereço' search page (main frame)");
                return Ok(());
            }

            // Additional error checking
            if lower_source.contains("erro") || lower_source.contains("error") {
                warn!("Page contains error indicators");
            }

            // If not on last attempt, go back and retry
            if attempt < 4 {
                debug!("Search form not found, backing out for retry");
                let _ = self.driver.back().await;
                sleep(Duration::from_secs(2)).await;
                self.driver.enter_default_frame().await?;
            }
        }

        bail!("Unable to reach IP-Trix 'Por Endereço' page after 4 attempts")
    }

    /// Search for properties by street name and number
    /// Assumes we're already on the search page after login
    pub async fn search_by_address(
        &self,
        street_name: &str,
        street_number: &str,
    ) -> Result<Vec<PropertyRecord>> {
        self.ensure_on_search_page().await?;

        let mut switched_to_frame = false;
        if let Ok(frame) = self.driver.find(By::Id("iframeConteudo")).await {
            frame.enter_frame().await?;
            switched_to_frame = true;
        }

        async fn ensure_input_value(
            driver: &WebDriver,
            element: &WebElement,
            element_id: &str,
            value: &str,
        ) -> Result<()> {
            // Ensure element is ready
            element.wait_until().displayed().await?;
            element.wait_until().enabled().await?;
            element.scroll_into_view().await?;

            // Human-like interaction: click, pause, focus
            element.click().await?;
            sleep(Duration::from_millis(300)).await;
            let _ = element.focus().await;

            // JavaScript focus for extra reliability
            let focus_script = format!(
                "var el = document.getElementById('{}'); if (el) {{ el.focus(); el.select(); }}",
                element_id
            );
            let _ = driver.execute(&focus_script, vec![]).await?;

            // Clear and type with human-like delays
            sleep(Duration::from_millis(200)).await;
            element.clear().await?;
            sleep(Duration::from_millis(200)).await;
            element.send_keys(value).await?;
            sleep(Duration::from_millis(300)).await;

            // Verify the value was set
            if let Ok(Some(current)) = element.prop("value").await {
                if current.trim() == value {
                    return Ok(());
                }
            }

            // Fallback: Set via JavaScript if normal typing didn't work
            let js_value = serde_json::to_string(value)?;
            let script = format!(
                "var el = document.getElementById('{}'); \
                 if (el) {{ \
                    el.value = {}; \
                    el.dispatchEvent(new Event('input', {{ bubbles: true }})); \
                    el.dispatchEvent(new Event('change', {{ bubbles: true }})); \
                    return true; \
                 }} \
                 return false;",
                element_id, js_value
            );

            let result = driver.execute(&script, vec![]).await?;
            if format!("{:?}", result).contains("true") {
                sleep(Duration::from_millis(200)).await;
                return Ok(());
            }

            bail!("Failed to set input value for {}", element_id);
        }

        info!(
            "Searching for properties at: {} {}",
            street_name, street_number
        );

        // Wait for the page to fully render
        sleep(Duration::from_secs(2)).await;

        // Step 1: Scroll to #porEndereco wrapper if it exists
        if let Ok(wrapper) = self.driver.find(By::Id("porEndereco")).await {
            debug!("Scrolling to #porEndereco wrapper");
            let _ = wrapper.scroll_into_view().await;
            sleep(Duration::from_millis(500)).await;
        }

        // Step 2: Click the Por Endereço link if present (to activate the form)
        if let Ok(por_endereco_link) = self.driver.find(By::LinkText("Por Endereço")).await {
            if por_endereco_link.is_displayed().await.unwrap_or(false) {
                debug!("Clicking 'Por Endereço' link to activate form");
                let _ = por_endereco_link.click().await;
                sleep(Duration::from_millis(800)).await;
            }
        }

        // Step 3: Try to find and focus the street name field with retries
        let mut street_name_field = None;
        for attempt in 1..=5 {
            match self.driver.find(By::Id("txtProcurar")).await {
                Ok(field) => {
                    street_name_field = Some(field);
                    break;
                }
                Err(_) => {
                    if attempt < 5 {
                        debug!("Attempt {}: Street field not found yet, waiting...", attempt);
                        sleep(Duration::from_secs(1)).await;
                        
                        // Try clicking the wrapper again
                        if let Ok(wrapper) = self.driver.find(By::Id("porEndereco")).await {
                            let _ = wrapper.click().await;
                            sleep(Duration::from_millis(500)).await;
                        }
                    }
                }
            }
        }

        let street_name_field = street_name_field
            .context("Could not find street name field #txtProcurar after 5 attempts")?;

        // Step 4: Click inside the street input before typing (human-like behavior)
        debug!("Clicking and focusing street name input field");
        street_name_field.click().await?;
        sleep(Duration::from_millis(500)).await;
        
        // Now fill the street name with human-like interaction
        ensure_input_value(&self.driver, &street_name_field, "txtProcurar", street_name).await?;
        info!("Filled street name: {}", street_name);

        // Step 5: Find and fill street number field
        let street_number_field = self
            .driver
            .find(By::Id("txtNumero"))
            .await
            .context("Could not find street number field #txtNumero")?;

        ensure_input_value(
            &self.driver,
            &street_number_field,
            "txtNumero",
            street_number,
        )
        .await?;
        info!("Filled street number: {}", street_number);

        // Step 6: Find and click search button with a brief pause
        sleep(Duration::from_millis(500)).await;
        
        let search_button = self
            .driver
            .find(By::Id("btnPesquisar"))
            .await
            .context("Could not find search button #btnPesquisar")?;

        info!("Clicking search button...");
        search_button.click().await?;

        // Wait for results to load (AJAX request)
        sleep(Duration::from_secs(5)).await;

        // Get the page HTML
        let html_content = self.driver.source().await?;
        debug!("Received HTML response of {} bytes", html_content.len());

        if switched_to_frame {
            let _ = self.driver.enter_default_frame().await;
        }

        // Parse the HTML and extract property records
        self.parse_property_table(&html_content)
    }

    /// Manual search mode - wait for user to complete the search manually
    /// Then parse the results
    #[allow(dead_code)]
    pub async fn search_by_address_manual(
        &self,
        street_name: &str,
        street_number: &str,
    ) -> Result<Vec<PropertyRecord>> {
        info!("=== MANUAL SEARCH MODE ===");
        info!("Please complete these steps in the browser window:");
        info!("1. Fill in street name: {}", street_name);
        info!("2. Fill in street number: {}", street_number);
        info!("3. Click the 'Buscar' button");
        info!("4. Wait for results to load");
        info!("");
        info!("Waiting 45 seconds for you to complete the search...");

        // Wait for user to manually perform the search
        sleep(Duration::from_secs(45)).await;

        info!("Retrieving results...");

        // Get the page HTML after user has performed the search
        let html_content = self.driver.source().await?;
        debug!("Received HTML response of {} bytes", html_content.len());

        // Parse the HTML and extract property records
        self.parse_property_table(&html_content)
    }

    /// Parse the HTML table containing property records
    fn parse_property_table(&self, html: &str) -> Result<Vec<PropertyRecord>> {
        let document = Html::parse_document(html);

        // Check if there are no results
        let no_results_selector = Selector::parse("#msgtab").unwrap();
        if let Some(msg_element) = document.select(&no_results_selector).next() {
            let display_style = msg_element.value().attr("style").unwrap_or("");
            if !display_style.contains("display:none") {
                warn!("No records found");
                return Ok(Vec::new());
            }
        }

        // Select all table rows in the tbody
        let row_selector = Selector::parse("#Relatorio tr").unwrap();
        let td_selector = Selector::parse("td").unwrap();
        let button_selector = Selector::parse("button.enderecoDet").unwrap();

        let mut records = Vec::new();

        for row in document.select(&row_selector) {
            let cells: Vec<_> = row.select(&td_selector).collect();

            if cells.len() < 8 {
                warn!("Skipping row with insufficient cells");
                continue;
            }

            // Extract text from cells
            let owner = cells[0].text().collect::<String>().trim().to_string();
            let iptu = cells[1].text().collect::<String>().trim().to_string();
            let street = cells[2].text().collect::<String>().trim().to_string();
            let number = cells[3].text().collect::<String>().trim().to_string();
            let complement = cells[4].text().collect::<String>().trim().to_string();
            let complement2 = cells[5].text().collect::<String>().trim().to_string();
            let neighborhood = cells[6].text().collect::<String>().trim().to_string();

            // Extract document numbers from button attributes
            let button = cells[7].select(&button_selector).next();
            let document1 = button
                .and_then(|b| b.value().attr("data-documento"))
                .map(|s| s.to_string());
            let document2 = button
                .and_then(|b| b.value().attr("data-documento-2"))
                .map(|s| s.to_string());

            let record = PropertyRecord {
                owner,
                iptu,
                street,
                number,
                complement,
                complement2,
                neighborhood,
                document1,
                document2,
            };

            debug!("Parsed record: {:?}", record);
            records.push(record);
        }

        info!("Parsed {} property records", records.len());
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires valid credentials and WebDriver
    async fn test_login() {
        let scraper = DiretrixScraper::new(
            "100198".to_string(),
            "Mb082025".to_string(),
            "http://localhost:9515",
            false,
        )
        .await
        .expect("Failed to create scraper");

        let result = scraper.login().await;
        assert!(result.is_ok());

        scraper.close().await.ok();
    }

    #[tokio::test]
    #[ignore] // Requires valid credentials, WebDriver and login
    async fn test_search_by_address() {
        let scraper = DiretrixScraper::new(
            "100198".to_string(),
            "Mb082025".to_string(),
            "http://localhost:9515",
            false,
        )
        .await
        .expect("Failed to create scraper");

        scraper.login().await.expect("Login failed");

        let results = scraper
            .search_by_address("Domingos Leme", "440")
            .await
            .expect("Search failed");

        assert!(!results.is_empty());

        for record in results {
            println!("{:?}", record);
        }

        scraper.close().await.ok();
    }
}
