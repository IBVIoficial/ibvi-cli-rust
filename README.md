# IPTU CLI

A robust command-line tool for extracting and enriching São Paulo property tax (IPTU) data with integrated web scraping, data enrichment services, intelligent failure recovery, and **fully automated DBase address scraping with reCAPTCHA solving**.

## Overview

IPTU CLI is a Rust-based automation tool that:
- Scrapes property data from São Paulo's municipal tax website
- **Scrapes address data from DBase with automatic reCAPTCHA v2 solving**
- **Supports pagination to extract complete datasets (250+ records)**
- Enriches property records with owner information via Diretrix API
- Integrates with Workbuscas API for automatic property enrichment
- Provides a microservice for real-time data enrichment
- Manages processing queues through Supabase
- Handles failures gracefully with smart cooldown mechanisms

## Key Features

### Core Functionality
- **Automated Job Processing**: Fetch and process property records from Supabase queues
- **Web Scraping**: Selenium-based scraping with ChromeDriver integration
- **DBase Scraper**: Fully automated address data extraction with reCAPTCHA solving
- **Batch Management**: Track processing progress across multiple batches
- **Concurrent Processing**: Configurable parallel scraper instances
- **Rate Limiting**: Built-in protection against API/website throttling

### DBase Scraper (NEW! 🚀)
- **Automatic reCAPTCHA v2 Solving**: Integrates with 2Captcha API for hands-free operation
- **Session Persistence**: Saves login cookies to avoid repeated CAPTCHA challenges
- **Multi-Credential Fallback**: Supports up to 3 accounts with automatic rotation
- **Full Pagination Support**: Extracts all pages automatically (tested with 251 records)
- **CEP-Based Search**: Search by Brazilian postal code (CEP) with optional number ranges
- **CSV Export**: Automatic export of scraped data with timestamps
- **Zero Manual Intervention**: Completely automated workflow from login to export

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

### DBase Address Scraper

Extract address data from DBase by CEP (Brazilian postal code):

**Basic Usage:**
```bash
# Search by CEP
cargo run -- dbase --cep 01455-040

# Search with number range
cargo run -- dbase --cep 01455-040 --numero-inicio 100 --numero-fim 500
```

**Advanced Options:**
```bash
# Specify custom credentials
cargo run -- dbase --cep 05676-120 \
  --username "your-username" \
  --password "your-password"

# Use custom WebDriver URL
cargo run -- dbase --cep 01455-040 \
  --webdriver-url "http://localhost:9515"

# Headless mode (default)
cargo run -- dbase --cep 01455-040 --headless true

# Specify output file
cargo run -- dbase --cep 01455-040 \
  --output "custom_output.csv"
```

**How It Works:**

1. **Automatic Login**: Uses credentials from `.env` or CLI arguments
2. **reCAPTCHA Solving**: 
   - First tries to use saved session (no CAPTCHA)
   - If session expired, automatically solves reCAPTCHA via 2Captcha API
   - Solution injected via JavaScript (2-60 second solve time)
3. **Search Execution**: Fills CEP search form and clicks "Pesquisar" automatically
4. **Pagination**: Detects and clicks through all result pages (» button)
5. **Data Extraction**: Extracts CPF/CNPJ, name, address, complement, neighborhood, and CEP
6. **CSV Export**: Saves to `output/dbase_scraped_YYYYMMDD_HHMMSS.csv`

**Output Format:**
```csv
cpf_cnpj,nome_razao_social,logradouro,numero,complemento,bairro,cep
61.486.650/0001-83,DIAGNOSTICOS DA AMERICA S/A,RUA SERIDó,0,,JARDIM EUROPA,01455040
521.656.718-68,VINICIUS LIMA FERNANDES,"R SERIDO, 00050, AP 101",0,,JD EUROPA,01455040
```

**Performance:**
- **Tested**: 251 records extracted across 13 pages
- **CAPTCHA Solve Time**: 2-60 seconds (avg. 30s)
- **Pagination Speed**: ~2 seconds per page
- **Total Time**: ~2-3 minutes for 251 records (including login)

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

#### DBase Scraper (Required for DBase scraping)
```env
# Primary credentials
DBASE_USERNAME=your-username
DBASE_PASSWORD=your-password

# Fallback credentials (optional but recommended)
DBASE_USERNAME_2=fallback-username
DBASE_PASSWORD_2=fallback-password
DBASE_USERNAME_3=fallback-username-3
DBASE_PASSWORD_3=fallback-password-3

# 2Captcha API key for automatic reCAPTCHA solving
TWOCAPTCHA_API_KEY=your-2captcha-api-key

# WebDriver URL (optional, defaults to localhost:9515)
DBASE_WEBDRIVER_URL=http://localhost:9515
```

**Get 2Captcha API Key:**
1. Sign up at https://2captcha.com
2. Add funds (~$3 for 1000 CAPTCHAs)
3. Copy your API key from dashboard
4. Cost: ~$0.003 per CAPTCHA solve

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
│   ├── dbase_scraper/             # DBase address scraper (NEW!)
│   │   ├── mod.rs                 # Main scraper logic
│   │   ├── captcha_solver.rs     # 2Captcha integration
│   │   └── session_manager.rs    # Session persistence
│   ├── diretrix_scraper/          # Diretrix property scraper
│   ├── diretrix_enrichment/       # Person data enrichment
│   ├── enrichment_service.rs      # REST API service
│   └── supabase/                  # Supabase client
├── output/                        # CSV export files (gitignored)
├── sessions/                      # Session cookies (gitignored)
├── logs/                          # Log files (gitignored)
├── docs/                          # Documentation (gitignored)
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

- **DBase Scraper Documentation** (in `docs/` folder):
  - `DBASE_USAGE.md` - Comprehensive usage guide
  - `RECAPTCHA_AUTOMATION.md` - reCAPTCHA solving strategies
  - `DBASE_IMPLEMENTATION_SUMMARY.md` - Technical implementation details
- **General Documentation**:
  - `ENRICHMENT_GUIDE.md` - Detailed enrichment service documentation
  - `CLAUDE.md` - Development commands and architecture guide

## Quick Start: DBase Scraper

**1. Set up environment:**
```bash
# Copy example config
cp .env.example .env

# Edit .env and add:
# - DBASE_USERNAME, DBASE_PASSWORD
# - TWOCAPTCHA_API_KEY (get from https://2captcha.com)
```

**2. Start ChromeDriver:**
```bash
./start.chromedriver.sh
```

**3. Run your first scrape:**
```bash
cargo run -- dbase --cep 01455-040
```

**4. Check results:**
```bash
ls -lh output/dbase_scraped_*.csv
cat output/dbase_scraped_*.csv | head
```

**Expected Output:**
- Login with reCAPTCHA solving: ~30-60 seconds
- Data extraction with pagination: ~1-2 minutes
- Total records: 20-250+ depending on CEP
- Output: `output/dbase_scraped_YYYYMMDD_HHMMSS.csv`

## Troubleshooting

### DBase Scraper Issues

**Problem: "Timeout waiting for search results"**
- **Cause**: CEP may not exist in database or wrong format
- **Solution**: Verify CEP format (XXXXX-XXX) and try different CEP

**Problem: "All login attempts failed"**
- **Cause**: Invalid credentials or session issues
- **Solution**: 
  1. Verify credentials in `.env`
  2. Delete `sessions/dbase_session.json`
  3. Try again with fresh session

**Problem: "2Captcha API error"**
- **Cause**: Invalid API key or insufficient balance
- **Solution**:
  1. Check API key is correct
  2. Verify balance at https://2captcha.com
  3. Top up if needed (~$3 for 1000 solves)

**Problem: "Only 20 records extracted (missing pagination)"**
- **Cause**: Pagination detection failed (fixed in latest version)
- **Solution**: Update to latest version with JavaScript pagination detection

### General Troubleshooting

**ChromeDriver not starting:**
```bash
# Kill existing instances
pkill chromedriver

# Restart
./start.chromedriver.sh
```

**Session expired issues:**
```bash
# Clear saved sessions
rm -rf sessions/

# Scraper will create fresh login
```
