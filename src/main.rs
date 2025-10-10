mod scraper;
mod supabase;

use anyhow::Result;
use clap::{Parser, Subcommand};
use rand::Rng;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

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

#[derive(Parser)]
#[command(name = "iptu-cli")]
#[command(about = "IPTU Data Extraction CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch pending jobs from Supabase and process them
    Process {
        /// Number of jobs to fetch
        #[arg(short, long, default_value_t = 10)]
        limit: usize,

        /// Number of concurrent scrapers
        #[arg(short, long, default_value_t = 1)]
        concurrent: usize,

        /// Run in headless mode
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        headless: bool,

        /// Rate limit per hour
        #[arg(short, long, default_value_t = 100)]
        rate_limit: usize,

        /// File with contributor numbers (one per line)
        #[arg(short, long)]
        file: Option<String>,

        /// Direct contributor numbers (comma-separated)
        #[arg(long)]
        numbers: Option<String>,
    },

    /// Fetch pending jobs from Supabase (without processing)
    Fetch {
        /// Number of jobs to fetch
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },

    /// Get results from Supabase
    Results {
        /// Number of results to fetch
        #[arg(short, long, default_value_t = 10)]
        limit: i32,

        /// Offset
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
    let mut results = Vec::new();
    let total_items = contributor_numbers.len();

    info!(
        "Processing {} items individually (already marked as 'p')",
        total_items
    );

    // Process each item in the block individually
    for (idx, contributor_number) in contributor_numbers.iter().enumerate() {
        let item_num = idx + 1;
        info!(
            "  Item {}/{}: Starting processing for {}",
            item_num, total_items, contributor_number
        );

        // Process single job with scraper
        let job_results = scraper
            .process_batch_with_callback(
                vec![contributor_number.clone()],
                move |result: &scraper::ScraperResult, _completed, _total| {
                    if result.success {
                        info!(
                            "  Item {}/{}: ✓ Successfully scraped {}",
                            item_num, total_items, result.contributor_number
                        );
                    } else {
                        info!(
                            "  Item {}/{}: ✗ Failed to scrape {}: {:?}",
                            item_num, total_items, result.contributor_number, result.error
                        );
                    }
                },
            )
            .await;

        if let Some(result) = job_results.into_iter().next() {
            // Convert to Supabase format and upload
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

            // Upload result to database
            if let Err(e) = client.upload_results(vec![iptu_result]).await {
                tracing::error!(
                    "  Item {}/{}: Failed to upload result: {}",
                    item_num,
                    total_items,
                    e
                );
            } else {
                info!(
                    "  Item {}/{}: Uploaded result to database",
                    item_num, total_items
                );
            }

            // Update status from 'p' to 's' (success) or 'e' (error)
            if result.success && result.nome_proprietario.is_some() {
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
            } else {
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
    }

    info!(
        "Block processing complete: {} items processed",
        results.len()
    );
    Ok(results)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load environment variables
    dotenv::dotenv().ok();

    let cli = Cli::parse();

    // Get Supabase credentials from environment
    let supabase_url = std::env::var("SUPABASE_URL").expect("SUPABASE_URL must be set");
    let supabase_anon_key =
        std::env::var("SUPABASE_ANON_KEY").expect("SUPABASE_ANON_KEY must be set");
    let supabase_service_role = std::env::var("SUPABASE_SERVICE_ROLE_KEY").ok();

    // Create Supabase client
    let mut client = SupabaseClient::new(supabase_url, supabase_anon_key);
    if let Some(service_role) = supabase_service_role {
        client = client.with_service_role(service_role);
    }

    match cli.command {
        Commands::Process {
            limit,
            concurrent,
            headless,
            rate_limit,
            file,
            numbers,
        } => {
            // Start timer
            let start_time = Instant::now();

            // Start ChromeDriver
            info!("Attempting to start ChromeDriver...");
            let status = Command::new("sh")
                .arg("start.chromedriver.sh")
                .status()
                .expect("Failed to execute start.chromedriver.sh script.");

            if !status.success() {
                anyhow::bail!("ChromeDriver script failed to execute successfully. Please check chromedriver.log for details.");
            }
            info!("ChromeDriver script executed. Check logs for status.");

            const BLOCK_SIZE: usize = 5;

            // Initialize scraper once
            let config = ScraperConfig {
                max_concurrent: concurrent,
                headless,
                timeout_secs: 60,
                retry_attempts: 4,
                rate_limit_per_hour: rate_limit,
            };

            info!(
                "Initializing scraper with {} concurrent workers...",
                concurrent
            );
            let scraper = ScraperEngine::new(config).await?;

            let client_arc = Arc::new(client);

            let mut all_results = Vec::new();
            let mut total_processed = 0;
            let mut total_success = 0;
            let mut total_error = 0;

            // Determine source and process in blocks
            if let Some(file_path) = file {
                // Read from file
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

                // Process in blocks
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
                        None,  // No batch ID for file processing
                        false, // Not from priority table
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

                    // Add delay between blocks (8-12 seconds)
                    if block_idx < contributor_numbers.chunks(BLOCK_SIZE).count() - 1 {
                        let mut rng = rand::thread_rng();
                        let delay_secs = rng.gen_range(8..=12);
                        info!("⏸️  Waiting {} seconds before next block...", delay_secs);
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                    }
                }
            } else if let Some(nums) = numbers {
                // Parse from comma-separated string
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

                // Process in blocks
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
                        None,  // No batch ID for manual processing
                        false, // Not from priority table
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

                    // Add delay between blocks (8-12 seconds)
                    if block_idx < contributor_numbers.chunks(BLOCK_SIZE).count() - 1 {
                        let mut rng = rand::thread_rng();
                        let delay_secs = rng.gen_range(8..=12);
                        info!("⏸️  Waiting {} seconds before next block...", delay_secs);
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                    }
                }
            } else {
                // Fetch from Supabase in blocks
                info!(
                    "Will fetch and process {} items from Supabase in blocks of {}",
                    limit, BLOCK_SIZE
                );

                // Create batch for entire operation
                let batch_id = client_arc.create_batch(limit as i32).await?;
                info!("Created batch: {}", batch_id);

                let total_blocks = limit.div_ceil(BLOCK_SIZE);

                for block_idx in 0..total_blocks {
                    let block_num = block_idx + 1;
                    let block_size = std::cmp::min(BLOCK_SIZE, limit - (block_idx * BLOCK_SIZE));

                    info!("========== Block {}/{} ==========", block_num, total_blocks);
                    info!("Fetching {} items from Supabase...", block_size);

                    // Fetch block from Supabase
                    let jobs = client_arc.fetch_pending_jobs(block_size).await?;

                    if jobs.is_empty() {
                        info!("No more pending jobs found");
                        break;
                    }

                    info!("Found {} pending jobs in block {}", jobs.len(), block_num);

                    // Check if jobs are from priority table
                    let from_priority_table =
                        jobs.first().map(|j| j.from_priority_table).unwrap_or(false);
                    if from_priority_table {
                        info!("Processing priority jobs from iptus_list_priority table");
                    }

                    // Extract contributor numbers
                    let contributor_numbers: Vec<String> =
                        jobs.iter().map(|j| j.contributor_number.clone()).collect();

                    // Claim all jobs in the block at once (mark as 'p')
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

                    // Process each item in the block individually
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

                    // Update batch progress
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

                    // Break if we've processed enough
                    if total_processed >= limit {
                        break;
                    }

                    // Add delay between blocks (8-12 seconds)
                    if block_idx < total_blocks - 1 && total_processed < limit {
                        let mut rng = rand::thread_rng();
                        let delay_secs = rng.gen_range(8..=12);
                        info!("⏸️  Waiting {} seconds before next block...", delay_secs);
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                    }
                }

                // Complete batch
                if total_processed > 0 {
                    client_arc.complete_batch(&batch_id).await?;
                    info!("Batch {} completed", batch_id);
                }
            }

            info!("========== Processing Complete ==========");
            info!("Total processed: {}", total_processed);
            info!("Success: {}, Errors: {}", total_success, total_error);

            // Calculate and display performance report
            let duration = start_time.elapsed().as_secs_f64();
            let report =
                PerformanceReport::new(total_processed, total_success, total_error, duration);
            report.display();

            // Shutdown scraper
            scraper.shutdown().await;
        }

        Commands::Fetch { limit } => {
            info!("Fetching {} pending jobs from Supabase...", limit);

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
