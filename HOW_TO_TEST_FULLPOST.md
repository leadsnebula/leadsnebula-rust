# How to Test Fullpost

## Prerequisites

1. **Database Setup**: Ensure you have a PostgreSQL database with:
   - Migrations applied
   - An active ping tree with `strategy = 'ping_post'`
   - At least one active campaign in the ping tree
   - A publisher configured

2. **API Server**: Start the API server:
   ```bash
   cd /home/badinoff/projects/leadsnebula/rust
   cargo run --bin leadsnebula-api
   ```

3. **Authentication**: You'll need a valid API key for the publisher

## Full JSON Request Example

### Complete Request (with all fields)

```json
{
  "verbose": false,
  "lead": {
    "publisher_id": "YOUR_PUBLISHER_UUID",
    "vertical": "solar",
    "request_type": "fullpost",
    "campaign_token": null,
    "promise_id": null,
    "lead_id": null,
    "first_name": "John",
    "last_name": "Doe",
    "email": "john.doe@example.com",
    "cell_phone": "5551234567",
    "mobile_phone": "5551234567",
    "street_address": "123 Main St",
    "city": "San Francisco",
    "state": "CA",
    "zip": "94102",
    "monthly_bill": 150.50,
    "credit_rating": "good",
    "own_home": true,
    "property_type": "single_family",
    "roof_shade": "partial",
    "roof_type": "asphalt",
    "utility_provider": "PG&E",
    "purchase_timeframe": "1-3_months",
    "ip_address": "192.168.1.1",
    "tcpa_consent": true,
    "tcpa_language": "en",
    "jornaya_lead_id": null,
    "trusted_form_url": null,
    "is_test": false,
    "verbose": false
  }
}
```

### Minimal Request (required fields only)

```json
{
  "lead": {
    "vertical": "solar",
    "request_type": "fullpost",
    "first_name": "Jane",
    "last_name": "Smith",
    "email": "jane.smith@example.com",
    "cell_phone": "5559876543",
    "street_address": "456 Oak Ave",
    "city": "Los Angeles",
    "state": "CA",
    "zip": "90001",
    "monthly_bill": 200.00,
    "own_home": true,
    "tcpa_consent": true
  }
}
```

## Testing with cURL

### Basic Request

```bash
curl -X POST http://localhost:3000/api/v1/leads \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -d @FULLPOST_TEST_REQUEST.json
```

### With Verbose Output

```bash
curl -X POST http://localhost:3000/api/v1/leads \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -d '{
    "verbose": true,
    "lead": {
      "vertical": "solar",
      "request_type": "fullpost",
      "first_name": "Test",
      "last_name": "User",
      "email": "test@example.com",
      "cell_phone": "5551234567",
      "street_address": "123 Test St",
      "city": "San Francisco",
      "state": "CA",
      "zip": "94102",
      "monthly_bill": 150.00,
      "own_home": true,
      "tcpa_consent": true
    }
  }'
```

## Expected Response

### Success Response (Fullpost Completed)

```json
{
  "status": {
    "success": true,
    "status": "sold",
    "message": "Lead Sold for 100.00"
  },
  "lead": {
    "promise_id": "PROMISE_ABC123",
    "lead_id": "lead_xyz789",
    "lead_uuid": "uuid-here",
    "ping_id": null,
    "bid": null,
    "post_id": "RP_base64encoded",
    "price": 100.00
  },
  "verbose": {
    "error_code": "ERR_200",
    "timestamp": "2024-01-14T12:00:00Z",
    "endpoint": "POST /api/v1/leads",
    "status_code": 200,
    "routing": {
      "processing_time_ms": 1200,
      "buyer_name": "Test Buyer",
      "buyer_id": "buyer-uuid",
      "campaign_name": "Test Campaign",
      "campaign_id": "campaign-uuid"
    }
  },
  "http_status": 200
}
```

### Ping Fails Response

```json
{
  "status": {
    "success": false,
    "status": "rejected",
    "message": null,
    "error": "No valid buyer responses (0 timeouts, 3 rejected)"
  },
  "lead": {
    "promise_id": null,
    "lead_id": null,
    "lead_uuid": null,
    "ping_id": null,
    "bid": null,
    "post_id": null,
    "price": null
  },
  "http_status": 200
}
```

## What Happens During Fullpost

1. **Request Arrives**: API receives fullpost request
2. **Ping Payload Saved**: Request payload saved to `ping_payloads` table
3. **Ping Auction**: Concurrent pings sent to all campaigns in ping tree
4. **Winner Selected**: Highest price → priority → random
5. **Lead Updated**: Lead updated with `promise_id`, `ping_id`, `campaign_id`
6. **Post Routing**: Post sent to winning campaign using `promise_id`
7. **Post Payload Saved**: Post request/response saved to `post_payloads` table
8. **Response Returned**: Combined ping/post result returned

## Verification Steps

1. **Check Lead Status**:
   ```sql
   SELECT uuid, status, promise_id, ping_id, post_id, campaign_id, buyer_id 
   FROM leads 
   WHERE uuid = 'YOUR_LEAD_UUID';
   ```

2. **Check Ping Payload**:
   ```sql
   SELECT id, lead_id, ping_id, payload, request_payload_encrypted, response_payload_encrypted
   FROM ping_payloads 
   WHERE lead_id = 'YOUR_LEAD_UUID';
   ```

3. **Check Post Payload**:
   ```sql
   SELECT id, lead_id, post_id, payload, request_payload_encrypted, response_payload_encrypted
   FROM post_payloads 
   WHERE lead_id = 'YOUR_LEAD_UUID';
   ```

4. **Check Buyer Responses**:
   ```sql
   SELECT id, lead_id, ping_id, post_id, buyer_id, campaign_id, payload
   FROM buyer_responses 
   WHERE lead_id = 'YOUR_LEAD_UUID'
   ORDER BY created_at;
   ```

## Troubleshooting

### Error: "No active ping tree found"
- Ensure ping tree exists with `status = 'active'` and `strategy = 'ping_post'`
- Verify publisher_id matches

### Error: "No active campaigns found in ping tree"
- Ensure at least one campaign is enabled in the ping tree
- Campaign must have `status = 'active'`

### Error: "Missing promise_id for post request"
- This should not happen with fullpost (it's generated during ping)
- Check that ping auction succeeded before post routing

### Post Fails After Ping Succeeds
- Verify lead was updated with `promise_id` after ping
- Check that campaign_id from ping matches available campaigns

## Test Files

- `FULLPOST_TEST_REQUEST.json` - Complete request example
- `FULLPOST_TEST_REQUEST_MINIMAL.json` - Minimal required fields
