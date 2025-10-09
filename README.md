# IPTU CLI

A command-line interface for extracting IPTU (São Paulo property tax) data from the municipal website.

## Features

- Fetch pending jobs from Supabase
- Process jobs with web scraping (using ChromeDriver)
- Upload results to Supabase
- Track batch progress
- Rate limiting and concurrent processing

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

## Database Schema

The CLI expects the following Supabase tables:

- `iptus_list`: Queue of contributor numbers to process
- `iptus`: Results table
- `batches`: Batch tracking table
