#!/bin/bash

# Enhanced load test script with metrics collection and reporting
# Usage: ./scripts/load_test.sh [endpoint_url] [concurrent_requests] [total_requests]

set -e

ENDPOINT_URL="${1:-http://localhost:3000/api/v1/carina}"
CONCURRENT="${2:-10}"
TOTAL="${3:-100}"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Load Testing: $ENDPOINT_URL"
echo "Concurrent requests: $CONCURRENT"
echo "Total requests: $TOTAL"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Check if wrk is installed
if ! command -v wrk > /dev/null 2>&1; then
    echo "⚠️  wrk not found. Installing via apt..."
    sudo apt-get update && sudo apt-get install -y wrk
fi

# Check if API key is set
if [ -z "$API_KEY" ]; then
    echo "⚠️  API_KEY not set. Using dummy key for testing."
    echo "   Set API_KEY environment variable for real testing."
    API_KEY="test-key-123"
fi

# Create a simple test payload
PAYLOAD_FILE=$(mktemp)
cat > "$PAYLOAD_FILE" <<EOF
{
  "vertical": "solar",
  "first_name": "John",
  "last_name": "Doe",
  "email": "john.doe@example.com",
  "cell_phone": "5551234567",
  "street_address": "123 Main St",
  "city": "San Francisco",
  "state": "CA",
  "zip": "94102",
  "ip_address": "192.168.1.1",
  "tcpa_consent": true,
  "is_test": true
}
EOF

echo "Running load test..."
echo ""

# Create results directory
RESULTS_DIR="load_test_results"
mkdir -p "$RESULTS_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_FILE="$RESULTS_DIR/load_test_${TIMESTAMP}.txt"

echo "Results will be saved to: $RESULTS_FILE"
echo ""

# Run wrk with custom Lua script for POST requests and capture output
{
    echo "Load Test Results - $(date)"
    echo "Endpoint: $ENDPOINT_URL"
    echo "Concurrent: $CONCURRENT"
    echo "Total: $TOTAL"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    wrk -t"$CONCURRENT" -c"$CONCURRENT" -d30s --latency \
        -s - <<'LUA'
wrk.method = "POST"
wrk.headers["Content-Type"] = "application/json"
wrk.headers["X-API-Key"] = os.getenv("API_KEY") or "test-key-123"

local errors = 0
local success = 0

request = function()
    local body = '{"vertical":"solar","first_name":"John","last_name":"Doe","email":"john.doe@example.com","cell_phone":"5551234567","street_address":"123 Main St","city":"San Francisco","state":"CA","zip":"94102","ip_address":"192.168.1.1","tcpa_consent":true,"is_test":true}'
    return wrk.format("POST", "/api/v1/carina", wrk.headers, body)
end

response = function(status, headers, body)
    if status == 200 or status == 201 then
        success = success + 1
    else
        errors = errors + 1
    end
end

done = function(summary, latency, requests)
    io.write("Errors: ", errors, "\n")
    io.write("Success: ", success, "\n")
end
LUA
    
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Load test completed at $(date)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
} | tee "$RESULTS_FILE"

# Cleanup
rm -f "$PAYLOAD_FILE"

echo ""
echo "Results saved to: $RESULTS_FILE"
echo "To compare with baseline, run: diff $RESULTS_FILE <previous_results_file>"
