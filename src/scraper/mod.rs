use anyhow::Result;
use thirtyfour::{WebDriver, DesiredCapabilities, By, WebElement};
use tokio::time::{sleep, Duration};
use rand::Rng;

// ScraperResult moved inline
#[derive(Debug, Clone)]
pub struct ScraperResult {
    pub contributor_number: String,
    pub numero_cadastro: Option<String>,
    pub nome_proprietario: Option<String>,
    pub nome_compromissario: Option<String>,
    pub endereco: Option<String>,
    pub numero: Option<String>,
    pub complemento: Option<String>,
    pub bairro: Option<String>,
    pub cep: Option<String>,
    pub success: bool,
    pub error: Option<String>,
}

pub struct ScraperConfig {
    pub max_concurrent: usize,
    pub headless: bool,
    pub timeout_secs: u64,
    pub retry_attempts: u32,
    pub rate_limit_per_hour: usize,
}

#[allow(dead_code)]
impl ScraperConfig {
    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    pub fn retry_attempts(&self) -> u32 {
        self.retry_attempts
    }
}

pub struct ScraperEngine {
    config: ScraperConfig,
    driver_pool: Vec<WebDriver>,
}

impl ScraperEngine {
    pub async fn new(config: ScraperConfig) -> Result<Self> {
        let mut driver_pool = Vec::new();

        // Create WebDriver pool
        for _ in 0..config.max_concurrent {
            let mut caps = DesiredCapabilities::chrome();
            if config.headless {
                caps.add_chrome_arg("--headless")?;
            }
            caps.add_chrome_arg("--no-sandbox")?;
            caps.add_chrome_arg("--disable-dev-shm-usage")?;
            caps.add_chrome_arg("--disable-gpu")?;
            caps.add_chrome_arg("--window-size=1920,1080")?;

            let driver = WebDriver::new("http://localhost:9515", caps).await?;
            driver_pool.push(driver);
        }

        Ok(Self {
            config,
            driver_pool,
        })
    }

    pub async fn process_batch_with_rate_limit<F>(
        &self,
        jobs: Vec<String>,
        mut progress_callback: F,
    ) -> Vec<ScraperResult>
    where
        F: FnMut(usize, usize) + Send + 'static,
    {
        let mut results = Vec::new();
        let total = jobs.len();
        let mut completed = 0;

        // Calculate delay between requests to respect rate limit
        let delay_ms = if self.config.rate_limit_per_hour > 0 {
            (3600 * 1000) / self.config.rate_limit_per_hour as u64
        } else {
            0
        };

        for chunk in jobs.chunks(self.config.max_concurrent) {

            for (i, contributor_number) in chunk.iter().enumerate() {
                let driver = &self.driver_pool[i];
                let number = contributor_number.clone();

                // Process job
                let result = self.scrape_iptu(driver, &number).await;

                results.push(ScraperResult {
                    contributor_number: number,
                    numero_cadastro: result.as_ref().ok().and_then(|r| r.numero_cadastro.clone()),
                    nome_proprietario: result.as_ref().ok().and_then(|r| r.nome_proprietario.clone()),
                    nome_compromissario: result.as_ref().ok().and_then(|r| r.nome_compromissario.clone()),
                    endereco: result.as_ref().ok().and_then(|r| r.endereco.clone()),
                    numero: result.as_ref().ok().and_then(|r| r.numero.clone()),
                    complemento: result.as_ref().ok().and_then(|r| r.complemento.clone()),
                    bairro: result.as_ref().ok().and_then(|r| r.bairro.clone()),
                    cep: result.as_ref().ok().and_then(|r| r.cep.clone()),
                    success: result.is_ok(),
                    error: result.err().map(|e| e.to_string()),
                });

                completed += 1;
                progress_callback(completed, total);

                // Add random delay between 2-5 seconds to avoid being detected as bot
                let mut rng = rand::thread_rng();
                let random_delay = rng.gen_range(2000..=5000); // 2 to 5 seconds in milliseconds
                tracing::info!("Waiting {}ms before next request", random_delay);
                sleep(Duration::from_millis(random_delay)).await;

                // Additional rate limiting if configured
                if delay_ms > 0 {
                    sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }

        results
    }

    async fn scrape_iptu(&self, driver: &WebDriver, contributor_number: &str) -> Result<IPTUData> {
        tracing::info!("Starting scrape for: {}", contributor_number);

        // Navigate to São Paulo IPTU website
        driver.goto("https://www3.prefeitura.sp.gov.br/sf8663/formsinternet/principal.aspx").await?;

        // Wait for page load
        sleep(Duration::from_secs(3)).await;

        // Handle cookie consent modal - it's an input button, not a button element
        tracing::info!("Looking for cookie consent modal...");

        // Try multiple selectors to find the cookie consent button
        let cookie_selectors = vec![
            "input.cc__button__autorizacao--all",
            "input[type='button'][class*='cc__button__autorizacao']",
            "input[value*='Autorizo o uso de todos os cookies']",
            ".cc__button__autorizacao--all",
        ];

        let mut cookie_handled = false;
        for selector in cookie_selectors {
            if let Ok(consent_button) = driver.find(By::Css(selector)).await {
                tracing::info!("Found cookie consent button with selector: {}", selector);

                // Try JavaScript click first (more reliable)
                let js_click = format!(
                    "document.querySelector('{}').click();",
                    selector
                );

                match driver.execute(&js_click, vec![]).await {
                    Ok(_) => {
                        tracing::info!("Cookie consent accepted via JavaScript");
                        cookie_handled = true;
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("JavaScript click failed: {}, trying direct click", e);
                        // Fallback to direct click
                        if consent_button.click().await.is_ok() {
                            tracing::info!("Cookie consent accepted via direct click");
                            cookie_handled = true;
                            break;
                        }
                    }
                }
            }
        }

        if cookie_handled {
            sleep(Duration::from_secs(2)).await;
            tracing::info!("Cookie modal dismissed, continuing...");
        } else {
            tracing::info!("No cookie consent modal found or couldn't click it");
        }

        // Parse contributor number (remove dots and dashes)
        let parts = contributor_number.replace(".", "").replace("-", "").trim().to_string();

        if parts.len() < 11 {
            anyhow::bail!("Número de cadastro inválido");
        }

        // Find all text input fields
        let inputs = driver.find_all(By::Css("input[type='text']")).await?;

        if inputs.len() < 4 {
            anyhow::bail!("Campos de entrada não encontrados");
        }

        // Fill in the contributor number in 4 parts
        inputs[0].send_keys(&parts[0..3]).await?;
        inputs[1].send_keys(&parts[3..6]).await?;
        inputs[2].send_keys(&parts[6..10]).await?;
        inputs[3].send_keys(&parts[10..11]).await?;

        // Wait a bit for any dynamic content to load
        sleep(Duration::from_secs(2)).await;

        // Submit form - use JavaScript click to bypass overlay issues
        tracing::info!("Submitting form...");

        // Execute JavaScript directly instead of trying to click the element
        let click_script = r#"
            var btn = document.getElementById('_BtnAvancarDasii');
            if (btn) {
                btn.click();
                return true;
            }
            return false;
        "#;

        match driver.execute(click_script, vec![]).await {
            Ok(result) => {
                tracing::info!("Form submitted via JavaScript click: {:?}", result);
            }
            Err(e) => {
                tracing::warn!("JavaScript click failed: {}, trying regular click", e);
                // Fallback to finding and clicking the element
                if let Ok(submit_button) = driver.find(By::Id("_BtnAvancarDasii")).await {
                    submit_button.click().await?;
                    tracing::info!("Form submitted via direct click");
                } else {
                    anyhow::bail!("Could not find submit button");
                }
            }
        }

        // Wait for results page to load
        tracing::info!("Waiting for results page to load...");
        sleep(Duration::from_secs(8)).await;

        // Check if we're on the data page
        let page_content = driver.source().await?;
        let current_url = driver.current_url().await?;
        tracing::info!("Current URL after form submit: {}", current_url);

        // Check for various indicators that we're on the results page
        let is_results_page = page_content.contains("DADOS DO IMÓVEL")
            || page_content.contains("Dados do Imóvel")
            || page_content.contains("Proprietário")
            || page_content.contains("txtProprietarioNome")
            || page_content.contains("txtNumeroCadastro");

        if !is_results_page {
            // Check if we're still on the form page (error occurred)
            if page_content.contains("_BtnAvancarDasii") {
                tracing::error!("Still on form page - submission may have failed");
                anyhow::bail!("Form submission failed - still on form page");
            }

            tracing::error!("Page does not contain expected content markers");
            tracing::error!("Page length: {} bytes", page_content.len());
            anyhow::bail!("Página de dados não carregada");
        }

        tracing::info!("Results page loaded successfully");

        // Save page source for debugging
        tracing::debug!("Page loaded, extracting data...");

        // Save HTML to file for inspection (temporary debug)
        if let Ok(home) = std::env::var("HOME") {
            let debug_file = format!("{}/Desktop/iptu_debug_{}.html", home, contributor_number.replace(".", ""));
            if let Err(e) = std::fs::write(&debug_file, &page_content) {
                tracing::warn!("Could not save debug HTML: {}", e);
            } else {
                tracing::info!("Debug HTML saved to: {}", debug_file);
            }
        }

        // Extract data
        let data = self.extract_data(driver).await?;

        Ok(data)
    }

    async fn extract_data(&self, driver: &WebDriver) -> Result<IPTUData> {
        let mut data = IPTUData::default();

        // Debug: Get page source to see what we're working with
        let page_source = driver.source().await?;
        tracing::debug!("Page contains txtProprietarioNome: {}", page_source.contains("txtProprietarioNome"));

        // Helper function to extract value from element
        async fn get_element_value(elem: &WebElement) -> Option<String> {
            // Try getting value attribute first
            if let Ok(Some(value)) = elem.prop("value").await {
                if !value.is_empty() {
                    return Some(value);
                }
            }
            // Try getting text content
            if let Ok(text) = elem.text().await {
                if !text.is_empty() {
                    return Some(text);
                }
            }
            // Try getting attribute value
            if let Ok(Some(value)) = elem.attr("value").await {
                if !value.is_empty() {
                    return Some(value);
                }
            }
            None
        }

        // Extract Número de Cadastro no IPTU
        if let Ok(elem) = driver.find(By::Name("txtNumeroCadastro")).await {
            if let Some(value) = get_element_value(&elem).await {
                tracing::debug!("txtNumeroCadastro value: {:?}", value);
                data.numero_cadastro = Some(value);
            }
        } else {
            tracing::warn!("Could not find txtNumeroCadastro element");
        }

        // Try to extract data by input name attributes
        if let Ok(elem) = driver.find(By::Name("txtProprietarioNome")).await {
            if let Some(value) = get_element_value(&elem).await {
                tracing::debug!("txtProprietarioNome value: {:?}", value);
                data.nome_proprietario = Some(value);
            }
        } else {
            tracing::warn!("Could not find txtProprietarioNome element");
        }

        if let Ok(elem) = driver.find(By::Name("txtCompromissarioNome")).await {
            if let Some(value) = get_element_value(&elem).await {
                data.nome_compromissario = Some(value);
            }
        }

        if let Ok(elem) = driver.find(By::Name("txtEndereco")).await {
            if let Some(value) = get_element_value(&elem).await {
                data.endereco = Some(value);
            }
        } else {
            tracing::warn!("Could not find txtEndereco element");
        }

        if let Ok(elem) = driver.find(By::Name("txtNumero")).await {
            if let Some(value) = get_element_value(&elem).await {
                data.numero = Some(value);
            }
        }

        if let Ok(elem) = driver.find(By::Name("txtComplemento")).await {
            if let Some(value) = get_element_value(&elem).await {
                data.complemento = Some(value);
            }
        }

        if let Ok(elem) = driver.find(By::Name("txtBairro")).await {
            if let Some(value) = get_element_value(&elem).await {
                data.bairro = Some(value);
            }
        }

        if let Ok(elem) = driver.find(By::Name("txtCepImovel")).await {
            if let Some(value) = get_element_value(&elem).await {
                data.cep = Some(value);
            }
        }

        Ok(data)
    }

    pub async fn shutdown(self) {
        for driver in self.driver_pool {
            let _ = driver.quit().await;
        }
    }
}

#[derive(Debug, Default, Clone)]
struct IPTUData {
    numero_cadastro: Option<String>,
    nome_proprietario: Option<String>,
    nome_compromissario: Option<String>,
    endereco: Option<String>,
    numero: Option<String>,
    complemento: Option<String>,
    bairro: Option<String>,
    cep: Option<String>,
}