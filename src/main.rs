mod supabase;
mod scraper;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;
use std::sync::Arc;

use supabase::{SupabaseClient, IPTUResult};
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
        Commands::Process { limit, concurrent, headless, rate_limit } => {
            info!("Fetching {} pending jobs from Supabase...", limit);

            // Fetch pending jobs
            let jobs = client.fetch_pending_jobs(limit).await?;

            if jobs.is_empty() {
                info!("No pending jobs found");
                return Ok(());
            }

            info!("Found {} pending jobs", jobs.len());

            // Extract contributor numbers
            let contributor_numbers: Vec<String> = jobs.iter()
                .map(|j| j.contributor_number.clone())
                .collect();

            // Claim the jobs
            info!("Claiming jobs...");
            let machine_id = "cli";
            client.claim_jobs(contributor_numbers.clone(), machine_id).await?;

            // Create batch
            let batch_id = client.create_batch(contributor_numbers.len() as i32).await?;
            info!("Created batch: {}", batch_id);

            // Initialize scraper
            let config = ScraperConfig {
                max_concurrent: concurrent,
                headless,
                timeout_secs: 30,
                retry_attempts: 3,
                rate_limit_per_hour: rate_limit,
            };

            info!("Initializing scraper with {} concurrent workers...", concurrent);
            let scraper = ScraperEngine::new(config).await?;

            // Process jobs
            let batch_id_clone = batch_id.clone();
            let client_arc = Arc::new(client);
            let client_for_callback = client_arc.clone();

            let results = scraper.process_batch_with_rate_limit(
                contributor_numbers.clone(),
                move |completed, total| {
                    info!("Progress: {}/{}", completed, total);

                    // Upload results every 10 processed
                    if completed % 10 == 0 && completed > 0 {
                        let client = client_for_callback.clone();
                        let batch_id = batch_id_clone.clone();
                        tokio::spawn(async move {
                            if let Err(e) = client.update_batch_progress(
                                &batch_id,
                                completed as i32,
                                0, // We'll update this later
                                0,
                            ).await {
                                tracing::error!("Failed to update batch progress: {}", e);
                            }
                        });
                    }
                }
            ).await;

            // Convert results to IPTUResult
            let now = chrono::Utc::now().to_rfc3339();
            let iptu_results: Vec<IPTUResult> = results.iter().map(|r| IPTUResult {
                id: Some(uuid::Uuid::new_v4().to_string()),
                contributor_number: r.contributor_number.clone(),
                numero_cadastro: r.numero_cadastro.clone(),
                nome_proprietario: r.nome_proprietario.clone(),
                nome_compromissario: r.nome_compromissario.clone(),
                endereco: r.endereco.clone(),
                numero: r.numero.clone(),
                complemento: r.complemento.clone(),
                bairro: r.bairro.clone(),
                cep: r.cep.clone(),
                sucesso: r.success,
                erro: r.error.clone(),
                batch_id: Some(batch_id.clone()),
                timestamp: now.clone(),
                processed_by: Some(machine_id.to_string()),
            }).collect();

            // Upload results
            info!("Uploading results...");
            client_arc.upload_results(iptu_results.clone()).await?;

            // Update batch progress
            let success_count = results.iter().filter(|r| r.success).count() as i32;
            let error_count = results.iter().filter(|r| !r.success).count() as i32;

            client_arc.update_batch_progress(
                &batch_id,
                results.len() as i32,
                success_count,
                error_count,
            ).await?;

            // Mark jobs as success/error
            let success_numbers: Vec<String> = results.iter()
                .filter(|r| r.success)
                .map(|r| r.contributor_number.clone())
                .collect();

            let error_numbers: Vec<String> = results.iter()
                .filter(|r| !r.success)
                .map(|r| r.contributor_number.clone())
                .collect();

            if !success_numbers.is_empty() {
                client_arc.mark_iptu_list_as_success(success_numbers).await?;
            }

            if !error_numbers.is_empty() {
                client_arc.mark_iptu_list_as_error(error_numbers).await?;
            }

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
