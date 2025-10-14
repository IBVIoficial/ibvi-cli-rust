use anyhow::Result;
use iptu_cli::diretrix_scraper::DiretrixScraper;
use std::io::{self, Write};
use tracing::Level;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    println!("=== Diretrix Consultoria Scraper ===");
    println!();

    // Credentials (you can modify these or read from environment variables)
    let username = "100198".to_string();
    let password = "Mb082025".to_string();

    // Create scraper instance (requires ChromeDriver running on localhost:9515)
    println!("Connecting to ChromeDriver...");
    let scraper = DiretrixScraper::new(username, password, "http://localhost:9515", false).await?;
    println!("Connected!");
    println!();

    // Login
    println!("========================================");
    println!("A CHROME BROWSER WILL OPEN NOW!");
    println!("Please login with:");
    println!("  Username: 100198");
    println!("  Password: Mb082025");
    println!("You have 20 seconds...");
    println!("========================================");
    println!();

    scraper.login().await?;
    println!("Login phase completed!");
    println!();

    loop {
        // Get street name from user
        print!("Enter street name (or 'quit' to exit): ");
        io::stdout().flush()?;
        let mut street_name = String::new();
        io::stdin().read_line(&mut street_name)?;
        let street_name = street_name.trim();

        if street_name.eq_ignore_ascii_case("quit") {
            break;
        }

        // Get street number from user
        print!("Enter street number: ");
        io::stdout().flush()?;
        let mut street_number = String::new();
        io::stdin().read_line(&mut street_number)?;
        let street_number = street_number.trim();

        println!();
        println!(
            "Searching for properties at: {} {}",
            street_name, street_number
        );
        println!();

        // Search for properties using manual mode
        match scraper
            .search_by_address_manual(street_name, street_number)
            .await
        {
            Ok(records) => {
                if records.is_empty() {
                    println!("No properties found.");
                } else {
                    println!("Found {} properties:", records.len());
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
                }
            }
            Err(e) => {
                eprintln!("Error searching properties: {}", e);
            }
        }

        println!("---");
        println!();
    }

    println!("Closing browser...");
    scraper.close().await?;
    println!("Goodbye!");
    Ok(())
}
