use anyhow::Result;
use iptu_cli::diretrix_scraper::DiretrixScraper;
use std::io::{self, Write};
use tracing::Level;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    println!("=== Diretrix Consultoria Interactive Scraper ===");
    println!();

    // Credentials
    let username = "100198".to_string();
    let password = "Mb082025".to_string();

    // Ask user for street name
    print!("Enter street name (e.g., Domingos Leme): ");
    io::stdout().flush()?;
    let mut street_name = String::new();
    io::stdin().read_line(&mut street_name)?;
    let street_name = street_name.trim();

    // Ask user for street number
    print!("Enter street number (e.g., 440): ");
    io::stdout().flush()?;
    let mut street_number = String::new();
    io::stdin().read_line(&mut street_number)?;
    let street_number = street_number.trim();

    println!();
    println!("========================================");
    println!("Searching for: {} {}", street_name, street_number);
    println!("========================================");
    println!();

    println!("Step 1: Connecting to ChromeDriver...");
    let scraper = DiretrixScraper::new(username, password, "http://localhost:9515", false).await?;
    println!("Connected!");
    println!();

    println!("Step 2: Logging in automatically...");
    scraper.login().await?;
    println!("Login successful!");
    println!();

    println!("Step 3: Performing search (manual mode)...");
    println!("A Chrome browser window has opened.");
    println!();
    println!("Please complete these steps in the browser:");
    println!("  1. Fill in street name: {}", street_name);
    println!("  2. Fill in street number: {}", street_number);
    println!("  3. Click the 'Buscar' button");
    println!("  4. Wait for results to load");
    println!();
    println!("You have 45 seconds...");
    println!();

    // Search with manual mode
    match scraper
        .search_by_address_manual(street_name, street_number)
        .await
    {
        Ok(records) => {
            if records.is_empty() {
                println!("No properties found.");
            } else {
                println!();
                println!("========================================");
                println!("SUCCESS! Found {} properties:", records.len());
                println!("========================================");
                println!();

                for (i, record) in records.iter().enumerate() {
                    println!("--- Property {} ---", i + 1);
                    println!("Owner:          {}", record.owner);
                    println!("IPTU:           {}", record.iptu);
                    println!("Street:         {}", record.street);
                    println!("Number:         {}", record.number);
                    println!("Complement:     {}", record.complement);
                    println!("Complement 2:   {}", record.complement2);
                    println!("Neighborhood:   {}", record.neighborhood);
                    if let Some(doc1) = &record.document1 {
                        println!("Document 1:     {}", doc1);
                    }
                    if let Some(doc2) = &record.document2 {
                        println!("Document 2:     {}", doc2);
                    }
                    println!();
                }

                println!("========================================");
                println!("Total: {} properties scraped successfully!", records.len());
                println!("========================================");
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
        }
    }

    println!();
    println!("Closing browser...");
    scraper.close().await?;
    println!("Done!");

    Ok(())
}
