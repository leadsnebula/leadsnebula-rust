// k6 load test script for Carina API
// Usage: k6 run scripts/load_test_k6.js
// Options:
//   - VUS (virtual users): k6 run --vus 10 scripts/load_test_k6.js
//   - Duration: k6 run --duration 30s scripts/load_test_k6.js
//   - Stages: k6 run scripts/load_test_k6.js (uses stages below)

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

// Custom metrics
const errorRate = new Rate('errors');
const pingAuctionTime = new Trend('ping_auction_time_ms');
const postTime = new Trend('post_time_ms');

// Configuration
export const options = {
    stages: [
        { duration: '30s', target: 10 },  // Ramp up to 10 users
        { duration: '1m', target: 10 },    // Stay at 10 users
        { duration: '30s', target: 20 },   // Ramp up to 20 users
        { duration: '1m', target: 20 },   // Stay at 20 users
        { duration: '30s', target: 0 },    // Ramp down
    ],
    thresholds: {
        'http_req_duration': ['p(95)<2000'], // 95% of requests should be below 2s
        'http_req_failed': ['rate<0.05'],    // Error rate should be less than 5%
        'errors': ['rate<0.05'],
    },
};

// Test payload
const payload = JSON.stringify({
    vertical: 'solar',
    first_name: 'John',
    last_name: 'Doe',
    email: `john.doe.${__VU}.${__ITER}@example.com`,
    cell_phone: '5551234567',
    street_address: '123 Main St',
    city: 'San Francisco',
    state: 'CA',
    zip: '94102',
    ip_address: '192.168.1.1',
    tcpa_consent: true,
    is_test: true,
});

const baseURL = __ENV.API_URL || 'http://localhost:3000';
const apiKey = __ENV.API_KEY || 'test-key-123';

export default function () {
    const params = {
        headers: {
            'Content-Type': 'application/json',
            'X-API-Key': apiKey,
        },
    };

    // Measure full auction flow (ping + post)
    const startTime = Date.now();
    
    const response = http.post(`${baseURL}/api/v1/leads`, payload, params);
    
    const totalTime = Date.now() - startTime;
    
    // Check response
    const success = check(response, {
        'status is 200 or 201': (r) => r.status === 200 || r.status === 201,
        'response has status field': (r) => {
            try {
                const body = JSON.parse(r.body);
                return body.status && body.status.success !== undefined;
            } catch {
                return false;
            }
        },
    });

    if (!success) {
        errorRate.add(1);
    } else {
        errorRate.add(0);
    }

    // Extract timing from response if available
    if (response.status === 200 || response.status === 201) {
        try {
            const body = JSON.parse(response.body);
            if (body.verbose && body.verbose.stages) {
                // Extract ping_auction timing if available
                const pingStage = body.verbose.stages.find((s: any) => s.name === 'ping_auction');
                if (pingStage && pingStage.duration_ms) {
                    pingAuctionTime.add(pingStage.duration_ms);
                }
                
                // Extract post timing if available
                const postStage = body.verbose.stages.find((s: any) => s.name === 'post');
                if (postStage && postStage.duration_ms) {
                    postTime.add(postStage.duration_ms);
                }
            }
        } catch (e) {
            // Ignore parsing errors
        }
    }

    // Add total time to trend
    pingAuctionTime.add(totalTime);

    // Sleep between requests (1-3 seconds)
    sleep(Math.random() * 2 + 1);
}

export function handleSummary(data) {
    return {
        'stdout': textSummary(data, { indent: ' ', enableColors: true }),
        'load_test_results.json': JSON.stringify(data),
    };
}

function textSummary(data, options) {
    // Simple text summary
    return `
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Load Test Results
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

HTTP Requests:
  Total: ${data.metrics.http_reqs.values.count}
  Failed: ${data.metrics.http_req_failed.values.rate * 100}%
  
Response Times:
  Average: ${data.metrics.http_req_duration.values.avg.toFixed(2)}ms
  P95: ${data.metrics.http_req_duration.values['p(95)'].toFixed(2)}ms
  P99: ${data.metrics.http_req_duration.values['p(99)'].toFixed(2)}ms
  Max: ${data.metrics.http_req_duration.values.max.toFixed(2)}ms

Ping Auction Time (if available):
  Average: ${data.metrics.ping_auction_time_ms ? data.metrics.ping_auction_time_ms.values.avg.toFixed(2) : 'N/A'}ms
  P95: ${data.metrics.ping_auction_time_ms ? data.metrics.ping_auction_time_ms.values['p(95)'].toFixed(2) : 'N/A'}ms

Post Time (if available):
  Average: ${data.metrics.post_time_ms ? data.metrics.post_time_ms.values.avg.toFixed(2) : 'N/A'}ms
  P95: ${data.metrics.post_time_ms ? data.metrics.post_time_ms.values['p(95)'].toFixed(2) : 'N/A'}ms

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
`;
}
