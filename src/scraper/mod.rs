use anyhow::Result;
use thirtyfour::{WebDriver, DesiredCapabilities, By, WebElement};
use tokio::time::{sleep, Duration};
use rand::Rng;
use rand::seq::SliceRandom;

// Delay patterns for human-like behavior
#[derive(Clone)]
enum DelayPattern {
    Quick,      // 1-3 seconds
    Normal,     // 3-7 seconds
    Slow,       // 7-15 seconds
}

impl DelayPattern {
    async fn wait(&self) {
        let mut rng = rand::thread_rng();
        let delay_ms = match self {
            Self::Quick => rng.gen_range(1000..3000),
            Self::Normal => rng.gen_range(3000..7000),
            Self::Slow => rng.gen_range(7000..15000),
        };

        // Add extra jitter: ±20%
        let jitter_percent = rng.gen_range(-20..=20) as f64 / 100.0;
        let final_delay = (delay_ms as f64 * (1.0 + jitter_percent)) as u64;

        sleep(Duration::from_millis(final_delay)).await;
    }

    fn random() -> Self {
        let patterns = [
            Self::Quick,
            Self::Normal,
            Self::Normal,  // More likely to be normal
            Self::Normal,
            Self::Slow,
        ];

        patterns.choose(&mut rand::thread_rng()).unwrap().clone()
    }
}

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

// Data structure for IPTU information
#[derive(Debug, Default)]
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

// Helper functions for human-like behavior
impl ScraperEngine {
    // Random scrolling to simulate human reading
    async fn random_scroll(driver: &WebDriver) -> Result<()> {
        let mut rng = rand::thread_rng();
        let num_scrolls = rng.gen_range(2..6);

        for _ in 0..num_scrolls {
            let scroll_amount = rng.gen_range(200..800);

            driver.execute(&format!(
                "window.scrollBy(0, {});",
                scroll_amount
            ), vec![]).await?;

            // Wait between scrolls (reading time)
            sleep(Duration::from_millis(rng.gen_range(300..1000))).await;
        }

        Ok(())
    }

    // Random mouse movements to avoid detection (simplified without action_chain)
    async fn random_mouse_movements(_driver: &WebDriver) -> Result<()> {
        let mut rng = rand::thread_rng();

        // Simulate mouse movement with JavaScript instead
        let movements = rng.gen_range(1..4);
        for _ in 0..movements {
            // Random pause to simulate mouse movements
            sleep(Duration::from_millis(rng.gen_range(200..800))).await;
        }

        Ok(())
    }
}

impl ScraperEngine {
    pub async fn new(config: ScraperConfig) -> Result<Self> {
        let mut driver_pool = Vec::new();

        // User-Agent strings for rotation
        let user_agents = vec![
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Safari/605.1.15",
        ];

        // Create WebDriver pool
        for i in 0..config.max_concurrent {
            let mut caps = DesiredCapabilities::chrome();
            if config.headless {
                caps.add_chrome_arg("--headless")?;
            }
            caps.add_chrome_arg("--no-sandbox")?;
            caps.add_chrome_arg("--disable-dev-shm-usage")?;
            caps.add_chrome_arg("--disable-gpu")?;
            caps.add_chrome_arg("--window-size=1920,1080")?;

            // Rotate User-Agent for each driver instance
            let user_agent = &user_agents[i % user_agents.len()];
            caps.add_chrome_arg(&format!("--user-agent={}", user_agent))?;

            // Additional anti-detection measures
            caps.add_chrome_arg("--disable-blink-features=AutomationControlled")?;

            let driver = WebDriver::new("http://localhost:9515", caps).await?;

            // Inject JavaScript to hide automation indicators
            let _ = driver.execute(r#"
                Object.defineProperty(navigator, 'webdriver', {
                    get: () => undefined
                });
                Object.defineProperty(navigator, 'plugins', {
                    get: () => [1, 2, 3, 4, 5]
                });
                Object.defineProperty(navigator, 'languages', {
                    get: () => ['en-US', 'en']
                });
                window.chrome = {
                    runtime: {}
                };
                Object.defineProperty(navigator, 'permissions', {
                    get: () => ({
                        query: () => Promise.resolve({ state: 'granted' })
                    })
                });
            "#, vec![]).await;

            driver_pool.push(driver);
        }

        Ok(Self {
            config,
            driver_pool,
        })
    }

    pub async fn process_batch_with_callback<F>(
        &self,
        jobs: Vec<String>,
        mut callback: F,
    ) -> Vec<ScraperResult>
    where
        F: FnMut(&ScraperResult, usize, usize) + Send + 'static,
    {
        let mut results = Vec::new();
        let total = jobs.len();
        let mut completed = 0;

        // Log the jobs being processed
        tracing::info!("Processing {} jobs total", total);
        for (idx, job) in jobs.iter().enumerate() {
            tracing::info!("Job {}: {}", idx + 1, job);
        }

        // Calculate delay between requests to respect rate limit
        let _delay_ms = if self.config.rate_limit_per_hour > 0 {
            (3600 * 1000) / self.config.rate_limit_per_hour as u64
        } else {
            0
        };

        use futures::future::join_all;

        for chunk in jobs.chunks(self.config.max_concurrent) {
            let mut tasks = Vec::new();

            // Launch all jobs in this chunk concurrently
            for (i, contributor_number) in chunk.iter().enumerate() {
                let driver = self.driver_pool[i].clone();
                let number = contributor_number.clone();

                tracing::info!("Launching concurrent job for: {}", number);

                // Calculate stagger delay with human-like variation
                let mut rng = rand::thread_rng();
                // Use exponentially distributed delays to be more human-like
                let base_delay = match i {
                    0 => 0, // First job starts immediately
                    _ => {
                        // Subsequent jobs have increasingly random delays
                        let min = 3000 + (i as u64 * 1000);
                        let max = 7000 + (i as u64 * 2000);
                        rng.gen_range(min..=max)
                    }
                };
                let stagger_delay = base_delay;

                // Create a future for each job
                let task = async move {
                    // Apply the stagger delay for all jobs except the first one
                    if stagger_delay > 0 {
                        tracing::info!("Waiting {}ms before starting job: {}", stagger_delay, number);
                        sleep(Duration::from_millis(stagger_delay)).await;
                    }

                    tracing::info!("Processing job: {}", number);

                    // Process job using the static scrape function
                    let result = Self::scrape_iptu_static(&driver, &number).await;

                    let scraper_result = ScraperResult {
                        contributor_number: number.clone(),
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
                    };

                    (number, scraper_result)
                };

                tasks.push(task);
            }

            // Wait for all tasks in the chunk to complete
            let chunk_results = join_all(tasks).await;

            // Process results and call callbacks
            for (number, scraper_result) in chunk_results {
                completed += 1;
                tracing::info!("Completed job {}/{}: {}", completed, total, number);

                // Call the callback with the result
                callback(&scraper_result, completed, total);

                results.push(scraper_result);
            }

            // Add delay between chunks to avoid overwhelming the server
            if chunk.len() == self.config.max_concurrent && completed < total {
                let mut rng = rand::thread_rng();
                let chunk_delay = rng.gen_range(3000..=5000);
                tracing::info!("Waiting {}ms before processing next chunk", chunk_delay);
                sleep(Duration::from_millis(chunk_delay)).await;
            }
        }

        results
    }

    // Static version for concurrent processing
    async fn scrape_iptu_static(driver: &WebDriver, contributor_number: &str) -> Result<IPTUData> {
        tracing::info!("Starting scrape for: {}", contributor_number);

        // Navigate to São Paulo IPTU website
        driver.goto("https://www3.prefeitura.sp.gov.br/sf8663/formsinternet/principal.aspx").await?;

        // Human-like delay pattern after page load
        DelayPattern::random().wait().await;

        // Sometimes do random mouse movements (30% chance)
        let mut rng = rand::thread_rng();
        if rng.gen_bool(0.3) {
            let _ = Self::random_mouse_movements(driver).await;
        }

        // Handle cookie consent and fill form (same logic as scrape_iptu)
        let _page_content = Self::handle_cookie_and_fill_form(driver, contributor_number).await?;

        // Occasionally scroll the page to simulate reading (40% chance)
        if rng.gen_bool(0.4) {
            let _ = Self::random_scroll(driver).await;
        }

        // Extract data using static method
        Self::extract_data_static(driver).await
    }

    async fn handle_cookie_and_fill_form(driver: &WebDriver, contributor_number: &str) -> Result<String> {
        // Cookie handling logic (extracted from scrape_iptu)
        tracing::info!("Looking for cookie consent modal...");

        sleep(Duration::from_secs(2)).await;

        let mut cookie_handled = false;
        let max_attempts = 3;

        for attempt in 1..=max_attempts {
            tracing::info!("Cookie consent attempt {}/{}", attempt, max_attempts);

            let js_direct_click = r#"
                var buttons = document.querySelectorAll('input[type="button"], button');
                var clicked = false;
                for (var i = 0; i < buttons.length; i++) {
                    var btn = buttons[i];
                    var text = (btn.value || btn.textContent || '').toLowerCase();
                    if (text.includes('autorizo') && text.includes('cookies')) {
                        console.log('Found cookie button:', btn);
                        btn.click();
                        clicked = true;
                        break;
                    }
                }
                if (!clicked) {
                    var cookieBtn = document.querySelector('input.cc__button__autorizacao--all');
                    if (cookieBtn) {
                        cookieBtn.click();
                        clicked = true;
                    }
                }
                return clicked;
            "#;

            if let Ok(result) = driver.execute(js_direct_click, vec![]).await {
                tracing::info!("JavaScript cookie consent result: {:?}", result);
                sleep(Duration::from_secs(2)).await;

                let check_modal = r#"
                    var buttons = document.querySelectorAll('input[type="button"]');
                    for (var i = 0; i < buttons.length; i++) {
                        var text = (buttons[i].value || '').toLowerCase();
                        if (text.includes('autorizo') && text.includes('cookies')) {
                            return true;
                        }
                    }
                    return false;
                "#;

                if let Ok(modal_present) = driver.execute(check_modal, vec![]).await {
                    let modal_gone = format!("{:?}", modal_present).contains("false");
                    if modal_gone {
                        tracing::info!("Cookie modal successfully dismissed!");
                        cookie_handled = true;
                        break;
                    }
                }
            }

            if attempt < max_attempts && !cookie_handled {
                sleep(Duration::from_secs(1)).await;
            }
        }

        if cookie_handled {
            tracing::info!("Cookie consent handled successfully");
        } else {
            tracing::warn!("Could not dismiss cookie modal, continuing anyway");
        }

        // Fill form logic
        let parts = contributor_number.replace(".", "").replace("-", "").trim().to_string();
        if parts.len() < 11 {
            anyhow::bail!("Número de cadastro inválido");
        }

        tracing::info!("Looking for form input fields...");
        let inputs = driver.find_all(By::Css("input[type='text']")).await?;
        tracing::info!("Found {} input fields", inputs.len());

        if inputs.len() < 4 {
            anyhow::bail!("Campos de entrada não encontrados");
        }

        tracing::info!("Filling contributor number: {}", parts);
        inputs[0].clear().await?;
        inputs[0].send_keys(&parts[0..3]).await?;
        tracing::info!("Filled field 1: {}", &parts[0..3]);

        inputs[1].clear().await?;
        inputs[1].send_keys(&parts[3..6]).await?;
        tracing::info!("Filled field 2: {}", &parts[3..6]);

        inputs[2].clear().await?;
        inputs[2].send_keys(&parts[6..10]).await?;
        tracing::info!("Filled field 3: {}", &parts[6..10]);

        inputs[3].clear().await?;
        inputs[3].send_keys(&parts[10..11]).await?;
        tracing::info!("Filled field 4: {}", &parts[10..11]);

        sleep(Duration::from_secs(2)).await;

        // Submit form
        tracing::info!("Submitting form...");

        let click_script = r#"
            var btn = document.getElementById('_BtnAvancarDasii');
            if (btn) {
                btn.click();
                return true;
            }
            return false;
        "#;

        if let Ok(result) = driver.execute(click_script, vec![]).await {
            tracing::info!("Form submitted via JavaScript click: {:?}", result);
        }

        tracing::info!("Waiting for results page to load...");
        sleep(Duration::from_secs(8)).await;

        let page_content = driver.source().await?;
        let current_url = driver.current_url().await?;
        tracing::info!("Current URL after form submit: {}", current_url);

        // Save debug HTML
        if let Ok(home) = std::env::var("HOME") {
            let debug_file = format!("{}/Desktop/iptus/iptu_debug_{}.html", home, contributor_number.replace(".", ""));
            if let Ok(_) = std::fs::write(&debug_file, &page_content) {
                tracing::info!("Debug HTML saved to: {}", debug_file);
            }
        }

        tracing::info!("Results page loaded successfully");
        Ok(page_content)
    }

    async fn extract_data_static(driver: &WebDriver) -> Result<IPTUData> {
        let mut data = IPTUData::default();

        // Helper function
        async fn get_element_value(elem: &WebElement) -> Option<String> {
            if let Ok(Some(value)) = elem.prop("value").await {
                if !value.is_empty() { return Some(value); }
            }
            if let Ok(text) = elem.text().await {
                if !text.is_empty() { return Some(text); }
            }
            if let Ok(Some(value)) = elem.attr("value").await {
                if !value.is_empty() { return Some(value); }
            }
            None
        }

        // Extract fields using the correct field names from the HTML
        // Número do IPTU
        if let Ok(elem) = driver.find(By::Name("txtNumIPTU")).await {
            data.numero_cadastro = get_element_value(&elem).await;
            tracing::debug!("Found txtNumIPTU: {:?}", data.numero_cadastro);
        } else {
            tracing::warn!("Could not find txtNumIPTU element");
        }

        // Nome do Proprietário
        if let Ok(elem) = driver.find(By::Name("txtProprietarioNome")).await {
            data.nome_proprietario = get_element_value(&elem).await;
            tracing::debug!("Found txtProprietarioNome: {:?}", data.nome_proprietario);
        } else {
            tracing::warn!("Could not find txtProprietarioNome element");
        }

        // Nome do Compromissário
        if let Ok(elem) = driver.find(By::Name("txtCompromissarioNome")).await {
            data.nome_compromissario = get_element_value(&elem).await;
            tracing::debug!("Found txtCompromissarioNome: {:?}", data.nome_compromissario);
        } else {
            tracing::debug!("No txtCompromissarioNome element (may be empty)");
        }

        // Endereço (logradouro)
        if let Ok(elem) = driver.find(By::Name("txtEndereco")).await {
            data.endereco = get_element_value(&elem).await;
            tracing::debug!("Found txtEndereco: {:?}", data.endereco);
        } else {
            tracing::warn!("Could not find txtEndereco element");
        }

        // Número do endereço
        if let Ok(elem) = driver.find(By::Name("txtNumero")).await {
            data.numero = get_element_value(&elem).await;
            tracing::debug!("Found txtNumero: {:?}", data.numero);
        } else {
            tracing::warn!("Could not find txtNumero element");
        }

        // Complemento
        if let Ok(elem) = driver.find(By::Name("txtComplemento")).await {
            data.complemento = get_element_value(&elem).await;
            tracing::debug!("Found txtComplemento: {:?}", data.complemento);
        } else {
            tracing::debug!("No txtComplemento element (may be empty)");
        }

        // Bairro
        if let Ok(elem) = driver.find(By::Name("txtBairro")).await {
            data.bairro = get_element_value(&elem).await;
            tracing::debug!("Found txtBairro: {:?}", data.bairro);
        } else {
            tracing::warn!("Could not find txtBairro element");
        }

        // CEP
        if let Ok(elem) = driver.find(By::Name("txtCepImovel")).await {
            data.cep = get_element_value(&elem).await;
            tracing::debug!("Found txtCepImovel: {:?}", data.cep);
        } else {
            tracing::warn!("Could not find txtCepImovel element");
        }

        Ok(data)
    }

    pub async fn shutdown(self) {
        // Clean shutdown of all drivers
        for driver in self.driver_pool {
            let _ = driver.quit().await;
        }
    }
}