#!/bin/bash

# Start ChromeDriver if not already running
if ! pgrep -f "chromedriver.*9515" > /dev/null; then
    echo "Starting ChromeDriver..."
    chromedriver --port=9515 > /tmp/chromedriver.log 2>&1 &
    sleep 2
    echo "ChromeDriver started!"
else
    echo "ChromeDriver is already running"
fi

echo ""
echo "========================================"
echo "Running Diretrix Scraper Test"
echo "========================================"
echo ""

# Run the example
cargo run --example diretrix_test

# Optionally kill chromedriver when done
# pkill -f "chromedriver.*9515"
