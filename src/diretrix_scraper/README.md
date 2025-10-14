# Diretrix Consultoria Scraper

A Rust service for scraping property data from the Diretrix Consultoria website (https://www.diretrixconsultoria.com.br/).

## Features

- Login with credentials
- Search properties by street name and number
- Parse and extract property information including:
  - Owner name (Proprietário)
  - IPTU number
  - Street address (Logradouro)
  - Street number
  - Complements
  - Neighborhood (Bairro)
  - Associated documents

## Usage

### As a Library

```rust
use iptu_cli::diretrix_scraper::DiretrixScraper;

#[tokio::main]
async fn main() -> Result<()> {
    // Create scraper with credentials
    let mut scraper = DiretrixScraper::new(
        "your_username".to_string(),
        "your_password".to_string(),
        "http://localhost:9515",
        true,
    ).await?;

    // Login
    scraper.login().await?;

    // Search for properties
    let results = scraper
        .search_by_address("Domingos Leme", "440")
        .await?;

    // Process results
    for record in results {
        println!("Owner: {}", record.owner);
        println!("IPTU: {}", record.iptu);
        println!("Address: {} {}", record.street, record.number);
        println!();
    }

    Ok(())
}
```

### Running the Example

You can test the scraper using the included example program:

```bash
cargo run --example diretrix_test
```

The example will:
1. Login with the provided credentials
2. Prompt you for a street name
3. Prompt you for a street number
4. Display all matching properties

Example session:
```
Enter street name (or 'quit' to exit): Domingos Leme
Enter street number: 440

Searching for properties at: Domingos Leme 440

Found 14 properties:

--- Property 1 ---
Owner:          EUGENIO ERMIRIO DE MORAES
IPTU:           0361290040-4
Street:         R  DOMINGOS LEME
Number:         440
Complement:     AP 11 E 5 VG
Complement 2:   ED. NUMBER ONE
Neighborhood:   VILA NOVA CONCEICAO
...
```

## Configuration

The scraper uses the following credentials by default:
- Username: `100198`
- Password: `Mb082025`

You can modify these in the example or pass different credentials when creating the `DiretrixScraper` instance.

## Dependencies

- `reqwest` - HTTP client with cookie support
- `scraper` - HTML parsing
- `serde` - Serialization/deserialization
- `tokio` - Async runtime
- `anyhow` - Error handling
- `tracing` - Logging

## Implementation Details

### Login Flow

1. Sends POST request to `/login` endpoint with credentials
2. Maintains session cookies automatically via `reqwest` cookie store

### Search Flow

1. Loads the search page (`/consultas/iptrix/endereco`) to establish session
2. Submits AJAX request to `/consultas/iptrix/endereco/buscar` with:
   - `txtProcurar`: Street name (without prefix like "Rua", "Av", etc.)
   - `txtNumero`: Street number
3. Parses the returned HTML table using CSS selectors

### HTML Parsing

The scraper targets the following HTML structure:

```html
<div id="tabelas">
    <table class="table table-filter table-hover">
        <tbody id="Relatorio">
            <tr>
                <td>Owner</td>
                <td>IPTU Number</td>
                <td>Street</td>
                <td>Number</td>
                <td>Complement</td>
                <td>Complement 2</td>
                <td>Neighborhood</td>
                <td>
                    <button class="enderecoDet"
                            data-documento="..."
                            data-documento-2="...">
                        Buscar
                    </button>
                </td>
            </tr>
        </tbody>
    </table>
</div>
```

## Notes

- The service is designed specifically for São Paulo city properties (as per website notice)
- When searching, use only the street name without prefixes (e.g., "Domingos Leme" not "Rua Domingos Leme")
- The scraper automatically handles sessions and cookies
- All operations are asynchronous using `tokio`

## Error Handling

The scraper returns `Result<T>` types with `anyhow::Error` for comprehensive error handling:

- Network errors (connection issues)
- Authentication failures
- Parsing errors (if HTML structure changes)
- Invalid responses

## Testing

Run the unit tests (requires valid credentials):

```bash
cargo test --lib diretrix_scraper -- --ignored
```

Note: Tests are marked as `#[ignore]` by default since they require valid credentials and network access.
