use anyhow::Result;
use iptu_cli::diretrix_scraper::DiretrixScraper;
use tracing::Level;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    println!("=== Diretrix Consultoria Manual Scraper Test ===");
    println!();

    // Credentials
    let username = "100198".to_string();
    let password = "Mb082025".to_string();

    // Street to search
    let street_name = "Domingos Leme";
    let street_number = "440";

    println!("Connecting to ChromeDriver...");
    let scraper = DiretrixScraper::new(username, password, "http://localhost:9515", false).await?;
    println!("Connected!");
    println!();

    println!("========================================");
    println!("Step 1: Logging in automatically...");
    println!("========================================");
    scraper.login().await?;
    println!("Login completed!");
    println!();

    println!("========================================");
    println!("Step 2: Manual Search");
    println!("========================================");
    println!("The browser is now on the search page.");
    println!();
    println!("Please complete these steps:");
    println!("  1. Fill in: {}", street_name);
    println!("  2. Fill in: {}", street_number);
    println!("  3. Click the 'Buscar' button");
    println!("  4. Wait for results to appear in the table");
    println!();
    println!("You have 45 seconds...");
    println!("========================================");
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
    println!("Closing browser in 5 seconds...");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    scraper.close().await?;
    println!("Done!");

    Ok(())
}
