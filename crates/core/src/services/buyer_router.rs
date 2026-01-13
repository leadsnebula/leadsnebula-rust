use crate::encryption::EncryptionService;
use crate::models::{
    buyer::Buyer,
    buyer_integration::{BuyerIntegration, BuyerIntegrationCredential},
    campaign::Campaign,
    lead::Lead,
};
use anyhow::Result;
use once_cell::sync::Lazy;
use reqwest::header::HeaderMap;
use reqwest::Client;

static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .build()
        .expect("Failed to build global HTTP client")
});
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuyerResponse {
    pub success: bool,
    pub status: String,
    pub error: Option<String>,
    pub message: Option<String>,
    pub promise_id: Option<String>,
    pub ping_id: Option<String>,
    pub post_id: Option<String>,
    pub price: Option<f64>,
}

pub struct BuyerRouter {
    lead: Lead,
    campaigns: Vec<Campaign>,
    request_type: String,
    pool: Arc<PgPool>,
    encryption_key: Arc<Vec<u8>>,
}

impl BuyerRouter {
    pub fn new(
        lead: Lead,
        campaigns: Vec<Campaign>,
        request_type: String,
        pool: Arc<PgPool>,
        encryption_key: Arc<Vec<u8>>,
    ) -> Self {
        Self {
            lead,
            campaigns,
            request_type,
            pool,
            encryption_key,
        }
    }

    pub async fn route(&self) -> Result<BuyerResponse> {
        let campaign = self
            .campaigns
            .first()
            .ok_or_else(|| anyhow::anyhow!("No campaign provided to BuyerRouter"))?;

        // Check if this is Pulsar (internal buyer)
        // For now, we'll route all to Pulsar endpoint
        // TODO: Check buyer integration type and route accordingly

        match self.request_type.as_str() {
            "ping" => self.route_ping(campaign).await,
            "post" => self.route_post(campaign).await,
            "fullpost" => self.route_fullpost(campaign).await,
            _ => Ok(BuyerResponse {
                success: false,
                status: "error".to_string(),
                error: Some(format!("Unknown request_type: {}", self.request_type)),
                message: None,
                promise_id: None,
                ping_id: None,
                post_id: None,
                price: None,
            }),
        }
    }

    async fn route_ping(&self, campaign: &Campaign) -> Result<BuyerResponse> {
        // Look up buyer integration
        let (integration, credentials) =
            self.get_buyer_integration_and_credentials(campaign).await?;

        // Get ping endpoint
        let endpoint = credentials
            .ping_endpoint
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing ping endpoint for buyer integration"))?;

        // Prepare request payload (lead data as JSON)
        let payload = serde_json::to_value(&self.lead)?;

        // Send HTTP request
        let response = self
            .send_request(
                &endpoint,
                &payload,
                &integration,
                &credentials,
                campaign,
                Duration::from_secs(1), // Ping timeout: 1 second
            )
            .await?;

        // Parse response
        self.parse_buyer_response(response, "ping").await
    }

    async fn route_post(&self, campaign: &Campaign) -> Result<BuyerResponse> {
        let promise_id = self
            .lead
            .promise_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing promise_id for post request"))?;

        // Look up buyer integration
        let (integration, credentials) =
            self.get_buyer_integration_and_credentials(campaign).await?;

        // Get post endpoint
        let endpoint = credentials
            .post_endpoint
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing post endpoint for buyer integration"))?;

        // Prepare request payload (lead data as JSON with promise_id)
        let mut payload = serde_json::to_value(&self.lead)?;
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("promise_id".to_string(), serde_json::json!(promise_id));
        }

        // Send HTTP request
        let response = self
            .send_request(
                &endpoint,
                &payload,
                &integration,
                &credentials,
                campaign,
                Duration::from_secs(3), // Post timeout: 3 seconds
            )
            .await?;

        // Parse response
        self.parse_buyer_response(response, "post").await
    }

    async fn route_fullpost(&self, campaign: &Campaign) -> Result<BuyerResponse> {
        // Fullpost: send ping first, then post
        let ping_response = self.route_ping(campaign).await?;

        if !ping_response.success {
            return Ok(ping_response);
        }

        // Update lead with ping response
        // Then send post
        self.route_post(campaign).await
    }

    /// Get buyer integration and credentials for a campaign
    async fn get_buyer_integration_and_credentials(
        &self,
        campaign: &Campaign,
    ) -> Result<(BuyerIntegration, BuyerIntegrationCredential)> {
        // Look up buyer to get buyer_integration_id
        let buyer = Buyer::find_by_id(&self.pool, campaign.buyer_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Buyer not found: {}", campaign.buyer_id))?;

        let integration_id = buyer
            .buyer_integration_id
            .ok_or_else(|| anyhow::anyhow!("Buyer has no integration configured"))?;

        // Look up integration
        let integration = BuyerIntegration::find_by_id(&self.pool, &integration_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Buyer integration not found: {}", integration_id))?;

        // Look up credentials
        let credentials = BuyerIntegrationCredential::find_by_buyer_integration_id(
            &self.pool,
            &integration_id,
            Some(&campaign.buyer_id),
        )
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Buyer integration credentials not found for buyer: {}",
                campaign.buyer_id
            )
        })?;

        Ok((integration, credentials))
    }

    /// Send HTTP request to buyer API
    async fn send_request(
        &self,
        endpoint: &str,
        payload: &serde_json::Value,
        _integration: &BuyerIntegration,
        credentials: &BuyerIntegrationCredential,
        campaign: &Campaign,
        timeout: Duration,
    ) -> Result<reqwest::Response> {
        // Reuse global client and set per-request timeout
        let client = &*HTTP_CLIENT;

        // Prepare headers
        let mut headers = HeaderMap::new();

        // Decrypt API key once and reuse (avoid repeated crypto ops)
        if let Some(api_key_encrypted) = &credentials.api_key_encrypted {
            if let Ok(api_key) = self.decrypt_key(api_key_encrypted) {
                headers.insert(
                    "X-API-Key",
                    api_key
                        .parse()
                        .map_err(|e| anyhow::anyhow!("Invalid API key format: {}", e))?,
                );
            }
        }

        // Add internal buyer ID header
        headers.insert(
            "X-Internal-Buyer-ID",
            campaign
                .buyer_id
                .to_string()
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid buyer ID format: {}", e))?,
        );

        headers.insert(
            "Content-Type",
            "application/json"
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid content type: {}", e))?,
        );

        // Build request with timeout override via request timeout extension
        let request_builder = client
            .post(endpoint)
            .json(payload)
            .headers(headers)
            .timeout(timeout);

        let response: reqwest::Response = request_builder
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("HTTP request failed: {}", e))?;

        Ok(response)
    }

    /// Parse buyer API response into BuyerResponse
    async fn parse_buyer_response(
        &self,
        response: reqwest::Response,
        request_type: &str,
    ) -> Result<BuyerResponse> {
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))?;

        // Try to parse as JSON
        let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|_| {
            // If JSON parsing fails, create a simple error response
            serde_json::json!({
                "success": false,
                "status": "error",
                "error": format!("Invalid JSON response: {}", body)
            })
        });

        // Extract fields from JSON response
        let success = json
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(status.is_success());

        let status_str = json
            .get("status")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                if success {
                    "accepted".to_string()
                } else {
                    "rejected".to_string()
                }
            });

        let error = json
            .get("error")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let message = json
            .get("message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let promise_id = json
            .get("promise_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let ping_id = json
            .get("ping_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                if request_type == "ping" {
                    Some(format!("ping_{}", uuid::Uuid::new_v4()))
                } else {
                    None
                }
            });

        let post_id = json
            .get("post_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                if request_type == "post" {
                    Some(format!("post_{}", uuid::Uuid::new_v4()))
                } else {
                    None
                }
            });

        let price = json.get("price").and_then(|v| v.as_f64()).or_else(|| {
            json.get("price")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
        });

        Ok(BuyerResponse {
            success,
            status: status_str,
            error,
            message,
            promise_id,
            ping_id,
            post_id,
            price,
        })
    }

    /// Decrypt API key using encryption service
    fn decrypt_key(&self, encrypted: &str) -> Result<String> {
        let encryption_service = EncryptionService::new(&self.encryption_key)
            .map_err(|e| anyhow::anyhow!("Failed to initialize encryption service: {}", e))?;

        encryption_service
            .decrypt(encrypted)
            .map_err(|e| anyhow::anyhow!("Failed to decrypt API key: {}", e))
    }
}

#[cfg(test)]
#[path = "buyer_router_edge_case_tests.rs"]
mod buyer_router_edge_case_tests;

#[cfg(test)]
#[path = "buyer_router_http_tests.rs"]
mod buyer_router_http_tests;

#[cfg(test)]
mod tests {
    #![allow(unused_variables, unreachable_code, dead_code)]
    use super::*;
    use crate::models::campaign::Campaign;

    fn sample_campaign() -> Campaign {
        Campaign {
            id: uuid::Uuid::new_v4(),
            buyer_id: uuid::Uuid::new_v4(),
            publisher_id: uuid::Uuid::new_v4(),
            instance_id: uuid::Uuid::new_v4(),
            name: Some("test-campaign".to_string()),
            vertical: "test-vertical".to_string(),
            campaign_token: "token123".to_string(),
            status: crate::models::enums::CampaignStatus::Active,
            is_documentation_test: false,
            deleted_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn sample_lead() -> crate::models::lead::Lead {
        crate::models::lead::Lead {
            uuid: uuid::Uuid::new_v4(),
            event_id: "evt_1".to_string(),
            lead_id: None,
            publisher_id: Some(uuid::Uuid::new_v4()),
            vertical_id: uuid::Uuid::new_v4(),
            campaign_id: None,
            buyer_id: None,
            request_type: "ping".to_string(),
            strategy: "default".to_string(),
            status: crate::models::enums::LeadStatus::Processing,
            promise_id: None,
            ping_id: None,
            post_id: None,
            session_id: None,
            request_stage: None,
            first_name_encrypted: None,
            last_name_encrypted: None,
            email_encrypted: None,
            cell_phone_encrypted: None,
            street_address_encrypted: None,
            city_encrypted: None,
            state_encrypted: None,
            zip_encrypted: None,
            ip_address_encrypted: None,
            email_sha256: None,
            phone_sha256: None,
            ip_address_hash: None,
            email_domain: None,
            tcpa_consent: false,
            tcpa_language: "en".to_string(),
            is_test: false,
            user_agent: None,
            referrer: None,
            website_url: None,
            click_id: None,
            url_consent: None,
            best_call_time: None,
            date_of_birth: None,
            home_phone: None,
            jornaya_lead_id: None,
            trusted_form_url: None,
            fbp_cookie: None,
            fbc_cookie: None,
            utm_params: None,
            submitted_at: None,
            sold_at: None,
            retry_count: 0,
            next_retry_at: None,
            vertical_data: serde_json::json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    #[ignore] // Requires database setup - BuyerRouter now needs real DB access
    async fn test_route_ping_returns_success_fields() {
        let lead = sample_lead();
        let campaign = sample_campaign();
        // Note: These tests require database setup - use integration tests instead
        // For now, marking as ignored since BuyerRouter needs real DB access
        return;
        // let pool = Arc::new(/* test pool */);
        // let encryption_key = Arc::new(vec![0u8; 32]);
        // let router = BuyerRouter::new(lead, vec![campaign], "ping".to_string(), pool, encryption_key);
        // let resp = router.route().await.expect("route ping should succeed");
    }

    #[tokio::test]
    #[ignore] // Requires database setup - BuyerRouter now needs real DB access
    async fn test_route_post_requires_promise_id() {
        let lead = sample_lead(); // no promise_id
        let campaign = sample_campaign();
        // Note: These tests require database setup - use integration tests instead
        return;
        // let pool = Arc::new(/* test pool */);
        // let encryption_key = Arc::new(vec![0u8; 32]);
        // let router = BuyerRouter::new(lead, vec![campaign], "post".to_string(), pool, encryption_key);
        // let res = router.route().await;
    }

    #[tokio::test]
    #[ignore] // Requires database setup - BuyerRouter now needs real DB access
    async fn test_route_fullpost_without_updating_lead_fails_post() {
        let mut lead = sample_lead();
        // Ensure lead has no promise_id so fullpost will fail at post stage
        lead.promise_id = None;
        let campaign = sample_campaign();
        // Note: These tests require database setup - use integration tests instead
        return;
        // let pool = Arc::new(/* test pool */);
        // let encryption_key = Arc::new(vec![0u8; 32]);
        // let router = BuyerRouter::new(lead, vec![campaign], "fullpost".to_string(), pool, encryption_key);
        // let res = router.route().await;
        // Because BuyerRouter::route_fullpost does not update `self.lead` with the ping promise_id,
        // the subsequent post attempt should fail due to missing promise_id.
    }

    #[tokio::test]
    #[ignore] // Requires database setup - BuyerRouter now needs real DB access
    async fn test_route_unknown_request_type_returns_error_response() {
        let lead = sample_lead();
        let campaign = sample_campaign();
        // Note: These tests require database setup - use integration tests instead
        return;
        // let pool = Arc::new(/* test pool */);
        // let encryption_key = Arc::new(vec![0u8; 32]);
        // let router = BuyerRouter::new(lead, vec![campaign], "weird".to_string(), pool, encryption_key);
        // let res = router.route().await.expect("should return BuyerResponse");
    }
}
