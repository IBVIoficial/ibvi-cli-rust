mod supabase;
mod scraper;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{info, warn};
use std::sync::Arc;

use supabase::SupabaseClient;
use scraper::{ScraperConfig, ScraperEngine};

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

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into())
        )
        .init();

    // Load environment variables
    dotenv::dotenv().ok();

    let cli = Cli::parse();

    // Get Supabase credentials from environment
    let supabase_url = std::env::var("SUPABASE_URL")
        .expect("SUPABASE_URL must be set");
    let supabase_anon_key = std::env::var("SUPABASE_ANON_KEY")
        .expect("SUPABASE_ANON_KEY must be set");
    let supabase_service_role = std::env::var("SUPABASE_SERVICE_ROLE_KEY").ok();

    // Create Supabase client
    let mut client = SupabaseClient::new(supabase_url, supabase_anon_key);
    if let Some(service_role) = supabase_service_role {
        client = client.with_service_role(service_role);
    }

    match cli.command {
        Commands::Process { limit, concurrent, headless, rate_limit, file, numbers } => {
            let mut contributor_numbers: Vec<String>;
            let mut from_priority_table = false;

            // Determine source of contributor numbers
            if let Some(file_path) = file {
                // Read from file
                info!("Reading contributor numbers from file: {}", file_path);
                let contents = std::fs::read_to_string(file_path)?;
                contributor_numbers = contents.lines()
                    .map(|line| line.trim().to_string())
                    .filter(|line| !line.is_empty())
                    .collect();
                info!("Found {} contributor numbers in file", contributor_numbers.len());
            } else if let Some(nums) = numbers {
                // Parse from comma-separated string
                info!("Processing provided contributor numbers");
                contributor_numbers = nums.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                info!("Processing {} provided contributor numbers", contributor_numbers.len());
            } else {
                // Fetch from Supabase
                info!("Fetching {} pending jobs from Supabase...", limit);
                let jobs = client.fetch_pending_jobs(limit).await?;

                if jobs.is_empty() {
                    info!("No pending jobs found");
                    return Ok(());
                }

                info!("Found {} pending jobs", jobs.len());

                // Check if jobs are from priority table
                from_priority_table = jobs.first().map(|j| j.from_priority_table).unwrap_or(false);
                if from_priority_table {
                    info!("Processing priority jobs from iptus_list_priority table");
                }

                // Extract contributor numbers
                contributor_numbers = jobs.iter()
                    .map(|j| j.contributor_number.clone())
                    .collect();

                // Claim the jobs
                info!("Claiming jobs...");
                let machine_id = "cli".to_string();
                client.claim_jobs(contributor_numbers.clone(), &machine_id, from_priority_table).await?;
            }

            // Remove duplicates while preserving order
            let mut seen = std::collections::HashSet::new();
            let original_count = contributor_numbers.len();
            contributor_numbers.retain(|item| seen.insert(item.clone()));

            if contributor_numbers.len() != original_count {
                warn!("Found {} duplicate jobs, processing {} unique jobs",
                    original_count - contributor_numbers.len(),
                    contributor_numbers.len());
            }

            if contributor_numbers.is_empty() {
                info!("No contributor numbers to process");
                return Ok(());
            }

            // Log the contributor numbers
            for (idx, num) in contributor_numbers.iter().enumerate() {
                info!("Job {}: {}", idx + 1, num);
            }

            // Create batch
            let batch_id = client.create_batch(contributor_numbers.len() as i32).await?;
            info!("Created batch: {}", batch_id);

            // Initialize scraper
            let config = ScraperConfig {
                max_concurrent: concurrent,
                headless,
                timeout_secs: 60,
                retry_attempts: 4,
                rate_limit_per_hour: rate_limit,
            };

            info!("Initializing scraper with {} concurrent workers...", concurrent);
            let scraper = ScraperEngine::new(config).await?;

            // Process jobs
            let batch_id_clone = batch_id.clone();
            let client_arc = Arc::new(client);
            let client_for_callback = client_arc.clone();

            let results = scraper.process_batch_with_callback(
                contributor_numbers.clone(),
                move |result: &scraper::ScraperResult, completed, total| {
                    info!("Progress: {}/{}", completed, total);

                    // Upload each result immediately after processing
                    let client = client_for_callback.clone();
                    let batch_id = batch_id_clone.clone();
                    let result_clone = result.clone();
                    let from_priority = from_priority_table;

                    tokio::spawn(async move {
                        // Convert to Supabase format
                        let now = chrono::Utc::now().to_rfc3339();
                        let iptu_result = crate::supabase::IPTUResult {
                            id: Some(uuid::Uuid::new_v4().to_string()),
                            contributor_number: result_clone.contributor_number.clone(),
                            numero_cadastro: result_clone.numero_cadastro.clone(),
                            nome_proprietario: result_clone.nome_proprietario.clone(),
                            nome_compromissario: result_clone.nome_compromissario.clone(),
                            endereco: result_clone.endereco.clone(),
                            numero: result_clone.numero.clone(),
                            complemento: result_clone.complemento.clone(),
                            bairro: result_clone.bairro.clone(),
                            cep: result_clone.cep.clone(),
                            sucesso: result_clone.success,
                            erro: result_clone.error.clone(),
                            batch_id: Some(batch_id.clone()),
                            timestamp: now,
                            processed_by: Some("cli".to_string()),
                        };

                        // Upload immediately
                        if let Err(e) = client.upload_results(vec![iptu_result]).await {
                            tracing::error!("Failed to upload result for {}: {}", result_clone.contributor_number, e);
                        } else {
                            tracing::info!("Successfully uploaded result for {}", result_clone.contributor_number);

                            // Mark as success or error in the appropriate table
                            if result_clone.success && result_clone.nome_proprietario.is_some() {
                                if let Err(e) = client.mark_iptu_list_as_success(vec![result_clone.contributor_number.clone()], from_priority).await {
                                    tracing::error!("Failed to mark as success: {}", e);
                                }
                            } else if !result_clone.success {
                                if let Err(e) = client.mark_iptu_list_as_error(vec![result_clone.contributor_number], from_priority).await {
                                    tracing::error!("Failed to mark as error: {}", e);
                                }
                            }
                        }

                        // Update batch progress
                        if let Err(e) = client.update_batch_progress(
                            &batch_id,
                            completed as i32,
                            if result_clone.success { 1 } else { 0 },
                            if !result_clone.success { 1 } else { 0 },
                        ).await {
                            tracing::error!("Failed to update batch progress: {}", e);
                        }
                    });
                }
            ).await;

            // All results have already been uploaded immediately after each scrape
            info!("All results have been uploaded to Supabase");

            // Final batch update with totals
            let success_count = results.iter().filter(|r| r.success).count() as i32;
            let error_count = results.iter().filter(|r| !r.success).count() as i32;

            client_arc.update_batch_progress(
                &batch_id,
                results.len() as i32,
                success_count,
                error_count,
            ).await?;

            // Complete batch
            client_arc.complete_batch(&batch_id).await?;

            info!("Processing complete!");
            info!("Success: {}, Errors: {}", success_count, error_count);

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
                    println!("  - {} | Success: {} | Owner: {:?}",
                        result.contributor_number,
                        result.sucesso,
                        result.nome_proprietario
                    );
                }
            }
        }
    }

    Ok(())
}
