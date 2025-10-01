#!/bin/bash

# Script to start ChromeDriver for IPTU scraping with optimized settings

echo "Starting ChromeDriver on port 9515..."

# Check if ChromeDriver is already running
if lsof -Pi :9515 -sTCP:LISTEN -t >/dev/null ; then
    echo "ChromeDriver is already running on port 9515"
    echo "Killing existing ChromeDriver process..."
    kill $(lsof -t -i:9515)
    sleep 2
fi

# Start ChromeDriver with verbose logging
# --verbose flag helps debug issues
# --whitelisted-ips allows connections from localhost
chromedriver \
    --port=9515 \
    --whitelisted-ips= \
    --verbose \
    --log-path=chromedriver.log &

DRIVER_PID=$!

# Wait for ChromeDriver to start
sleep 3

# Check if ChromeDriver started successfully
if ps -p $DRIVER_PID > /dev/null; then
    echo "ChromeDriver started successfully (PID: $DRIVER_PID)"
    echo "Logs: tail -f chromedriver.log"
    echo "To stop ChromeDriver, run: kill $DRIVER_PID"
    echo ""
    echo "ChromeDriver is ready!"
else
    echo "Failed to start ChromeDriver"
    exit 1
fi