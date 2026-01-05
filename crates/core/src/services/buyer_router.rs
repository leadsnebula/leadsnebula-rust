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

        Ok(BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: Some("Post accepted".to_string()),
            promise_id: Some(promise_id.clone()),
            ping_id: self.lead.ping_id.clone(),
            post_id: Some(post_id),
            price: None,
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
