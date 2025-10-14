mod diretrix_scraper;
mod scraper;
mod supabase;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rand::Rng;
use std::collections::HashMap;
use std::io::{self, Write};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};

use diretrix_scraper::{DiretrixScraper, PropertyRecord};
use scraper::{ScraperConfig, ScraperEngine};
use supabase::SupabaseClient;

struct PerformanceReport {
    total_jobs: usize,
    successful: usize,
    failed: usize,
    duration_secs: f64,
    jobs_per_minute: f64,
    success_rate: f64,
}

impl PerformanceReport {
    fn new(total_jobs: usize, successful: usize, failed: usize, duration_secs: f64) -> Self {
        let jobs_per_minute = if duration_secs > 0.0 {
            (total_jobs as f64 / duration_secs) * 60.0
        } else {
            0.0
        };

        let success_rate = if total_jobs > 0 {
            (successful as f64 / total_jobs as f64) * 100.0
        } else {
            0.0
        };

        Self {
            total_jobs,
            successful,
            failed,
            duration_secs,
            jobs_per_minute,
            success_rate,
        }
    }

    fn format_duration(&self) -> String {
        let total_secs = self.duration_secs as u64;
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;

        if hours > 0 {
            format!("{}h {}m {}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        }
    }

    fn display(&self) {
        println!("\n╔══════════════════════════════════════════════════════════╗");
        println!("║              PERFORMANCE REPORT                          ║");
        println!("╠══════════════════════════════════════════════════════════╣");
        println!("║  Total Jobs:              {:>30} ║", self.total_jobs);
        println!("║  Successful:              {:>30} ║", self.successful);
        println!("║  Failed:                  {:>30} ║", self.failed);
        println!(
            "║  Duration:                {:>30} ║",
            self.format_duration()
        );
        println!(
            "║  Throughput:              {:>26.2}/min ║",
            self.jobs_per_minute
        );
        println!(
            "║  Success Rate:            {:>27.1}%   ║",
            self.success_rate
        );

        // Performance status based on success rate and throughput
        let status = if self.success_rate >= 90.0 && self.jobs_per_minute >= 5.0 {
            "🟢 EXCELLENT"
        } else if self.success_rate >= 75.0 && self.jobs_per_minute >= 3.0 {
            "🟡 GOOD"
        } else if self.success_rate >= 50.0 {
            "🟠 MODERATE"
        } else {
            "🔴 NEEDS IMPROVEMENT"
        };

        println!("║  Status:                  {:>30}║", status);
        println!("╚══════════════════════════════════════════════════════════╝\n");
    }
}

fn prompt_non_empty(prompt: &str) -> Result<String> {
    loop {
        print!("{}", prompt);
        io::stdout()
            .flush()
            .context("Failed to flush stdout while prompting for input")?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("Failed to read prompt input")?;

        let trimmed = input.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }

        println!("Input cannot be empty. Please try again.\n");
    }
}

fn sanitize_iptu(value: &str) -> String {
    value.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn resolve_credential(value: Option<String>, env_key: &str, prompt: &str) -> Result<String> {
    if let Some(val) = value {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Ok(val) = std::env::var(env_key) {
        if !val.trim().is_empty() {
            return Ok(val);
        }
    }
    prompt_non_empty(prompt)
}

async fn fetch_diretrix_records(
    street_name: &str,
    street_number: &str,
    headless: bool,
    username: &str,
    password: &str,
    webdriver_url_override: Option<&str>,
) -> Result<Vec<PropertyRecord>> {
    let webdriver_url = webdriver_url_override
        .map(|s| s.to_string())
        .or_else(|| std::env::var("DIRETRIX_WEBDRIVER_URL").ok())
        .unwrap_or_else(|| "http://localhost:9515".to_string());

    info!(
        "Connecting to Diretrix with user {} to search {} {}",
        username, street_name, street_number
    );

    let diretrix_scraper = DiretrixScraper::new(
        username.to_string(),
        password.to_string(),
        &webdriver_url,
        headless,
    )
    .await?;

    diretrix_scraper.login().await?;

    let search_result = diretrix_scraper
        .search_by_address(street_name, street_number)
        .await;

    if let Err(e) = diretrix_scraper.close().await {
        warn!("Failed to close Diretrix browser session cleanly: {}", e);
    }

    let records = search_result?;
    if records.is_empty() {
        info!(
            "Diretrix search returned no records for {} {}",
            street_name, street_number
        );
    } else {
        info!(
            "Diretrix search returned {} records for {} {}",
            records.len(),
            street_name,
            street_number
        );
    }

    Ok(records)
}

fn print_diretrix_records(records: &[PropertyRecord]) {
    println!(
        "\n{:<4} {:<35} {:<14} {:<25} {:<8} {:<20} {:<20} {:<18}",
        "#", "Owner", "IPTU", "Street", "Number", "Complement", "Complement 2", "Neighborhood"
    );
    println!("{}", "-".repeat(150));

    for (idx, record) in records.iter().enumerate() {
        println!(
            "{:<4} {:<35} {:<14} {:<25} {:<8} {:<20} {:<20} {:<18}",
            idx + 1,
            record.owner.trim(),
            record.iptu.trim(),
            record.street.trim(),
            record.number.trim(),
            record.complement.trim(),
            record.complement2.trim(),
            record.neighborhood.trim()
        );
    }
}

fn start_chromedriver() -> Result<()> {
    info!("Attempting to start ChromeDriver...");
    let status = Command::new("sh")
        .arg("start.chromedriver.sh")
        .status()
        .context("Failed to execute start.chromedriver.sh script.")?;

    if !status.success() {
        bail!("ChromeDriver script failed to execute successfully. Please check chromedriver.log.");
    }
    info!("ChromeDriver script executed. Check logs for status.");
    Ok(())
}

fn build_supabase_client() -> Result<SupabaseClient> {
    let supabase_url = std::env::var("SUPABASE_URL").context("SUPABASE_URL must be set")?;
    let supabase_anon_key =
        std::env::var("SUPABASE_ANON_KEY").context("SUPABASE_ANON_KEY must be set")?;
    let supabase_service_role = std::env::var("SUPABASE_SERVICE_ROLE_KEY").ok();

    let mut client = SupabaseClient::new(supabase_url, supabase_anon_key);
    if let Some(service_role) = supabase_service_role {
        client = client.with_service_role(service_role);
    }

    Ok(client)
}

#[derive(Parser)]
#[command(name = "iptu-cli")]
#[command(about = "IPTU Data Extraction CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Process {
        #[arg(short, long, default_value_t = 10)]
        limit: usize,

        #[arg(short, long, default_value_t = 1)]
        concurrent: usize,

        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        headless: bool,

        #[arg(short, long, default_value_t = 100)]
        rate_limit: usize,

        #[arg(short, long)]
        file: Option<String>,

        #[arg(long)]
        numbers: Option<String>,

        #[arg(long, default_value_t = false)]
        from_diretrix: bool,

        #[arg(long)]
        street: Option<String>,

        #[arg(long = "street-number")]
        street_number: Option<String>,
    },

    Diretrix {
        #[arg(long)]
        street: Option<String>,

        #[arg(long = "street-number")]
        street_number: Option<String>,

        #[arg(long)]
        username: Option<String>,

        #[arg(long)]
        password: Option<String>,

        #[arg(long)]
        webdriver_url: Option<String>,

        #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
        headless: bool,
    },

    Fetch {
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },

    Results {
        #[arg(short, long, default_value_t = 10)]
        limit: i32,

        #[arg(short, long, default_value_t = 0)]
        offset: i32,
    },
}

async fn process_block(
    scraper: &ScraperEngine,
    contributor_numbers: Vec<String>,
    client: &Arc<SupabaseClient>,
    batch_id: Option<String>,
    from_priority_table: bool,
) -> Result<Vec<scraper::ScraperResult>> {
    let total_items = contributor_numbers.len();

    info!(
        "Processing {} items concurrently in this block",
        total_items
    );

    // Process all items in the block concurrently using process_batch_with_callback
    let job_results = scraper
        .process_batch_with_callback(
            contributor_numbers.clone(),
            move |result: &scraper::ScraperResult, completed, total| {
                if result.success {
                    info!(
                        "  [{}/{}] ✓ Successfully scraped {}",
                        completed, total, result.contributor_number
                    );
                } else {
                    info!(
                        "  [{}/{}] ✗ Failed to scrape {}: {:?}",
                        completed, total, result.contributor_number, result.error
                    );
                }
            },
        )
        .await;

    // Now handle database operations for all results
    let mut results = Vec::new();
    for (idx, result) in job_results.into_iter().enumerate() {
        let item_num = idx + 1;

        let now = chrono::Utc::now().to_rfc3339();
        let iptu_result = crate::supabase::IPTUResult {
            id: Some(uuid::Uuid::new_v4().to_string()),
            contributor_number: result.contributor_number.clone(),
            numero_cadastro: result.numero_cadastro.clone(),
            nome_proprietario: result.nome_proprietario.clone(),
            nome_compromissario: result.nome_compromissario.clone(),
            endereco: result.endereco.clone(),
            numero: result.numero.clone(),
            complemento: result.complemento.clone(),
            bairro: result.bairro.clone(),
            cep: result.cep.clone(),
            sucesso: result.success,
            erro: result.error.clone(),
            batch_id: batch_id.clone(),
            timestamp: now,
            processed_by: Some("cli".to_string()),
        };

        // Só salvar na tabela iptus se foi bem-sucedido
        if result.success {
            // Verificar se já existe um registro com este contributor_number
            let already_exists = match client.check_existing_iptu(&result.contributor_number).await
            {
                Ok(exists) => exists,
                Err(e) => {
                    tracing::error!(
                        "  Item {}/{}: Failed to check existing IPTU: {}",
                        item_num,
                        total_items,
                        e
                    );
                    false // Em caso de erro, tentamos salvar mesmo assim
                }
            };

            if !already_exists {
                if let Err(e) = client.upload_results(vec![iptu_result]).await {
                    tracing::error!(
                        "  Item {}/{}: Failed to upload result: {}",
                        item_num,
                        total_items,
                        e
                    );
                } else {
                    info!(
                        "  Item {}/{}: ✓ Uploaded new result to database",
                        item_num, total_items
                    );
                }
            } else {
                info!(
                    "  Item {}/{}: ⏭️  Skipped upload - contributor_number {} already exists in iptus table",
                    item_num, total_items, result.contributor_number
                );
            }

            // Marcar como sucesso na lista de controle
            if result.nome_proprietario.is_some() {
                info!(
                    "  Item {}/{}: Updating status from 'p' to 's' (success)",
                    item_num, total_items
                );
                if let Err(e) = client
                    .mark_iptu_list_as_success(
                        vec![result.contributor_number.clone()],
                        from_priority_table,
                    )
                    .await
                {
                    tracing::error!(
                        "  Item {}/{}: Failed to mark as success: {}",
                        item_num,
                        total_items,
                        e
                    );
                } else {
                    info!(
                        "  Item {}/{}: ✓ Status updated to 's'",
                        item_num, total_items
                    );
                }
            }
        } else {
            // Falha no scraping - NÃO salvar na tabela iptus, apenas marcar como erro
            info!(
                "  Item {}/{}: ❌ Scraping failed - NOT saving to iptus table",
                item_num, total_items
            );
            info!(
                "  Item {}/{}: Updating status from 'p' to 'e' (error)",
                item_num, total_items
            );
            if let Err(e) = client
                .mark_iptu_list_as_error(
                    vec![result.contributor_number.clone()],
                    from_priority_table,
                )
                .await
            {
                tracing::error!(
                    "  Item {}/{}: Failed to mark as error: {}",
                    item_num,
                    total_items,
                    e
                );
            } else {
                info!(
                    "  Item {}/{}: ✓ Status updated to 'e'",
                    item_num, total_items
                );
            }
        }

        info!("  Item {}/{}: Complete", item_num, total_items);
        results.push(result);
    }

    info!(
        "Block processing complete: {} items processed",
        results.len()
    );
    Ok(results)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    dotenv::dotenv().ok();

    let cli = Cli::parse();

    match cli.command {
        Commands::Process {
            limit,
            concurrent,
            headless,
            rate_limit,
            file,
            numbers,
            from_diretrix,
            street,
            street_number,
        } => {
            let start_time = Instant::now();
            let use_diretrix = from_diretrix || street.is_some() || street_number.is_some();

            if use_diretrix && (file.is_some() || numbers.is_some()) {
                bail!("Address mode cannot be combined with --file or --numbers options");
            }

            start_chromedriver()?;

            const BLOCK_SIZE: usize = 12;

            let config = ScraperConfig {
                max_concurrent: concurrent,
                headless,
                timeout_secs: 60,
                retry_attempts: 4,
                rate_limit_per_hour: rate_limit,
            };

            if use_diretrix {
                let street_name = match street {
                    Some(value) if !value.trim().is_empty() => value.trim().to_string(),
                    _ => prompt_non_empty("Street name: ")?,
                };

                let street_number_value = match street_number {
                    Some(value) if !value.trim().is_empty() => value.trim().to_string(),
                    _ => prompt_non_empty("Street number: ")?,
                };

                let username =
                    resolve_credential(None, "DIRETRIX_USERNAME", "Diretrix username: ")?;
                let password =
                    resolve_credential(None, "DIRETRIX_PASSWORD", "Diretrix password: ")?;
                let webdriver_url_env = std::env::var("DIRETRIX_WEBDRIVER_URL").ok();

                let properties = fetch_diretrix_records(
                    &street_name,
                    &street_number_value,
                    headless,
                    &username,
                    &password,
                    webdriver_url_env.as_deref(),
                )
                .await?;

                if properties.is_empty() {
                    info!("No IPTU numbers found for the provided address. Nothing to process.");
                    return Ok(());
                }

                info!(
                    "Preparing to scrape {} IPTU numbers from Diretrix results",
                    properties.len()
                );
                for (idx, record) in properties.iter().enumerate() {
                    info!(
                        "  {:>2}. {} | IPTU: {} | {} {}",
                        idx + 1,
                        record.owner,
                        record.iptu.trim(),
                        record.street.trim(),
                        record.number.trim()
                    );
                }

                let mut property_lookup: HashMap<String, PropertyRecord> = HashMap::new();
                let mut jobs: Vec<String> = Vec::new();

                for record in &properties {
                    let sanitized = sanitize_iptu(&record.iptu);
                    if sanitized.len() != 11 {
                        warn!(
                            "Skipping IPTU {} ({}) because sanitized value does not have 11 digits",
                            record.iptu, sanitized
                        );
                        continue;
                    }

                    if property_lookup.contains_key(&sanitized) {
                        warn!(
                            "Duplicate IPTU detected in Diretrix results: {}",
                            record.iptu
                        );
                    }

                    property_lookup.insert(sanitized.clone(), record.clone());
                    jobs.push(sanitized);
                }

                if jobs.is_empty() {
                    bail!("No valid IPTU numbers found after sanitizing Diretrix results");
                }

                info!(
                    "Initializing IPTU scraper with {} concurrent workers...",
                    concurrent
                );
                let scraper = ScraperEngine::new(config).await?;

                let property_lookup = Arc::new(property_lookup);
                let property_lookup_for_logs = Arc::clone(&property_lookup);

                let job_results = scraper
                    .process_batch_with_callback(
                        jobs.clone(),
                        move |result: &scraper::ScraperResult, completed, total| {
                            let key = sanitize_iptu(&result.contributor_number);
                            if result.success {
                                if let Some(property) = property_lookup_for_logs.get(&key) {
                                    info!(
                                        "  [{}/{}] ✓ {} | IPTU {}",
                                        completed,
                                        total,
                                        property.owner,
                                        property.iptu.trim()
                                    );
                                } else {
                                    info!(
                                        "  [{}/{}] ✓ Successfully scraped {}",
                                        completed, total, result.contributor_number
                                    );
                                }
                            } else if let Some(property) = property_lookup_for_logs.get(&key) {
                                info!(
                                    "  [{}/{}] ✗ Failed to scrape IPTU {} ({}) : {:?}",
                                    completed,
                                    total,
                                    property.iptu.trim(),
                                    property.owner,
                                    result.error
                                );
                            } else {
                                info!(
                                    "  [{}/{}] ✗ Failed to scrape {}: {:?}",
                                    completed, total, result.contributor_number, result.error
                                );
                            }
                        },
                    )
                    .await;

                let total_processed = job_results.len();
                let total_success = job_results.iter().filter(|r| r.success).count();
                let total_error = total_processed - total_success;

                info!("========== Processing Complete ==========");
                info!("Total processed: {}", total_processed);
                info!("Success: {}, Errors: {}", total_success, total_error);

                let duration = start_time.elapsed().as_secs_f64();
                PerformanceReport::new(total_processed, total_success, total_error, duration)
                    .display();

                if let Ok(property_lookup) = Arc::try_unwrap(property_lookup) {
                    if !property_lookup.is_empty() {
                        info!("Detailed results from Diretrix-IPTU pipeline:");
                        for result in &job_results {
                            let key = sanitize_iptu(&result.contributor_number);
                            if let Some(property) = property_lookup.get(&key) {
                                info!(
                                    "- IPTU {} | Owner: {} | Success: {} | Error: {:?}",
                                    property.iptu.trim(),
                                    property.owner,
                                    result.success,
                                    result.error
                                );
                            }
                        }
                    }
                }

                scraper.shutdown().await;
            } else {
                info!(
                    "Initializing scraper with {} concurrent workers...",
                    concurrent
                );
                let scraper = ScraperEngine::new(config).await?;

                let client = build_supabase_client()?;
                let client_arc = Arc::new(client);

                let mut all_results = Vec::new();
                let mut total_processed = 0;
                let mut total_success = 0;
                let mut total_error = 0;

                if let Some(file_path) = file {
                    info!("Reading contributor numbers from file: {}", file_path);
                    let contents = std::fs::read_to_string(file_path)?;
                    let contributor_numbers: Vec<String> = contents
                        .lines()
                        .map(|line| line.trim().to_string())
                        .filter(|line| !line.is_empty())
                        .collect();
                    info!(
                        "Found {} contributor numbers in file",
                        contributor_numbers.len()
                    );

                    for (block_idx, block) in contributor_numbers.chunks(BLOCK_SIZE).enumerate() {
                        let block_num = block_idx + 1;
                        info!(
                            "========== Processing Block {}/{} ==========",
                            block_num,
                            contributor_numbers.len().div_ceil(BLOCK_SIZE)
                        );

                        let results = crate::process_block(
                            &scraper,
                            block.to_vec(),
                            &client_arc,
                            None,
                            false,
                        )
                        .await?;

                        let block_success = results.iter().filter(|r| r.success).count();
                        let block_error = results.len() - block_success;

                        total_processed += results.len();
                        total_success += block_success;
                        total_error += block_error;

                        info!(
                            "Block {} complete: {} success, {} errors",
                            block_num, block_success, block_error
                        );

                        all_results.extend(results);

                        if block_idx < contributor_numbers.chunks(BLOCK_SIZE).count() - 1 {
                            let mut rng = rand::thread_rng();
                            let delay_secs = rng.gen_range(8..=12);
                            info!("⏸️  Waiting {} seconds before next block...", delay_secs);
                            tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                        }
                    }
                } else if let Some(nums) = numbers {
                    info!("Processing provided contributor numbers");
                    let contributor_numbers: Vec<String> = nums
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    info!(
                        "Processing {} provided contributor numbers",
                        contributor_numbers.len()
                    );

                    for (block_idx, block) in contributor_numbers.chunks(BLOCK_SIZE).enumerate() {
                        let block_num = block_idx + 1;
                        info!(
                            "========== Processing Block {}/{} ==========",
                            block_num,
                            contributor_numbers.len().div_ceil(BLOCK_SIZE)
                        );

                        let results = crate::process_block(
                            &scraper,
                            block.to_vec(),
                            &client_arc,
                            None,
                            false,
                        )
                        .await?;

                        let block_success = results.iter().filter(|r| r.success).count();
                        let block_error = results.len() - block_success;

                        total_processed += results.len();
                        total_success += block_success;
                        total_error += block_error;

                        info!(
                            "Block {} complete: {} success, {} errors",
                            block_num, block_success, block_error
                        );

                        all_results.extend(results);

                        if block_idx < contributor_numbers.chunks(BLOCK_SIZE).count() - 1 {
                            let mut rng = rand::thread_rng();
                            let delay_secs = rng.gen_range(8..=12);
                            info!("⏸️  Waiting {} seconds before next block...", delay_secs);
                            tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                        }
                    }
                } else {
                    info!(
                        "Will fetch and process {} items from Supabase in blocks of {}",
                        limit, BLOCK_SIZE
                    );

                    let batch_id = client_arc.create_batch(limit as i32).await?;
                    info!("Created batch: {}", batch_id);

                    let total_blocks = limit.div_ceil(BLOCK_SIZE);

                    for block_idx in 0..total_blocks {
                        let block_num = block_idx + 1;
                        let block_size =
                            std::cmp::min(BLOCK_SIZE, limit - (block_idx * BLOCK_SIZE));

                        info!("========== Block {}/{} ==========", block_num, total_blocks);
                        info!("Fetching {} items from Supabase...", block_size);

                        let jobs = client_arc.fetch_pending_jobs(block_size).await?;

                        if jobs.is_empty() {
                            info!("No more pending jobs found");
                            break;
                        }

                        info!("Found {} pending jobs in block {}", jobs.len(), block_num);

                        let from_priority_table =
                            jobs.first().map(|j| j.from_priority_table).unwrap_or(false);
                        if from_priority_table {
                            info!("Processing priority jobs from iptus_list_priority table");
                        }

                        let contributor_numbers: Vec<String> =
                            jobs.iter().map(|j| j.contributor_number.clone()).collect();

                        info!(
                            "Step 1: Claiming all {} jobs in block {} (marking as 'p')...",
                            contributor_numbers.len(),
                            block_num
                        );
                        let machine_id = "cli".to_string();
                        client_arc
                            .claim_jobs(
                                contributor_numbers.clone(),
                                &machine_id,
                                from_priority_table,
                            )
                            .await?;
                        info!(
                            "Step 1 complete: All {} jobs marked as 'p'",
                            contributor_numbers.len()
                        );

                        info!("Step 2: Processing items individually...");
                        let results = crate::process_block(
                            &scraper,
                            contributor_numbers,
                            &client_arc,
                            Some(batch_id.clone()),
                            from_priority_table,
                        )
                        .await?;

                        let block_success = results.iter().filter(|r| r.success).count();
                        let block_error = results.len() - block_success;

                        total_processed += results.len();
                        total_success += block_success;
                        total_error += block_error;

                        client_arc
                            .update_batch_progress(
                                &batch_id,
                                total_processed as i32,
                                total_success as i32,
                                total_error as i32,
                            )
                            .await?;

                        info!(
                            "Block {} complete: {} success, {} errors",
                            block_num, block_success, block_error
                        );
                        info!(
                            "Total progress: {}/{} items processed",
                            total_processed, limit
                        );

                        all_results.extend(results);

                        if total_processed >= limit {
                            break;
                        }

                        if block_idx < total_blocks - 1 && total_processed < limit {
                            let mut rng = rand::thread_rng();
                            let delay_secs = rng.gen_range(8..=12);
                            info!("⏸️  Waiting {} seconds before next block...", delay_secs);
                            tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                        }
                    }

                    if total_processed > 0 {
                        client_arc.complete_batch(&batch_id).await?;
                        info!("Batch {} completed", batch_id);
                    }
                }

                info!("========== Processing Complete ==========");
                info!("Total processed: {}", total_processed);
                info!("Success: {}, Errors: {}", total_success, total_error);

                let duration = start_time.elapsed().as_secs_f64();
                PerformanceReport::new(total_processed, total_success, total_error, duration)
                    .display();

                scraper.shutdown().await;
            }
        }

        Commands::Diretrix {
            street,
            street_number,
            username,
            password,
            webdriver_url,
            headless,
        } => {
            start_chromedriver()?;

            let street_name = match street {
                Some(value) if !value.trim().is_empty() => value.trim().to_string(),
                _ => prompt_non_empty("Street name: ")?,
            };

            let street_number_value = match street_number {
                Some(value) if !value.trim().is_empty() => value.trim().to_string(),
                _ => prompt_non_empty("Street number: ")?,
            };

            let username =
                resolve_credential(username, "DIRETRIX_USERNAME", "Diretrix username: ")?;
            let password =
                resolve_credential(password, "DIRETRIX_PASSWORD", "Diretrix password: ")?;

            let records = fetch_diretrix_records(
                &street_name,
                &street_number_value,
                headless,
                &username,
                &password,
                webdriver_url.as_deref(),
            )
            .await?;

            if records.is_empty() {
                println!(
                    "No records found for {} {} on Diretrix.",
                    street_name, street_number_value
                );
            } else {
                println!(
                    "Found {} record(s) for {} {}:\n",
                    records.len(),
                    street_name,
                    street_number_value
                );
                print_diretrix_records(&records);
            }
        }

        Commands::Fetch { limit } => {
            info!("Fetching {} pending jobs from Supabase...", limit);

            let client = build_supabase_client()?;
            let jobs = client.fetch_pending_jobs(limit).await?;

            if jobs.is_empty() {
                info!("No pending jobs found");
            } else {
                info!("Found {} pending jobs:", jobs.len());
                for job in jobs {
                    println!("  - {}", job.contributor_number);
                }
            }
        }

        Commands::Results { limit, offset } => {
            info!("Fetching results (limit: {}, offset: {})...", limit, offset);

            let client = build_supabase_client()?;
            let results = client.get_results(limit, offset).await?;

            if results.is_empty() {
                info!("No results found");
            } else {
                info!("Found {} results:", results.len());
                for result in results {
                    println!(
                        "  - {} | Success: {} | Owner: {:?}",
                        result.contributor_number, result.sucesso, result.nome_proprietario
                    );
                }
            }
        }
    }

    Ok(())
}
