# IPTU CLI

A command-line interface for extracting IPTU (São Paulo property tax) data from the municipal website.

## Features

- Fetch pending jobs from Supabase
- Process jobs with web scraping (using ChromeDriver)
- Upload results to Supabase
- Track batch progress
- Rate limiting and concurrent processing
- **Automatic failure recovery with cooldown system**
  - Tracks failures within 10-minute windows
  - Applies 20-minute cooldown after 2 failures
  - Automatically resets counters on success

## Prerequisites

- Rust (latest stable)
- ChromeDriver running on port 9515
- Supabase account with the required tables

## Installation

1. Clone the repository
2. Copy `.env.example` to `.env` and fill in your Supabase credentials
3. Build the project:

```bash
cargo build --release
```

## Usage

### Start ChromeDriver

Make sure ChromeDriver is running:

```bash
chromedriver --port=9515
```

### Fetch Pending Jobs

List pending jobs without processing:

```bash
cargo run -- fetch --limit 10
```

### Process Jobs

Fetch and process jobs:

```bash
cargo run -- process --limit 10 --concurrent 1 --headless --rate-limit 100
```

Options:
- `-l, --limit <LIMIT>`: Number of jobs to fetch (default: 10)
- `-c, --concurrent <CONCURRENT>`: Number of concurrent scrapers (default: 1)
- `--headless`: Run in headless mode (default: true)
- `-r, --rate-limit <RATE_LIMIT>`: Rate limit per hour (default: 100)

### Get Results

Fetch results from Supabase:

```bash
cargo run -- results --limit 10 --offset 0
```

### Diretrix Enrichment Service

The CLI now exposes a Rust microservice that enriches customer data using the
Diretrix API. Configure the required environment variables and launch the
service with the new `serve-enrichment` subcommand:

```bash
export DIRETRIX_BASE_URL=https://www.diretrixconsultoria.com.br/api
export DIRETRIX_USER=your-user
export DIRETRIX_PASS=your-pass

cargo run -- serve-enrichment --addr 127.0.0.1:8080
```

When running, send POST requests to `/enrich/person` with parallel arrays of
search types and values:

```bash
curl -X POST http://127.0.0.1:8080/enrich/person \
  -H 'Content-Type: application/json' \
  -d '{
        "search_types": ["cpf", "name", "email"],
        "searches": ["12345678901", "Maria Joaquina", "maria@example.com"]
      }'
```

The service tries the CPF first, then falls back to email, phone, and finally
name. When multiple candidates are returned the best match is selected via a
cosine similarity score (> 0.5). Successful responses are returned as a
`GetCustomerData` payload. Failures return 404 when no match is found or 502 for
Diretrix/API issues.

Whenever you run the `diretrix` scraping command, the CLI automatically feeds
each scraped property into the enrichment pipeline using the `Document 1`
field (when it resembles a CPF) and the owner name as a secondary search.

### React helper screen

For quick manual tests a lightweight React component is available at
`frontend/EnrichmentScreen.tsx`. Drop the component into your app, fill any of
the optional fields (CPF, Name, Email, Phone) and submit; it will call the
`/enrich/person` endpoint and render the normalised result or error messages.

## Environment Variables

Create a `.env` file with:

```
SUPABASE_URL=https://your-project.supabase.co
SUPABASE_ANON_KEY=your-anon-key
SUPABASE_SERVICE_ROLE_KEY=your-service-role-key
```

## Logging

Set the `RUST_LOG` environment variable to control logging level:

```bash
RUST_LOG=debug cargo run -- process
```

## Testing

The project includes comprehensive unit and integration tests.

### Running Tests

```bash
# Run all tests
cargo test

# Run with output for debugging
cargo test -- --nocapture

# Run specific test module
cargo test scraper::tests

# Run integration tests only
cargo test --test scraper_integration_test

# Run a specific test
cargo test test_failure_tracker_new

# Run tests with single thread (useful for debugging)
cargo test -- --test-threads=1
```

### Test Coverage

The test suite covers:

- **FailureTracker**: Cooldown system and failure management
  - Creation and initialization
  - Failure/success recording
  - Cooldown detection (2 failures in 10 minutes)
  - Timestamp cleanup for old failures
  - Concurrent access handling

- **ScraperEngine**: Core scraping functionality
  - Configuration management
  - Result creation and validation
  - Error handling

- **Integration Tests**: End-to-end scenarios
  - Contributor number format validation
  - Batch processing
  - Concurrent operations
  - Failure scenarios and recovery

### Test Files

- `src/scraper/mod.rs`: Unit tests within the module (15 tests)
- `tests/scraper_integration_test.rs`: Integration tests (9 tests)

## Database Schema

The CLI expects the following Supabase tables:

- `iptus_list`: Queue of contributor numbers to process
- `iptus`: Results table
- `batches`: Batch tracking table

## Failure Recovery System

The scraper includes an intelligent failure recovery mechanism:

1. **Failure Detection**: Monitors all scraping attempts
2. **10-Minute Window**: Tracks failures within rolling 10-minute windows
3. **Automatic Cooldown**: After 2 failures in 10 minutes, enters 20-minute cooldown
4. **Progress Tracking**: Shows cooldown progress every 2 minutes
5. **Auto-Reset**: Success automatically resets all failure counters

This prevents aggressive scraping that could lead to IP bans while maintaining efficient processing.

### Diretrix
        
```bash
cargo run -- diretrix --street "nome da rua sem o rua" --street-number "123"  
```