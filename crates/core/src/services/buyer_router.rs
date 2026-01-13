use crate::models::{campaign::Campaign, lead::Lead};
use anyhow::Result;
use serde::{Deserialize, Serialize};

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
}

impl BuyerRouter {
    pub fn new(lead: Lead, campaigns: Vec<Campaign>, request_type: String) -> Self {
        Self {
            lead,
            campaigns,
            request_type,
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

    async fn route_ping(&self, _campaign: &Campaign) -> Result<BuyerResponse> {
        // For now, generate a mock response
        // TODO: Make actual HTTP request to buyer API
        let ping_id = format!("ping_{}", uuid::Uuid::new_v4());
        let promise_id = format!(
            "PROMISE_{}",
            hex::encode(rand::random::<[u8; 6]>()).to_uppercase()
        );

        // Mock price - in real implementation, this comes from buyer response
        let price = Some(100.0 + (rand::random::<f64>() * 50.0));

        Ok(BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: Some("Lead accepted".to_string()),
            promise_id: Some(promise_id),
            ping_id: Some(ping_id),
            post_id: None,
            price,
        })
    }

    async fn route_post(&self, _campaign: &Campaign) -> Result<BuyerResponse> {
        let promise_id = self
            .lead
            .promise_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing promise_id for post request"))?;

        // For now, generate a mock response
        // TODO: Make actual HTTP request to buyer API
        let post_id = format!("post_{}", uuid::Uuid::new_v4());

        // Mock price for post (in real integration price may be provided by buyer)
        let price = Some(100.0 + (rand::random::<f64>() * 50.0));

        Ok(BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: Some("Post accepted".to_string()),
            promise_id: Some(promise_id.clone()),
            ping_id: self.lead.ping_id.clone(),
            post_id: Some(post_id),
            price,
        })
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
}

#[cfg(test)]
#[path = "buyer_router_edge_case_tests.rs"]
mod buyer_router_edge_case_tests;

#[cfg(test)]
#[path = "buyer_router_http_tests.rs"]
mod buyer_router_http_tests;

#[cfg(test)]
mod tests {
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
    async fn test_route_ping_returns_success_fields() {
        let lead = sample_lead();
        let campaign = sample_campaign();
        let router = BuyerRouter::new(lead, vec![campaign], "ping".to_string());
        let resp = router.route().await.expect("route ping should succeed");
        assert!(resp.success);
        assert!(resp.ping_id.is_some());
        assert!(resp.promise_id.is_some());
        assert!(resp.price.is_some());
    }

    #[tokio::test]
    async fn test_route_post_requires_promise_id() {
        let lead = sample_lead(); // no promise_id
        let campaign = sample_campaign();
        let router = BuyerRouter::new(lead, vec![campaign], "post".to_string());
        let res = router.route().await;
        assert!(res.is_err(), "post without promise_id should return Err");
    }

    #[tokio::test]
    async fn test_route_fullpost_without_updating_lead_fails_post() {
        let mut lead = sample_lead();
        // Ensure lead has no promise_id so fullpost will fail at post stage
        lead.promise_id = None;
        let campaign = sample_campaign();
        let router = BuyerRouter::new(lead, vec![campaign], "fullpost".to_string());
        let res = router.route().await;
        // Because BuyerRouter::route_fullpost does not update `self.lead` with the ping promise_id,
        // the subsequent post attempt should fail due to missing promise_id.
        assert!(
            res.is_err(),
            "fullpost without persisted promise_id should return Err"
        );
    }

    #[tokio::test]
    async fn test_route_unknown_request_type_returns_error_response() {
        let lead = sample_lead();
        let campaign = sample_campaign();
        let router = BuyerRouter::new(lead, vec![campaign], "weird".to_string());
        let res = router.route().await.expect("should return BuyerResponse");
        assert!(!res.success);
        assert_eq!(res.status, "error");
        assert!(res.error.is_some());
    }
}
