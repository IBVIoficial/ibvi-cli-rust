# IPTU CLI

A robust command-line tool for extracting and enriching São Paulo property tax (IPTU) data with integrated web scraping, data enrichment services, and intelligent failure recovery.

## Overview

IPTU CLI is a Rust-based automation tool that:
- Scrapes property data from São Paulo's municipal tax website
- Enriches property records with owner information via Diretrix API
- Integrates with Workbuscas API for automatic property enrichment
- Provides a microservice for real-time data enrichment
- Manages processing queues through Supabase
- Handles failures gracefully with smart cooldown mechanisms

## Key Features

### Core Functionality
- **Automated Job Processing**: Fetch and process property records from Supabase queues
- **Web Scraping**: Selenium-based scraping with ChromeDriver integration
- **Batch Management**: Track processing progress across multiple batches
- **Concurrent Processing**: Configurable parallel scraper instances
- **Rate Limiting**: Built-in protection against API/website throttling

### Intelligent Failure Recovery
- Tracks failures within rolling 10-minute windows
- Automatically applies 20-minute cooldown after 2 failures
- Progress tracking with 2-minute status updates
- Auto-reset on successful operations
- Prevents IP bans while maintaining efficiency

### Data Enrichment Services
- **Diretrix Integration**: Search property owner information by CPF, name, email, or phone
- **Workbuscas API**: Automatic property enrichment during scraping
- **Enrichment Microservice**: REST API for real-time enrichment requests
- **Smart Matching**: Cosine similarity scoring for accurate owner identification

## Prerequisites

- **Rust**: Latest stable version (2021 edition)
- **ChromeDriver**: Running on port 9515 for web scraping
- **Supabase**: Account with configured tables (see [Database Schema](#database-schema))
- **Node.js** (optional): For the React enrichment UI component

## Installation

1. **Clone the repository**
   ```bash
   git clone <repository-url>
   cd iptu-cli
   ```

2. **Configure environment variables**
   ```bash
   cp .env.example .env
   # Edit .env with your credentials
   ```

3. **Build the project**
   ```bash
   cargo build --release
   ```

4. **Start ChromeDriver** (in a separate terminal)
   ```bash
   ./start.chromedriver.sh
   # Or manually: chromedriver --port=9515
   ```

## Usage

### Basic Commands

#### List Pending Jobs
View queued jobs without processing:
```bash
cargo run -- fetch --limit 10
```

#### Process Jobs
Fetch and process property records:
```bash
cargo run -- process --limit 10 --concurrent 2 --headless true --rate-limit 100
```

**Options:**
- `-l, --limit <LIMIT>`: Number of jobs to fetch (default: 10)
- `-c, --concurrent <CONCURRENT>`: Concurrent scraper instances (default: 1)
- `--headless <true|false>`: Run browser in headless mode (default: true)
- `-r, --rate-limit <RATE_LIMIT>`: Maximum requests per hour (default: 100)

#### Retrieve Results
Fetch processed results from Supabase:
```bash
cargo run -- results --limit 10 --offset 0
```

### Diretrix Property Scraper

Search and export property data from Diretrix:
```bash
cargo run -- diretrix --street "nome da rua" --street-number "123"
```

This command automatically enriches scraped properties using CPF and owner name data.

### Enrichment Microservice

Start the enrichment REST API service:

```bash
# Configure credentials in .env or export directly
export DIRETRIX_BASE_URL=https://www.diretrixconsultoria.com.br
export DIRETRIX_USER=your-user
export DIRETRIX_PASS=your-pass

# Launch the service
cargo run -- serve-enrichment --addr 127.0.0.1:8080
```

#### API Endpoint: `/enrich/person`

Enrich person data by CPF, name, email, or phone:

```bash
curl -X POST http://127.0.0.1:8080/enrich/person \
  -H 'Content-Type: application/json' \
  -d '{
        "search_types": ["cpf", "name", "email"],
        "searches": ["12345678901", "Maria Silva", "maria@example.com"]
      }'
```

**Fallback Strategy:**
1. Search by CPF (primary)
2. Fallback to email
3. Fallback to phone
4. Fallback to name

**Matching Logic:**
- Multiple candidates are ranked using cosine similarity
- Best match selected if similarity score > 0.5
- Returns `GetCustomerData` payload on success
- Returns `404` if no match found
- Returns `502` for API/Diretrix errors

### React UI Component

A lightweight testing interface is available at `frontend/EnrichmentScreen.tsx`:

```tsx
import EnrichmentScreen from './frontend/EnrichmentScreen';

// Use in your React app
<EnrichmentScreen />
```

Fill optional fields (CPF, Name, Email, Phone) and submit to test the enrichment endpoint with real-time results.

## Configuration

### Environment Variables

Copy `.env.example` to `.env` and configure:

#### Supabase (Required)
```env
SUPABASE_URL=https://your-project.supabase.co
SUPABASE_ANON_KEY=your-anon-key
SUPABASE_SERVICE_ROLE_KEY=your-service-role-key
```

#### Diretrix Scraper
```env
DIRETRIX_USERNAME=your-username
DIRETRIX_PASSWORD=your-password
DIRETRIX_WEBDRIVER_URL=http://localhost:9515
```

#### Workbuscas API (Property Enrichment)
```env
WORKBUSCAS_TOKEN=your-api-token
```

#### Local Enrichment Service (Optional)
```env
ENRICHMENT_ENDPOINT=http://127.0.0.1:8080/enrich/person
DIRETRIX_BASE_URL=https://www.diretrixconsultoria.com.br
DIRETRIX_USER=your-enrichment-user
DIRETRIX_PASS=your-enrichment-pass
```

### Logging

Control log verbosity with the `RUST_LOG` environment variable:

```bash
# Debug level (verbose)
RUST_LOG=debug cargo run -- process

# Info level (default)
RUST_LOG=info cargo run -- process

# Single module debug
RUST_LOG=iptu_cli::scraper=debug cargo run -- process
```

## Testing

Comprehensive test suite with unit and integration tests.

### Running Tests

```bash
# All tests
cargo test

# With output (debug mode)
cargo test -- --nocapture

# Specific module
cargo test scraper::tests

# Integration tests only
cargo test --test scraper_integration_test

# Single test
cargo test test_failure_tracker_new

# Sequential execution (debugging)
cargo test -- --test-threads=1
```

### Test Coverage

| Component | Focus Area | Test Count |
|-----------|-----------|------------|
| **FailureTracker** | Cooldown & failure management | Multiple |
| **ScraperEngine** | Core scraping functionality | Multiple |
| **Integration** | End-to-end scenarios | 9 tests |

**Key test areas:**
- Failure tracking and cooldown detection (2 failures in 10min)
- Automatic timestamp cleanup for old failures
- Concurrent access handling
- Contributor number format validation
- Batch processing workflows
- Error handling and recovery scenarios

**Test files:**
- `src/scraper/mod.rs` - Unit tests (15 tests)
- `tests/scraper_integration_test.rs` - Integration tests (9 tests)

## Database Schema

Required Supabase tables:

### `iptus_list`

Job queue containing contributor numbers to process.

### `iptus`

Processed property records and scraping results.

### `batches`

Batch tracking for monitoring processing progress across multiple runs.

## Architecture

### Failure Recovery System

Intelligent failure handling prevents IP bans while maintaining throughput:

1. **Failure Detection**: Monitors all scraping attempts in real-time
2. **Rolling Window**: Tracks failures within 10-minute sliding windows
3. **Automatic Cooldown**: Triggers 20-minute pause after 2 failures in 10min
4. **Progress Updates**: Displays cooldown status every 2 minutes
5. **Auto-Reset**: Successful operations reset all failure counters

**Benefits:**

- Prevents aggressive scraping
- Reduces risk of IP bans
- Maintains processing efficiency
- Self-healing on success

### Project Structure

```plaintext
iptu-cli/
├── src/
│   ├── main.rs                    # CLI entry point
│   ├── lib.rs                     # Library exports
│   ├── scraper/                   # IPTU scraper module
│   ├── diretrix_scraper/          # Diretrix property scraper
│   ├── diretrix_enrichment/       # Person data enrichment
│   ├── enrichment_service.rs      # REST API service
│   └── supabase/                  # Supabase client
├── tests/                         # Integration tests
├── frontend/                      # React UI components
└── Cargo.toml                     # Dependencies

```

## Contributing

Contributions are welcome! Please ensure:

- All tests pass: `cargo test`
- Code is formatted: `cargo fmt`
- No linter warnings: `cargo clippy`

## License

[Specify your license here]

## Related Documentation

- [ENRICHMENT_GUIDE.md](./ENRICHMENT_GUIDE.md) - Detailed enrichment service documentation
