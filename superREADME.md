# IPTU CLI

A high-performance, production-ready command-line interface for extracting IPTU (Imposto Predial e Territorial Urbano - São Paulo's property tax) data from the municipal website through intelligent web scraping.

## Overview

### What It Does

This CLI tool automates the extraction of property tax information from São Paulo's municipal tax website (`prefeitura.sp.gov.br`). It fetches property owner data, addresses, tax registration numbers, and related information by scraping the city's IPTU lookup system. The tool is designed for bulk processing, handling thousands of contributor numbers efficiently while respecting rate limits and avoiding detection.

### Why It Exists

**Problem**: São Paulo's IPTU website doesn't provide a public API, making bulk data extraction for legitimate purposes (real estate analysis, property research, tax consulting) extremely time-consuming when done manually.

**Solution**: This CLI provides:
- **Automation**: Processes hundreds of contributor numbers without manual intervention
- **Reliability**: Built-in retry mechanisms, error handling, and progress tracking
- **Scalability**: Concurrent processing with configurable worker pools
- **Stealth**: Human-like behavior patterns to avoid bot detection
- **Integration**: Direct integration with Supabase for job queuing and result storage

### Architecture

The system follows a **distributed job processing** architecture:

```
┌─────────────┐      ┌──────────────┐      ┌─────────────────┐
│  Supabase   │ ───▶ │   IPTU CLI   │ ───▶ │  São Paulo IPTU │
│ Job Queue   │      │   (Rust)     │      │     Website     │
└─────────────┘      └──────────────┘      └─────────────────┘
      ▲                      │                       │
      │                      ▼                       │
      │              ┌──────────────┐                │
      └──────────────│ ChromeDriver │◀───────────────┘
                     │  (Selenium)  │
                     └──────────────┘
```

1. **Job Source**: Pending contributor numbers are stored in Supabase tables (`iptus_list` or `iptus_list_priority`)
2. **Processing**: The CLI claims jobs, spawns WebDriver instances, and scrapes data concurrently
3. **Storage**: Results are immediately uploaded back to Supabase with success/error status
4. **Tracking**: Batch metadata tracks progress, throughput, and success rates

## Core Features

### 🚀 Performance & Concurrency
- **Parallel Processing**: Run multiple ChromeDriver instances concurrently
- **Connection Pooling**: Reusable WebDriver pool eliminates startup overhead
- **Async/Await**: Tokio-based async runtime for maximum throughput
- **Rate Limiting**: Configurable requests-per-hour to respect server limits

### 🛡️ Anti-Detection & Reliability
- **Human-like Behavior**: Random delays, scrolling, mouse movements
- **User-Agent Rotation**: Different browser signatures per worker
- **Automation Masking**: JavaScript injection to hide WebDriver indicators
- **Smart Retries**: Exponential backoff with configurable retry attempts
- **Cookie Handling**: Automatic consent modal detection and dismissal

### 📊 Monitoring & Observability
- **Structured Logging**: Tracing-based logs with multiple severity levels
- **Real-time Progress**: Live updates on job completion status
- **Performance Reports**: Detailed throughput and success rate statistics
- **Batch Tracking**: Per-batch metrics stored in database
- **Debug Artifacts**: Saves HTML snapshots for failed scrapes

### 🔌 Flexible Input Sources
- **Database Queue**: Fetch from Supabase tables (with priority support)
- **File Input**: Process contributor numbers from text files
- **Direct Input**: Pass comma-separated numbers via CLI arguments

## Prerequisites

### System Requirements

- **Rust**: Version 1.70+ (latest stable recommended)
  - Install via [rustup](https://rustup.rs/): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
  
- **ChromeDriver**: Version matching your Chrome/Chromium installation
  - Download from [chromedriver.chromium.org](https://chromedriver.chromium.org/)
  - Must be running on port 9515
  - The CLI expects ChromeDriver to be accessible at `http://localhost:9515`

- **Supabase**: PostgreSQL-based backend
  - Create a project at [supabase.com](https://supabase.com)
  - Obtain API keys (anon key and service role key)
  - Set up the required database tables (see Database Schema section)

### Dependencies

The project relies on several key Rust crates:

- **tokio**: Async runtime for concurrent operations
- **thirtyfour**: WebDriver protocol implementation for browser automation
- **reqwest**: HTTP client for Supabase API calls
- **serde/serde_json**: JSON serialization/deserialization
- **clap**: Command-line argument parsing
- **tracing**: Structured logging and diagnostics
- **chrono**: Date/time handling
- **anyhow**: Error handling and propagation

## Installation

### 1. Clone the Repository

```bash
git clone <repository-url>
cd iptu-cli
```

### 2. Configure Environment Variables

Copy the example environment file and fill in your Supabase credentials:

```bash
cp .env.example .env
```

Edit `.env` with your credentials:

```env
SUPABASE_URL=https://your-project.supabase.co
SUPABASE_ANON_KEY=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
SUPABASE_SERVICE_ROLE_KEY=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
```

**Note**: The service role key is optional but recommended for bypassing RLS (Row Level Security) policies.

### 3. Build the Project

For development:

```bash
cargo build
```

For production (optimized):

```bash
cargo build --release
```

The compiled binary will be in `target/release/iptu-cli`

### 4. Set Up ChromeDriver

The CLI includes a helper script to start ChromeDriver:

```bash
chmod +x start.chromedriver.sh
./start.chromedriver.sh
```

Or start manually:

```bash
chromedriver --port=9515 --verbose --log-path=chromedriver.log
```

## Usage

The CLI provides three main commands: `fetch`, `process`, and `results`.

### Command: `fetch`

**Purpose**: Query pending jobs from Supabase without processing them. Useful for checking queue status.

```bash
cargo run -- fetch --limit 10
```

**Options**:
- `-l, --limit <NUMBER>`: Maximum number of pending jobs to fetch (default: 10)

**Example Output**:
```
Found 10 pending jobs:
  - 123.456.7890-1
  - 234.567.8901-2
  - 345.678.9012-3
```

---

### Command: `process`

**Purpose**: The main command that fetches jobs and processes them through web scraping.

#### Basic Usage

Process jobs from Supabase queue:

```bash
cargo run -- process --limit 10
```

#### Processing from a File

Process contributor numbers from a text file (one per line):

```bash
cargo run -- process --file contributor_numbers.txt
```

#### Processing Direct Numbers

Process specific numbers without database:

```bash
cargo run -- process --numbers "123.456.7890-1,234.567.8901-2"
```

#### Advanced Usage

Run with maximum concurrency and custom rate limiting:

```bash
cargo run --release -- process \
  --limit 100 \
  --concurrent 5 \
  --headless true \
  --rate-limit 200
```

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--limit` | `-l` | number | 10 | Number of jobs to fetch from database |
| `--concurrent` | `-c` | number | 1 | Number of parallel browser instances |
| `--headless` | - | bool | true | Run browsers in headless mode |
| `--rate-limit` | `-r` | number | 100 | Maximum requests per hour |
| `--file` | `-f` | string | - | Path to file with contributor numbers |
| `--numbers` | - | string | - | Comma-separated contributor numbers |

#### How It Works

1. **Job Acquisition**:
   - Checks `iptus_list_priority` table first for high-priority jobs
   - Falls back to `iptus_list` if no priority jobs exist
   - Claims jobs by setting status to `'p'` (processing)

2. **ChromeDriver Initialization**:
   - Executes `start.chromedriver.sh` to ensure ChromeDriver is running
   - Creates a pool of WebDriver instances (one per `--concurrent` value)
   - Each driver has unique User-Agent and anti-detection measures

3. **Concurrent Processing**:
   - Jobs are chunked based on `--concurrent` setting
   - Each chunk processes in parallel using Tokio tasks
   - Staggered delays between workers to appear more human-like

4. **Scraping Flow** (per job):
   ```
   Navigate to IPTU website
   ↓
   Wait with random delay (2-4s)
   ↓
   Handle cookie consent modal
   ↓
   Fill form with contributor number
   ↓
   Submit form
   ↓
   Wait for results (12s)
   ↓
   Extract property data
   ↓
   Upload to Supabase
   ↓
   Update job status (success/error)
   ```

5. **Result Storage**:
   - Each successful scrape is immediately uploaded to `iptus` table
   - Job status updated in `iptus_list`/`iptus_list_priority`
   - Batch progress tracked in `batches` table

6. **Completion**:
   - Displays performance report with metrics
   - Shuts down WebDriver pool gracefully

#### Performance Report

After processing, you'll see a detailed report:

```
╔══════════════════════════════════════════════════════════╗
║              PERFORMANCE REPORT                          ║
╠══════════════════════════════════════════════════════════╣
║  Total Jobs:                                         100 ║
║  Successful:                                          95 ║
║  Failed:                                               5 ║
║  Duration:                                      15m 30s ║
║  Throughput:                                    6.45/min ║
║  Success Rate:                                     95.0% ║
║  Status:                              🟢 EXCELLENT      ║
╚══════════════════════════════════════════════════════════╝
```

**Status Thresholds**:
- 🟢 **EXCELLENT**: ≥90% success rate + ≥5 jobs/min
- 🟡 **GOOD**: ≥75% success rate + ≥3 jobs/min
- 🟠 **MODERATE**: ≥50% success rate
- 🔴 **NEEDS IMPROVEMENT**: <50% success rate

---

### Command: `results`

**Purpose**: Retrieve processed results from Supabase.

```bash
cargo run -- results --limit 10 --offset 0
```

**Options**:
- `-l, --limit <NUMBER>`: Number of results to fetch (default: 10)
- `-o, --offset <NUMBER>`: Offset for pagination (default: 0)

**Example Output**:
```
Found 10 results:
  - 123.456.7890-1 | Success: true | Owner: Some("JOAO DA SILVA")
  - 234.567.8901-2 | Success: true | Owner: Some("MARIA SANTOS")
  - 345.678.9012-3 | Success: false | Owner: None
```

## Environment Variables

The application uses environment variables for configuration, loaded via the `dotenv` crate.

### Required Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `SUPABASE_URL` | Your Supabase project URL | `https://abcdefgh.supabase.co` |
| `SUPABASE_ANON_KEY` | Supabase anonymous/public API key | `eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...` |

### Optional Variables

| Variable | Description | Default | Notes |
|----------|-------------|---------|-------|
| `SUPABASE_SERVICE_ROLE_KEY` | Supabase service role key (bypasses RLS) | None | **Recommended** for production use |
| `RUST_LOG` | Logging verbosity level | `info` | Options: `trace`, `debug`, `info`, `warn`, `error` |



# Logging
RUST_LOG=info
```

### Why Service Role Key?

The **service role key** is recommended because:
- Bypasses Row Level Security (RLS) policies
- Ensures consistent access to all tables
- Prevents permission-related errors during bulk operations
- Required for updating job statuses in system tables

**Security Note**: Never commit `.env` to version control. The service role key has admin privileges.

## Logging

The CLI uses **tracing** for structured, high-performance logging.

### Log Levels

Set via the `RUST_LOG` environment variable:

```bash
# Minimal output (errors only)
RUST_LOG=error cargo run -- process

# Standard output (recommended)
RUST_LOG=info cargo run -- process

# Verbose debugging
RUST_LOG=debug cargo run -- process

# Maximum verbosity (includes library logs)
RUST_LOG=trace cargo run -- process
```

### Filtering by Module

Target specific modules for debugging:

```bash
# Only scraper module
RUST_LOG=iptu_cli::scraper=debug cargo run -- process

# Scraper + Supabase client
RUST_LOG=iptu_cli::scraper=debug,iptu_cli::supabase=debug cargo run -- process

# All app logs at debug, dependencies at warn
RUST_LOG=iptu_cli=debug,warn cargo run -- process
```

### Log Output Examples

**Info Level** (typical usage):
```
[INFO] Fetching 10 pending jobs from Supabase...
[INFO] Found 10 pending jobs
[INFO] Created batch: 550e8400-e29b-41d4-a716-446655440000
[INFO] Initializing scraper with 5 concurrent workers...
[INFO] Progress: 1/10
[INFO] Successfully uploaded result for 123.456.7890-1
```

**Debug Level** (troubleshooting):
```
[DEBUG] Response from iptus_list: [{"contributor_number":"123.456.7890-1","status":null}]
[DEBUG] Found txtNumIPTU: Some("123.456.7890-1")
[DEBUG] Found txtProprietarioNome: Some("JOAO DA SILVA")
[DEBUG] Found txtEndereco: Some("RUA DAS FLORES")
```

### Log Persistence

ChromeDriver logs are automatically saved to `chromedriver.log` in the project directory. Useful for debugging browser automation issues.

## Database Schema

The application requires three main tables in your Supabase PostgreSQL database.

### Table: `iptus_list`

**Purpose**: Job queue for contributor numbers awaiting processing.

```sql
CREATE TABLE iptus_list (
    contributor_number TEXT PRIMARY KEY,
    status TEXT,                    -- NULL (pending), 'p' (processing), 's' (success), 'e' (error)
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

-- Index for efficient queries
CREATE INDEX idx_iptus_list_status ON iptus_list(status);
```

**Status Values**:
- `NULL`: Pending, ready to be claimed
- `'p'`: Currently being processed (claimed by a worker)
- `'s'`: Successfully processed
- `'e'`: Processing failed

### Table: `iptus_list_priority` (Optional)

**Purpose**: High-priority job queue. The CLI checks this table first.

```sql
CREATE TABLE iptus_list_priority (
    contributor_number TEXT PRIMARY KEY,
    status TEXT,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_iptus_list_priority_status ON iptus_list_priority(status);
```

Same structure as `iptus_list`, but processed with higher priority.

### Table: `iptus`

**Purpose**: Stores scraped IPTU property data.

```sql
CREATE TABLE iptus (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contributor_number TEXT NOT NULL,
    numero_cadastro TEXT,           -- IPTU registration number
    nome_proprietario TEXT,         -- Property owner name
    nome_compromissario TEXT,       -- Buyer/compromiser name (if applicable)
    endereco TEXT,                  -- Street address
    numero TEXT,                    -- Street number
    complemento TEXT,               -- Address complement (apt, suite, etc.)
    bairro TEXT,                    -- Neighborhood
    cep TEXT,                       -- Postal code (CEP)
    sucesso BOOLEAN NOT NULL,       -- Success flag
    erro TEXT,                      -- Error message (if failed)
    batch_id UUID,                  -- Associated batch ID
    timestamp TIMESTAMP NOT NULL,   -- Processing timestamp
    processed_by TEXT,              -- Machine/worker identifier
    FOREIGN KEY (batch_id) REFERENCES batches(id)
);

-- Indexes for common queries
CREATE INDEX idx_iptus_contributor ON iptus(contributor_number);
CREATE INDEX idx_iptus_batch ON iptus(batch_id);
CREATE INDEX idx_iptus_timestamp ON iptus(timestamp DESC);
CREATE INDEX idx_iptus_success ON iptus(sucesso);
```

**Example Row**:
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "contributor_number": "123.456.7890-1",
  "numero_cadastro": "123.456.7890-1",
  "nome_proprietario": "JOAO DA SILVA",
  "nome_compromissario": null,
  "endereco": "RUA DAS FLORES",
  "numero": "123",
  "complemento": "APT 45",
  "bairro": "JARDIM PAULISTA",
  "cep": "01234-567",
  "sucesso": true,
  "erro": null,
  "batch_id": "660f9511-f39c-52e5-b827-557766551111",
  "timestamp": "2025-10-07T03:45:00Z",
  "processed_by": "cli"
}
```

### Table: `batches`

**Purpose**: Tracks batch processing statistics and progress.

```sql
CREATE TABLE batches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    total INTEGER NOT NULL,         -- Total jobs in batch
    processados INTEGER DEFAULT 0,  -- Number of jobs processed
    sucesso INTEGER DEFAULT 0,      -- Number of successful jobs
    erros INTEGER DEFAULT 0,        -- Number of failed jobs
    status TEXT DEFAULT 'processing', -- 'processing' or 'completed'
    created_at TIMESTAMP DEFAULT NOW(),
    completed_at TIMESTAMP
);

CREATE INDEX idx_batches_status ON batches(status);
CREATE INDEX idx_batches_created ON batches(created_at DESC);
```

**Example Row**:
```json
{
  "id": "660f9511-f39c-52e5-b827-557766551111",
  "total": 100,
  "processados": 100,
  "sucesso": 95,
  "erros": 5,
  "status": "completed",
  "created_at": "2025-10-07T03:00:00Z",
  "completed_at": "2025-10-07T03:45:00Z"
}
```

### Setup SQL Script

Run this complete script in your Supabase SQL Editor:

```sql
-- Create tables
CREATE TABLE IF NOT EXISTS iptus_list (
    contributor_number TEXT PRIMARY KEY,
    status TEXT,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS iptus_list_priority (
    contributor_number TEXT PRIMARY KEY,
    status TEXT,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS batches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    total INTEGER NOT NULL,
    processados INTEGER DEFAULT 0,
    sucesso INTEGER DEFAULT 0,
    erros INTEGER DEFAULT 0,
    status TEXT DEFAULT 'processing',
    created_at TIMESTAMP DEFAULT NOW(),
    completed_at TIMESTAMP
);

CREATE TABLE IF NOT EXISTS iptus (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contributor_number TEXT NOT NULL,
    numero_cadastro TEXT,
    nome_proprietario TEXT,
    nome_compromissario TEXT,
    endereco TEXT,
    numero TEXT,
    complemento TEXT,
    bairro TEXT,
    cep TEXT,
    sucesso BOOLEAN NOT NULL,
    erro TEXT,
    batch_id UUID,
    timestamp TIMESTAMP NOT NULL,
    processed_by TEXT,
    FOREIGN KEY (batch_id) REFERENCES batches(id)
);

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_iptus_list_status ON iptus_list(status);
CREATE INDEX IF NOT EXISTS idx_iptus_list_priority_status ON iptus_list_priority(status);
CREATE INDEX IF NOT EXISTS idx_iptus_contributor ON iptus(contributor_number);
CREATE INDEX IF NOT EXISTS idx_iptus_batch ON iptus(batch_id);
CREATE INDEX IF NOT EXISTS idx_iptus_timestamp ON iptus(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_iptus_success ON iptus(sucesso);
CREATE INDEX IF NOT EXISTS idx_batches_status ON batches(status);
CREATE INDEX IF NOT EXISTS idx_batches_created ON batches(created_at DESC);
```

## Technical Deep Dive: How Rust Powers This CLI

### Why Rust?

This project is written in **Rust** for several compelling reasons:

#### 1. **Performance**

- **Zero-cost abstractions**: High-level code compiles to performant machine code
- **No garbage collector**: Predictable memory usage and latency
- **Efficient concurrency**: Lightweight async tasks via Tokio runtime
- **Result**: Processes 5-10 jobs/minute with minimal CPU and memory overhead

#### 2. **Memory Safety**

- **Compile-time guarantees**: No null pointer dereferences, buffer overflows, or use-after-free bugs
- **Ownership system**: Prevents data races in concurrent code
- **Result**: Rock-solid stability even with multiple concurrent WebDriver instances

#### 3. **Concurrency Without Fear**

- **Async/await**: Write sequential-looking code that runs concurrently
- **Send + Sync traits**: Compiler-enforced thread safety
- **Result**: Safely run 5+ concurrent scrapers without race conditions

#### 4. **Error Handling**

- **Result<T, E> type**: Explicit error handling, no uncaught exceptions
- **? operator**: Clean error propagation
- **anyhow crate**: Rich context for debugging failures

#### 5. **Type System**

- **Strong static typing**: Catch bugs at compile time
- **Option<T>**: No null/undefined confusion
- **Pattern matching**: Exhaustive handling of all cases

### Architecture Overview

The codebase is organized into three main modules:

```
src/
├── main.rs           # CLI entry point, command handling, orchestration
├── scraper/          # Web scraping engine with anti-detection
│   └── mod.rs
└── supabase/         # Supabase API client
    └── mod.rs
```

### Module Breakdown

#### `main.rs` - Orchestration Layer

**Key Responsibilities**:
- CLI argument parsing with `clap`
- Environment configuration with `dotenv`
- Batch lifecycle management
- Performance reporting

**How Rust Helps**:

```rust
// Type-safe command definition with clap's derive macro
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// Enum ensures exhaustive handling of all commands
#[derive(Subcommand)]
enum Commands {
    Process { /* ... */ },
    Fetch { /* ... */ },
    Results { /* ... */ },
}
```

**Async Runtime**:

```rust
#[tokio::main]  // Macro transforms main into async runtime
async fn main() -> Result<()> {
    // Tokio provides:
    // - Thread pool for async tasks
    // - Event loop for I/O operations
    // - Task spawning and scheduling
}
```

**Arc for Safe Sharing**:

```rust
// Arc = Atomic Reference Counted pointer
// Allows multiple async tasks to share SupabaseClient safely
let client_arc = Arc::new(client);
let client_for_callback = client_arc.clone();

tokio::spawn(async move {
    // This closure "moves" ownership of the cloned Arc
    client_for_callback.upload_results(...).await?;
});
```

#### `scraper/mod.rs` - Web Automation Engine

**Key Technologies**:
- **thirtyfour**: Rust WebDriver client (like Selenium)
- **ChromeDriver**: Browser automation protocol
- **Tokio**: Async task execution

**ScraperEngine Architecture**:

```rust
pub struct ScraperEngine {
    config: ScraperConfig,
    driver_pool: Vec<WebDriver>,  // Pool of reusable browser instances
}
```

**Why Connection Pooling?**
- Creating a new WebDriver instance takes 2-3 seconds
- Reusing instances eliminates startup overhead
- Enables true concurrent processing

**Concurrency Model**:

```rust
// Process jobs in chunks matching concurrent limit
for chunk in jobs.chunks(config.max_concurrent) {
    let mut tasks = Vec::new();
    
    // Launch all jobs in chunk concurrently
    for (i, contributor_number) in chunk.iter().enumerate() {
        let driver = driver_pool[i].clone();  // Each task gets its own driver
        let task = async move {
            // This runs in parallel with other tasks
            scrape_iptu_static(&driver, &contributor_number).await
        };
        tasks.push(task);
    }
    
    // Wait for all concurrent tasks to complete
    let results = join_all(tasks).await;
}
```

**How Rust Prevents Data Races**:

```rust
// WebDriver implements Clone, creating new references
// Each async task "moves" its driver, preventing shared mutable access
let driver = driver_pool[i].clone();
let task = async move {  // "move" transfers ownership
    // Only this task can access this driver instance
};
```

**Anti-Detection Techniques**:

1. **User-Agent Rotation**:

```rust
let user_agents = vec![/* ... */];
let user_agent = &user_agents[i % user_agents.len()];
caps.add_chrome_arg(&format!("--user-agent={}", user_agent))?;
```

2. **WebDriver Masking**:

```rust
driver.execute(r#"
    Object.defineProperty(navigator, 'webdriver', {
        get: () => undefined  // Hide automation indicator
    });
"#, vec![]).await;
```

3. **Human-like Delays**:

```rust
// Random delay patterns with jitter
enum DelayPattern {
    Quick,   // 2-4 seconds
    Normal,  // 4-8 seconds
    Slow,    // 8-18 seconds
}

impl DelayPattern {
    async fn wait(&self) {
        let mut rng = rand::thread_rng();
        let delay_ms = match self {
            Self::Quick => rng.gen_range(2000..4000),
            Self::Normal => rng.gen_range(4000..8000),
            Self::Slow => rng.gen_range(8000..18000),
        };
        
        // Add jitter: ±20%
        let jitter = rng.gen_range(-20..=20) as f64 / 100.0;
        let final_delay = (delay_ms as f64 * (1.0 + jitter)) as u64;
        sleep(Duration::from_millis(final_delay)).await;
    }
}
```

**Error Handling**:

```rust
// Result type makes errors explicit
async fn scrape_iptu_static(driver: &WebDriver, number: &str) 
    -> Result<IPTUData> {
    driver.goto("...").await?;  // ? propagates errors upward
    
    // Check if page loaded correctly
    if !driver.find(By::Name("txtNumIPTU")).await.is_ok() {
        anyhow::bail!("Page did not load correctly");
    }
    
    Ok(data)  // Explicit success return
}
```

#### `supabase/mod.rs` - API Client

**Key Technologies**:
- **reqwest**: Async HTTP client
- **serde**: JSON serialization/deserialization

**Type-Safe API Responses**:

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct IPTUResult {
    pub contributor_number: String,
    pub nome_proprietario: Option<String>,  // Option = nullable
    pub sucesso: bool,
    // ... other fields
}

// Serde automatically converts JSON ↔ Rust structs
let results: Vec<IPTUResult> = response.json().await?;
```

**Why This Works**:
- Compile-time type checking ensures fields match
- No runtime errors from typos or missing fields
- `Option<T>` explicitly marks nullable fields

**Query Builder Pattern**:

```rust
let response = client
    .get(&url)
    .header("apikey", auth_key)
    .query(&[("select", "*"), ("status", "is.null")])
    .send()
    .await?;
```

### Key Rust Features in Action

#### 1. Ownership and Borrowing

**The Problem**: Multiple async tasks need access to shared data

**The Solution**: Arc (Atomic Reference Counting)

```rust
// SupabaseClient is wrapped in Arc for shared ownership
let client_arc = Arc::new(client);

// Clone creates a new reference (increments count)
let client_for_callback = client_arc.clone();

// Spawn async task that moves the Arc clone
tokio::spawn(async move {
    // Reference count: 2
    client_for_callback.upload_results(...).await;
    // Task ends, reference count decrements
});
// Reference count: 1 (original Arc still exists)
```

#### 2. Pattern Matching

```rust
match cli.command {
    Commands::Process { limit, concurrent, .. } => {
        // Handle process command
    }
    Commands::Fetch { limit } => {
        // Handle fetch command
    }
    Commands::Results { limit, offset } => {
        // Handle results command
    }
    // Compiler enforces exhaustive matching
}
```

#### 3. Type-Safe Error Handling

```rust
// No try/catch - errors are values
let result: Result<IPTUData, anyhow::Error> = scrape_iptu(...).await;

match result {
    Ok(data) => {
        // Success path
    }
    Err(e) => {
        // Error path - e contains context
        tracing::error!("Scraping failed: {}", e);
    }
}

// Or use ? for early return
let data = scrape_iptu(...).await?;  // Returns error if failed
```

#### 4. Async/Await

```rust
// Sequential-looking code that runs concurrently
async fn process_job(driver: &WebDriver, number: &str) 
    -> Result<IPTUData> {
    driver.goto("...").await?;           // Suspends, CPU does other work
    sleep(Duration::from_secs(2)).await; // Non-blocking sleep
    let data = extract_data(driver).await?;
    Ok(data)
}

// Multiple jobs run concurrently
let tasks = vec![
    process_job(&driver1, "123"),
    process_job(&driver2, "456"),
    process_job(&driver3, "789"),
];
let results = join_all(tasks).await;  // All run in parallel
```

### Performance Characteristics

**Memory Usage**:
- Base: ~10 MB (Rust binary)
- Per ChromeDriver: ~150-200 MB
- Total for 5 concurrent: ~1 GB

**CPU Usage**:
- Mostly I/O bound (waiting for web pages)
- Rust overhead: < 5% CPU
- ChromeDriver: 10-20% per instance

**Throughput**:
- Single worker: 3-5 jobs/minute
- 5 concurrent workers: 15-25 jobs/minute
- Bottleneck: Website load times, not application

**Why Rust Excels Here**:
- Async I/O handles waiting efficiently
- Low overhead allows more concurrent workers
- Memory safety prevents crashes during long runs
- Type system prevents logic errors

### Compilation and Optimization

**Debug Build** (faster compilation):

```bash
cargo build
# - No optimizations
# - Debug symbols included
# - ~2-3x slower runtime
# - Useful for development
```

**Release Build** (optimized):

```bash
cargo build --release
# - Full LLVM optimizations
# - No debug symbols
# - 2-3x faster runtime
# - Binary stripped and compact
# - Use for production
```

**What the Compiler Does**:
1. **Borrow checking**: Ensures memory safety
2. **Monomorphization**: Generates specialized code for generics
3. **Inlining**: Eliminates function call overhead
4. **Dead code elimination**: Removes unused code
5. **LLVM optimization**: State-of-the-art optimization backend

**Result**: Single binary with no runtime dependencies

## Troubleshooting

### ChromeDriver Issues

**Error**: "Could not connect to ChromeDriver"

```bash
# Check if ChromeDriver is running
ps aux | grep chromedriver

# Start manually
chromedriver --port=9515
```

**Error**: "Session not created: version mismatch"

- Download ChromeDriver matching your Chrome version
- Check version: `chrome://version` in your browser
- Download from: [chromedriver.chromium.org](https://chromedriver.chromium.org/)

### Supabase Connection Errors

**Error**: "Failed to fetch pending jobs"

- Verify `SUPABASE_URL` in `.env`
- Check API keys are correct and not expired
- Ensure tables exist (run setup SQL)
- Test connection: `curl $SUPABASE_URL/rest/v1/`

**Error**: "Permission denied"

- Use `SUPABASE_SERVICE_ROLE_KEY` instead of anon key
- Check RLS policies on tables
- Verify service role key has proper permissions

### Rate Limiting

**Symptom**: Many failed scrapes, "page did not load" errors

**Solution**:

- Reduce `--concurrent` value (try 1-2)
- Lower `--rate-limit` (try 50-100)
- Check `chromedriver.log` for rate limit messages
- Wait 60 seconds before retrying after detection

### Common Errors

**Error**: "Número de cadastro inválido"

- Contributor number must be 11+ digits
- Format: `XXX.XXX.XXXX-X`

**Error**: "Campos de entrada não encontrados"

- Website structure may have changed
- Cookie modal may be blocking form
- Check debug HTML in `~/Desktop/iptus/`

## Best Practices

### Production Deployment

1. **Use release builds**:

   ```bash
   cargo build --release
   ./target/release/iptu-cli process --limit 1000
   ```

2. **Set appropriate logging**:

   ```bash
   RUST_LOG=info ./target/release/iptu-cli process
   ```

3. **Monitor for failures**:

   - Check batch success rates in database
   - Watch for sudden drops in throughput
   - Review `chromedriver.log` for errors
   - Set up alerts for error rates > 20%

4. **Optimize concurrency**:

   - Start with `--concurrent 1`
   - Gradually increase to 3-5
   - Monitor success rate (keep above 90%)
   - Back off if success rate drops

### Development Tips

1. **Fast feedback loop**:

   ```bash
   # Use cargo watch for auto-rebuild
   cargo install cargo-watch
   cargo watch -x 'run -- process --limit 1'
   ```

2. **Debug specific jobs**:

   ```bash
   RUST_LOG=debug cargo run -- process --numbers "123.456.7890-1"
   ```

3. **Test without headless mode**:

   ```bash
   cargo run -- process --headless false --limit 1
   ```

4. **Profile performance**:

   ```bash
   # Use cargo flamegraph for performance analysis
   cargo install flamegraph
   cargo flamegraph -- process --limit 100
   ```

### Data Management

**Populate Job Queue**:

```sql
-- Insert contributor numbers into queue
INSERT INTO iptus_list (contributor_number, status)
SELECT DISTINCT contributor_number, NULL
FROM your_source_table
ON CONFLICT (contributor_number) DO NOTHING;
```

**Query Results**:

```sql
-- Get successful results
SELECT * FROM iptus 
WHERE sucesso = true 
ORDER BY timestamp DESC;

-- Check batch statistics
SELECT 
    b.*,
    ROUND(100.0 * b.sucesso / NULLIF(b.total, 0), 2) as success_rate_pct
FROM batches b
ORDER BY created_at DESC;
```

**Reset Failed Jobs**:

```sql
-- Retry failed jobs
UPDATE iptus_list 
SET status = NULL 
WHERE status = 'e';
```

## Performance Tuning

### Concurrency Settings

| Concurrent Workers | Throughput | Success Rate | CPU Usage | Memory | Notes |
|--------------------|------------|--------------|-----------|--------|-------|
| 1 | 3-5/min | 95-99% | Low | ~200 MB | Safest option |
| 3 | 9-15/min | 90-95% | Medium | ~600 MB | Recommended |
| 5 | 15-25/min | 85-90% | High | ~1 GB | Aggressive |
| 10+ | Varies | <80% | Very High | >2 GB | Not recommended |

### Rate Limiting Strategies

**Conservative** (recommended for long runs):

```bash
cargo run -- process --concurrent 3 --rate-limit 100
```

**Balanced** (good throughput with safety):

```bash
cargo run -- process --concurrent 5 --rate-limit 150
```

**Aggressive** (short bursts only, high risk):

```bash
cargo run -- process --concurrent 5 --rate-limit 200
```

### Hardware Recommendations

**Minimum**:
- 4 GB RAM
- 2 CPU cores
- 1 GB disk space

**Recommended**:
- 8 GB RAM
- 4 CPU cores
- 5 GB disk space (for logs and debug files)

**Optimal**:
- 16 GB RAM
- 8 CPU cores
- 10 GB disk space

## License

MIT License

Copyright (c) 2025

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Contributing

Contributions are welcome! To contribute:

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Make your changes
4. Add tests for new functionality
5. Ensure code quality:
   ```bash
   cargo test
   cargo clippy -- -D warnings
   cargo fmt --check
   ```
6. Commit your changes: `git commit -m 'Add amazing feature'`
7. Push to the branch: `git push origin feature/amazing-feature`
8. Open a Pull Request

### Code Style

- Follow Rust standard conventions
- Use `cargo fmt` for formatting
- Address all `cargo clippy` warnings
- Add documentation for public APIs
- Write tests for new features

## Support

For issues or questions:

- **GitHub Issues**: Open an issue for bugs or feature requests
- **Logs**: Check `chromedriver.log` for browser automation errors
- **Debug Mode**: Use `RUST_LOG=debug` for verbose output
- **Documentation**: Review this README and inline code documentation

## Acknowledgments

Built with:
- [Rust](https://www.rust-lang.org/) - Systems programming language
- [Tokio](https://tokio.rs/) - Async runtime
- [thirtyfour](https://github.com/stevepryde/thirtyfour) - WebDriver client
- [Supabase](https://supabase.com/) - Backend as a Service
- [ChromeDriver](https://chromedriver.chromium.org/) - Browser automation
