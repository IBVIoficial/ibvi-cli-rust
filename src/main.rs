mod dbase_scraper;
mod diretrix_enrichment;
mod diretrix_scraper;
mod enrichment_service;
mod scraper;
mod supabase;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rand::Rng;
use reqwest::{header::CONTENT_TYPE, Client as HttpClient, Response, StatusCode};
use serde_json::{self, json};
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use dbase_scraper::DbaseScraper;
use diretrix_enrichment::{GetCustomerData, WorkbuscasResponse};
use diretrix_scraper::{DiretrixScraper, PropertyRecord};
use enrichment_service::run_enrichment_server;
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

fn sanitize_document_candidate(value: &Option<String>) -> Option<String> {
    value.as_ref().and_then(|doc| {
        // Ignore documents with 'X' characters (masked/redacted CPFs)
        if doc.contains('X') || doc.contains('x') {
            return None;
        }

        let digits: String = doc.chars().filter(|c| c.is_ascii_digit()).collect();

        // Must have at least 1 digit and at most 11
        if digits.is_empty() || digits.len() > 11 {
            return None;
        }

        // Pad with leading zeros to reach 11 characters
        Some(format!("{:0>11}", digits))
    })
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

fn export_diretrix_to_csv(
    records: &[PropertyRecord],
    enrichment: &[Option<GetCustomerData>],
    filename: &str,
) -> Result<()> {
    if enrichment.len() != records.len() {
        bail!("Enrichment results count does not match records count");
    }

    let file = File::create(filename)
        .with_context(|| format!("Failed to create CSV file: {}", filename))?;

    let mut wtr = csv::Writer::from_writer(file);

    // Write header
    wtr.write_record(&[
        "Owner",
        "IPTU",
        "Street",
        "Number",
        "Complement",
        "Complement 2",
        "Neighborhood",
        "Document 1",
        "Document 2",
        "EnrichmentJSON",
    ])?;

    // Write records
    for (idx, record) in records.iter().enumerate() {
        let enrichment_json = enrichment
            .get(idx)
            .and_then(|opt| opt.as_ref())
            .and_then(|data| serde_json::to_string(data).ok())
            .unwrap_or_default();

        wtr.write_record(&[
            &record.owner,
            &record.iptu,
            &record.street,
            &record.number,
            &record.complement,
            &record.complement2,
            &record.neighborhood,
            record.document1.as_deref().unwrap_or(""),
            record.document2.as_deref().unwrap_or(""),
            &enrichment_json,
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

#[derive(Debug)]
enum EnrichmentParseError {
    BodyRead {
        status: StatusCode,
        message: String,
    },
    Html {
        status: StatusCode,
        content_type: Option<String>,
        snippet: String,
        source: &'static str,
    },
    Json {
        status: StatusCode,
        message: String,
        snippet: String,
        source: &'static str,
    },
}

impl fmt::Display for EnrichmentParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnrichmentParseError::BodyRead { status, message } => {
                write!(
                    f,
                    "Failed to read enrichment response body (status {}): {}",
                    status, message
                )
            }
            EnrichmentParseError::Html {
                status,
                content_type,
                snippet,
                source,
            } => {
                let content = content_type
                    .as_deref()
                    .map(|ct| ct.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                write!(
                    f,
                    "{} returned HTML instead of JSON (status {}, content-type {}). \
                     This usually indicates an authentication or availability issue. \
                     Body starts with: {}",
                    source, status, content, snippet
                )
            }
            EnrichmentParseError::Json {
                status,
                message,
                snippet,
                source,
            } => write!(
                f,
                "Failed to parse {} response (status {}): {}. Body starts with: {}",
                source, status, message, snippet
            ),
        }
    }
}

async fn parse_enrichment_payload(
    response: Response,
    use_workbuscas: bool,
) -> std::result::Result<Option<GetCustomerData>, EnrichmentParseError> {
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_ascii_lowercase());
    let source = if use_workbuscas {
        "Workbuscas API"
    } else {
        "local enrichment service"
    };

    let body = response
        .text()
        .await
        .map_err(|e| EnrichmentParseError::BodyRead {
            status,
            message: e.to_string(),
        })?;

    let cleaned = body.trim().trim_start_matches('\u{feff}');

    if cleaned.is_empty() {
        return Ok(None);
    }

    let trimmed_start = cleaned.trim_start();
    let looks_like_html = content_type
        .as_deref()
        .map(|ct| ct.contains("html"))
        .unwrap_or(false)
        || trimmed_start.starts_with('<');

    if looks_like_html {
        let snippet = trimmed_start.chars().take(160).collect::<String>();
        return Err(EnrichmentParseError::Html {
            status,
            content_type,
            snippet,
            source,
        });
    }

    if use_workbuscas {
        match serde_json::from_str::<WorkbuscasResponse>(cleaned) {
            Ok(data) => return Ok(Some(data.into())),
            Err(primary_err) => {
                if let Ok(as_array) = serde_json::from_str::<Vec<WorkbuscasResponse>>(cleaned) {
                    if let Some(first) = as_array.into_iter().next() {
                        return Ok(Some(first.into()));
                    }
                    return Ok(None);
                }

                let snippet = cleaned.chars().take(160).collect::<String>();
                return Err(EnrichmentParseError::Json {
                    status,
                    message: primary_err.to_string(),
                    snippet,
                    source,
                });
            }
        }
    }

    match serde_json::from_str::<GetCustomerData>(cleaned) {
        Ok(data) => Ok(Some(data)),
        Err(err) => {
            let snippet = cleaned.chars().take(160).collect::<String>();
            Err(EnrichmentParseError::Json {
                status,
                message: err.to_string(),
                snippet,
                source,
            })
        }
    }
}

fn display_enrichment_result(result: &GetCustomerData) {
    println!("\n🔎 Enriched profile:");
    println!("  Name: {}", result.base.name);
    println!(
        "  CPF: {}",
        result.base.cpf.clone().unwrap_or_else(|| "-".to_string())
    );
    println!(
        "  Birth date: {}",
        result
            .base
            .birth_date
            .clone()
            .unwrap_or_else(|| "-".to_string())
    );
    if let Some(sex) = &result.base.sex {
        println!("  Sex: {}", sex);
    }
    if let Some(mother) = &result.base.mother_name {
        println!("  Mother: {}", mother);
    }
    if let Some(father) = &result.base.father_name {
        println!("  Father: {}", father);
    }
    if let Some(rg) = &result.base.rg {
        println!("  RG: {}", rg);
    }

    if !result.emails.is_empty() {
        println!("  Emails:");
        for email in &result.emails {
            println!(
                "    - {}{}",
                email.email,
                email
                    .ranking
                    .map(|r| format!(" (rank {})", r))
                    .unwrap_or_default()
            );
        }
    }

    if !result.phones.is_empty() {
        println!("  Phones:");
        for phone in &result.phones {
            let number = match (&phone.ddd, &phone.number) {
                (Some(ddd), Some(num)) => format!("({}) {}", ddd, num),
                (Some(ddd), None) => format!("({})", ddd),
                (None, Some(num)) => num.clone(),
                _ => "-".to_string(),
            };
            let extras = [
                phone.operator_.as_deref(),
                phone.kind.as_deref(),
                phone.ranking.map(|r| format!("rank {}", r)).as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
            if extras.is_empty() {
                println!("    - {}", number);
            } else {
                println!("    - {} [{}]", number, extras);
            }
        }
    }

    if !result.addresses.is_empty() {
        println!("  Addresses:");
        for address in &result.addresses {
            let parts = [
                address.street.as_deref(),
                address.number.as_deref(),
                address.neighborhood.as_deref(),
                address.city.as_deref(),
                address.uf.as_deref(),
                address.postal_code.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
            println!(
                "    - {}",
                if parts.is_empty() {
                    "-".to_string()
                } else {
                    parts
                }
            );
        }
    }
}

async fn enrich_diretrix_records(records: &[PropertyRecord]) -> Vec<Option<GetCustomerData>> {
    if records.is_empty() {
        return Vec::new();
    }

    // Check if using Workbuscas API or local enrichment service
    let use_workbuscas = std::env::var("WORKBUSCAS_TOKEN").is_ok();

    let (base_url, token) = if use_workbuscas {
        let token = std::env::var("WORKBUSCAS_TOKEN")
            .unwrap_or_else(|_| "FXEniLsawoXPlTdYTbdjZAxn".to_string());
        (
            "https://completa.workbuscas.com/api".to_string(),
            Some(token),
        )
    } else {
        // Fallback to local enrichment service
        let endpoint = std::env::var("ENRICHMENT_ENDPOINT")
            .unwrap_or_else(|_| "http://127.0.0.1:8080/enrich/person".to_string());
        (endpoint, None)
    };

    let client = match HttpClient::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(http) => http,
        Err(err) => {
            warn!("Skipping enrichment - failed to build HTTP client: {}", err);
            return vec![None; records.len()];
        }
    };

    if use_workbuscas {
        info!("✅ Using Workbuscas API for enrichment");
    } else {
        // Test if local enrichment service is available
        let test_payload = json!({
            "search_types": ["cpf"],
            "searches": ["00000000000"],
        });

        match client.post(&base_url).json(&test_payload).send().await {
            Ok(_) => {
                info!("✅ Enrichment service available at {}", base_url);
            }
            Err(err) => {
                info!(
                    "ℹ️  Enrichment service not available ({}), skipping enrichment",
                    err
                );
                info!("   To enable enrichment, either:");
                info!("   1. Set WORKBUSCAS_TOKEN environment variable");
                info!("   2. Or start local service: cargo run -- serve-enrichment --addr 127.0.0.1:8080");
                return vec![None; records.len()];
            }
        }
    }

    let mut results = Vec::with_capacity(records.len());
    let mut workbuscas_html_response_detected = false;

    for record in records {
        let cpf_candidate = sanitize_document_candidate(&record.document1)
            .or_else(|| sanitize_document_candidate(&record.document2));
        let name_candidate = if record.owner.trim().is_empty() {
            None
        } else {
            Some(record.owner.trim().to_string())
        };

        if cpf_candidate.is_none() && name_candidate.is_none() {
            results.push(None);
            continue;
        }

        // Try CPF first if available
        let mut enrichment_result = None;

        if let Some(cpf) = cpf_candidate.clone() {
            if use_workbuscas && workbuscas_html_response_detected {
                info!(
                    "Skipping Workbuscas CPF lookup for '{}' because the API returned HTML earlier in this run",
                    record.owner
                );
            } else {
                let url = if use_workbuscas {
                    // Workbuscas API format
                    format!(
                        "{}?token={}&modulo=cpf&consulta={}",
                        base_url,
                        token.as_ref().unwrap(),
                        cpf
                    )
                } else {
                    // Local enrichment service
                    base_url.clone()
                };

                let request = if use_workbuscas {
                    client.get(&url)
                } else {
                    let payload = json!({
                        "search_types": ["cpf"],
                        "searches": [cpf.clone()],
                    });
                    client.post(&url).json(&payload)
                };

                match request.send().await {
                    Ok(response) => {
                        let status = response.status();

                        if status == StatusCode::NOT_FOUND {
                            info!(
                                "No enrichment data found for owner '{}' with CPF {}",
                                record.owner, cpf
                            );
                        } else if status.is_success() {
                            match parse_enrichment_payload(response, use_workbuscas).await {
                                Ok(Some(result)) => {
                                    println!(
                                        "\n✅ Enrichment succeeded for '{}' using CPF {}",
                                        record.owner, cpf
                                    );
                                    display_enrichment_result(&result);
                                    enrichment_result = Some(result);
                                }
                                Ok(None) => {
                                    if use_workbuscas {
                                        info!(
                                            "Workbuscas returned an empty response for owner '{}' with CPF {}",
                                            record.owner, cpf
                                        );
                                    } else {
                                        info!(
                                            "Local enrichment service returned an empty response for owner '{}' with CPF {}",
                                            record.owner, cpf
                                        );
                                    }
                                }
                                Err(err @ EnrichmentParseError::Html { .. }) => {
                                    warn!(
                                        "Failed to parse enrichment response for '{}': {}",
                                        record.owner, err
                                    );
                                    if use_workbuscas {
                                        workbuscas_html_response_detected = true;
                                        warn!(
                                            "Disabling further Workbuscas requests for this run. \
                                             Please verify your WORKBUSCAS_TOKEN and Workbuscas API availability."
                                        );
                                    }
                                }
                                Err(err) => {
                                    warn!(
                                        "Failed to parse enrichment response for '{}': {}",
                                        record.owner, err
                                    );
                                }
                            }
                        } else {
                            warn!(
                                "Enrichment service error for '{}' with CPF {} (status {})",
                                record.owner, cpf, status
                            );
                        }
                    }
                    Err(err) => {
                        warn!(
                            "Failed to call enrichment service for '{}' with CPF {}: {}",
                            record.owner, cpf, err
                        );
                    }
                }
            }
        }

        // Fallback to name search if CPF enrichment failed
        if enrichment_result.is_none() {
            if let Some(name) = name_candidate.clone() {
                if use_workbuscas && workbuscas_html_response_detected {
                    info!(
                        "Skipping Workbuscas name lookup for '{}' because the API returned HTML earlier in this run",
                        record.owner
                    );
                } else {
                    info!("Trying enrichment by name for '{}'", name);

                    let url = if use_workbuscas {
                        // Workbuscas API format - URL encode the name
                        let encoded_name = urlencoding::encode(&name);
                        format!(
                            "{}?token={}&modulo=name&consulta={}",
                            base_url,
                            token.as_ref().unwrap(),
                            encoded_name
                        )
                    } else {
                        // Local enrichment service
                        base_url.clone()
                    };

                    let request = if use_workbuscas {
                        client.get(&url)
                    } else {
                        let payload = json!({
                            "search_types": ["name"],
                            "searches": [name.clone()],
                        });
                        client.post(&url).json(&payload)
                    };

                    match request.send().await {
                        Ok(response) => {
                            let status = response.status();

                            if status == StatusCode::NOT_FOUND {
                                info!(
                                    "No enrichment data found for owner '{}' by name search",
                                    record.owner
                                );
                            } else if status.is_success() {
                                match parse_enrichment_payload(response, use_workbuscas).await {
                                    Ok(Some(result)) => {
                                        println!(
                                            "\n✅ Enrichment succeeded for '{}' using name search",
                                            record.owner
                                        );
                                        display_enrichment_result(&result);
                                        enrichment_result = Some(result);
                                    }
                                    Ok(None) => {
                                        if use_workbuscas {
                                            info!(
                                                "Workbuscas returned an empty response for owner '{}' by name search",
                                                record.owner
                                            );
                                        } else {
                                            info!(
                                                "Local enrichment service returned an empty response for owner '{}' by name search",
                                                record.owner
                                            );
                                        }
                                    }
                                    Err(err @ EnrichmentParseError::Html { .. }) => {
                                        warn!(
                                            "Failed to parse enrichment response for '{}': {}",
                                            record.owner, err
                                        );
                                        if use_workbuscas {
                                            workbuscas_html_response_detected = true;
                                            warn!(
                                                "Disabling further Workbuscas requests for this run. \
                                                 Please verify your WORKBUSCAS_TOKEN and Workbuscas API availability."
                                            );
                                        }
                                    }
                                    Err(err) => {
                                        warn!(
                                            "Failed to parse enrichment response for '{}': {}",
                                            record.owner, err
                                        );
                                    }
                                }
                            } else {
                                warn!(
                                    "Enrichment service error for '{}' by name (status {})",
                                    record.owner, status
                                );
                            }
                        }
                        Err(err) => {
                            warn!(
                                "Failed to call enrichment service for '{}' by name: {}",
                                record.owner, err
                            );
                        }
                    }
                }
            }
        }

        results.push(enrichment_result);
    }

    results
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

    ServeEnrichment {
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,
    },

    Dbase {
        #[arg(long)]
        cep: Option<String>,

        #[arg(long, default_value_t = 0)]
        numero_inicio: u64,

        #[arg(long, default_value_t = 999999999999999)]
        numero_fim: u64,

        #[arg(long)]
        username: Option<String>,

        #[arg(long)]
        password: Option<String>,

        #[arg(long)]
        username2: Option<String>,

        #[arg(long)]
        password2: Option<String>,

        #[arg(long)]
        username3: Option<String>,

        #[arg(long)]
        password3: Option<String>,

        #[arg(long)]
        webdriver_url: Option<String>,

        #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
        headless: bool,

        #[arg(long)]
        output: Option<String>,
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

                let enrichment_results = enrich_diretrix_records(&records).await;

                let csv_filename = format!(
                    "diretrix_{}_{}.csv",
                    street_name.replace(" ", "_").to_lowercase(),
                    street_number_value
                );

                match export_diretrix_to_csv(&records, &enrichment_results, &csv_filename) {
                    Ok(_) => {
                        println!("\n✅ Results exported to: {}", csv_filename);
                    }
                    Err(e) => {
                        warn!("Failed to export CSV: {}", e);
                        println!("\n⚠️  Warning: Could not export CSV file: {}", e);
                    }
                }
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

        Commands::ServeEnrichment { addr } => {
            run_enrichment_server(&addr).await?;
        }

        Commands::Dbase {
            cep,
            numero_inicio,
            numero_fim,
            username,
            password,
            username2,
            password2,
            username3,
            password3,
            webdriver_url,
            headless,
            output,
        } => {
            info!("Starting DBase scraper for dbase.com.br");

            // Resolve credentials from CLI args or environment variables
            let cred1_user =
                resolve_credential(username, "DBASE_USERNAME", "DBase username (1): ")?;
            let cred1_pass =
                resolve_credential(password, "DBASE_PASSWORD", "DBase password (1): ")?;

            let cred2_user = username2
                .or_else(|| std::env::var("DBASE_USERNAME_2").ok())
                .unwrap_or_else(|| cred1_user.clone());
            let cred2_pass = password2
                .or_else(|| std::env::var("DBASE_PASSWORD_2").ok())
                .unwrap_or_else(|| cred1_pass.clone());

            let cred3_user = username3
                .or_else(|| std::env::var("DBASE_USERNAME_3").ok())
                .unwrap_or_else(|| cred1_user.clone());
            let cred3_pass = password3
                .or_else(|| std::env::var("DBASE_PASSWORD_3").ok())
                .unwrap_or_else(|| cred1_pass.clone());

            let credentials = vec![
                (cred1_user.clone(), cred1_pass.clone()),
                (cred2_user, cred2_pass),
                (cred3_user, cred3_pass),
                (cred1_user, cred1_pass), // Loop back to first
            ];

            let webdriver_url_val = if let Some(url) = webdriver_url.as_deref() {
                url
            } else if let Ok(url) = std::env::var("DBASE_WEBDRIVER_URL") {
                Box::leak(url.into_boxed_str()) as &str
            } else {
                "http://localhost:9515"
            };

            // Ensure ChromeDriver is running
            start_chromedriver()?;

            // Create scraper
            let scraper = DbaseScraper::new(credentials, webdriver_url_val, headless).await?;

            // Login
            scraper.login().await?;

            // Get CEP from CLI or prompt
            let cep_value = match cep {
                Some(value) if !value.trim().is_empty() => value.trim().to_string(),
                _ => prompt_non_empty("CEP (8 digits): ")?,
            };

            // Search by CEP
            info!("Searching for CEP: {}", cep_value);
            let records = scraper
                .search_by_cep(&cep_value, numero_inicio, numero_fim)
                .await?;

            info!("Total records found: {}", records.len());

            // Display records
            if !records.is_empty() {
                println!(
                    "\n{:<20} {:<35} {:<25} {:<8} {:<20} {:<18} {:<10}",
                    "CPF/CNPJ",
                    "Nome/Razão Social",
                    "Logradouro",
                    "Número",
                    "Complemento",
                    "Bairro",
                    "CEP"
                );
                println!("{}", "-".repeat(140));

                for (idx, record) in records.iter().enumerate().take(20) {
                    println!(
                        "{:<20} {:<35} {:<25} {:<8} {:<20} {:<18} {:<10}",
                        record.cpf_cnpj,
                        record
                            .nome_razao_social
                            .chars()
                            .take(35)
                            .collect::<String>(),
                        record.logradouro.chars().take(25).collect::<String>(),
                        record.numero,
                        record.complemento.chars().take(20).collect::<String>(),
                        record.bairro.chars().take(18).collect::<String>(),
                        record.cep
                    );

                    if idx == 19 && records.len() > 20 {
                        println!("... and {} more records", records.len() - 20);
                    }
                }
            }

            // Export to CSV
            let output_filename = output.unwrap_or_else(|| dbase_scraper::generate_csv_filename());

            // Create output directory if it doesn't exist
            if let Some(parent) = std::path::Path::new(&output_filename).parent() {
                std::fs::create_dir_all(parent)?;
            }

            dbase_scraper::export_to_csv(&records, &output_filename)?;

            // Close browser
            if let Err(e) = scraper.close().await {
                warn!("Failed to close browser cleanly: {}", e);
            }

            info!("✅ DBase scraping complete!");
        }
    }

    Ok(())
}
