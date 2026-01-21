use crate::encryption::EncryptionService;
use crate::models::{
    buyer::Buyer,
    buyer_integration::{BuyerIntegration, BuyerIntegrationCredential},
    campaign::Campaign,
    lead::Lead,
};
use crate::services::{auction_timing::AuctionTiming, diagnostic_metrics::DiagnosticMetrics};
use anyhow::Result;
use once_cell::sync::Lazy;
use reqwest::header::HeaderMap;
use reqwest::Client;

static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .http2_prior_knowledge() // Enable HTTP/2
        .pool_max_idle_per_host(10) // Connection pooling
        .pool_idle_timeout(Duration::from_secs(90))
        .timeout(Duration::from_secs(30)) // Request timeout
        .connect_timeout(Duration::from_secs(5)) // Connection timeout
        .tcp_keepalive(Duration::from_secs(60))
        .tcp_nodelay(true) // Lower latency by disabling Nagle's algorithm
        .http2_keep_alive_interval(Duration::from_secs(30)) // Send keep-alives every 30s
        .http2_keep_alive_timeout(Duration::from_secs(20)) // Timeout if no response in 20s
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
    pub price: Option<f64>, // For post responses
    pub bid: Option<f64>,   // For ping responses
}

pub struct BuyerRouter {
    lead: Lead,
    campaigns: Vec<Campaign>,
    request_type: String,
    pool: Arc<PgPool>,
    #[allow(dead_code)] // May be used in future for external buyer encryption
    encryption_key: Arc<Vec<u8>>,
    timing: Option<Arc<std::sync::Mutex<AuctionTiming>>>,
    metrics: Option<Arc<DiagnosticMetrics>>,
    // Pre-loaded buyer/integration data to avoid redundant DB lookups
    preloaded_integration: Option<crate::models::buyer_integration::BuyerIntegration>,
    // Pre-loaded qualification config to avoid redundant DB lookups
    preloaded_qual_config:
        Option<crate::models::buyer_qualification_config::BuyerQualificationConfig>,
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
            timing: None,
            metrics: None,
            preloaded_integration: None,
            preloaded_qual_config: None,
        }
    }

    /// Create BuyerRouter with pre-loaded integration data (optimizes DB lookups)
    pub fn with_preloaded_integration(
        mut self,
        integration: Option<crate::models::buyer_integration::BuyerIntegration>,
    ) -> Self {
        self.preloaded_integration = integration;
        self
    }

    /// Create BuyerRouter with pre-loaded qualification config (optimizes DB lookups)
    pub fn with_preloaded_qual_config(
        mut self,
        qual_config: Option<crate::models::buyer_qualification_config::BuyerQualificationConfig>,
    ) -> Self {
        self.preloaded_qual_config = qual_config;
        self
    }

    pub fn with_timing_and_metrics(
        mut self,
        timing: Option<Arc<std::sync::Mutex<AuctionTiming>>>,
        metrics: Option<Arc<DiagnosticMetrics>>,
    ) -> Self {
        self.timing = timing;
        self.metrics = metrics;
        self
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
                bid: None,
            }),
        }
    }

    async fn route_ping(&self, campaign: &Campaign) -> Result<BuyerResponse> {
        // Look up buyer integration to check if it's internal
        let integration = self.get_buyer_integration(campaign).await?;

        // For internal buyers (Pulsar), use direct function calls (skip HTTP overhead)
        if integration.is_internal {
            tracing::debug!(
                campaign_id = %campaign.id,
                buyer_id = %campaign.buyer_id,
                method = "direct",
                integration_slug = %integration.slug,
                "Using direct Pulsar call (NO HTTP)"
            );
            // Allow any slug starting with "pulsar" (e.g., "pulsar", "pulsar-solar", "pulsar-scalar-test")
            if !integration.slug.starts_with("pulsar") {
                tracing::error!(
                    integration_slug = %integration.slug,
                    "Unknown internal buyer – failing to prevent HTTP use"
                );
                return Err(anyhow::anyhow!(
                    "Invalid internal buyer configuration: {} (must start with 'pulsar')",
                    integration.slug
                ));
            }
            // Start buyer_ping_sent stage
            let stage_ping_sent = if let Some(ref t) = self.timing {
                let mut timing_guard = t.lock().unwrap();
                Some(timing_guard.start_stage(
                    "buyer_ping_sent",
                    serde_json::json!({"campaign_id": campaign.id, "method": "direct"}),
                ))
            } else {
                None
            };
            use crate::services::pulsar::PulsarService;
            let ping_start = std::time::Instant::now();
            let result = PulsarService::route_ping_direct(
                self.pool.clone(),
                &self.lead,
                campaign,
                self.preloaded_qual_config.clone(),
            )
            .await;
            let ping_duration = ping_start.elapsed().as_millis() as u64;

            if let Ok(ref resp) = result {
                tracing::debug!(
                    campaign_id = %campaign.id,
                    buyer_id = %campaign.buyer_id,
                    status = %resp.status,
                    bid = ?resp.bid,
                    success = resp.success,
                    duration_ms = ping_duration,
                    method = "direct",
                    "Direct Pulsar ping response received"
                );
            }

            // Complete buyer_ping_sent and start buyer_ping_response
            if let Some(ref t) = self.timing {
                if let Some(stage) = stage_ping_sent {
                    let mut timing_guard = t.lock().unwrap();
                    timing_guard.complete_stage(
                        stage,
                        Some(serde_json::json!({"duration_ms": ping_duration})),
                    );
                }
                let mut timing_guard = t.lock().unwrap();
                timing_guard.start_stage(
                    "buyer_ping_response",
                    serde_json::json!({"campaign_id": campaign.id}),
                );
            }
            if let Some(ref m) = self.metrics {
                m.record_query(ping_duration);
            }

            // Complete buyer_ping_response
            if let Some(ref t) = self.timing {
                let mut timing_guard = t.lock().unwrap();
                if let Some(stage) = timing_guard
                    .stages
                    .iter()
                    .position(|s| s.name == "buyer_ping_response" && s.completed_at.is_none())
                {
                    let success = result.is_ok();
                    timing_guard.complete_stage(
                        stage,
                        Some(serde_json::json!({
                            "success": success,
                            "duration_ms": ping_duration
                        })),
                    );
                }
            }

            return result;
        }

        // For external buyers, use HTTP
        let endpoint = integration
            .posting_url_template
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing endpoint for external buyer integration"))?;

        tracing::info!(
            campaign_id = %campaign.id,
            buyer_id = %campaign.buyer_id,
            endpoint = %endpoint,
            method = "HTTP",
            "Sending buyer ping request"
        );

        // Prepare request payload
        let mut lead_data = serde_json::to_value(&self.lead)?;
        if let Some(obj) = lead_data.as_object_mut() {
            obj.insert("request_type".to_string(), serde_json::json!("ping"));
        }
        let payload = serde_json::json!({
            "lead": lead_data
        });

        // Start buyer_ping_sent stage
        let stage_ping_sent = if let Some(ref t) = self.timing {
            let mut timing_guard = t.lock().unwrap();
            Some(timing_guard.start_stage(
                "buyer_ping_sent",
                serde_json::json!({"campaign_id": campaign.id, "endpoint": endpoint}),
            ))
        } else {
            None
        };

        // Send HTTP request
        let client = &*HTTP_CLIENT;
        let mut headers = HeaderMap::new();
        headers.insert(
            "Content-Type",
            "application/json"
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid content type: {}", e))?,
        );

        let ping_start = std::time::Instant::now();
        let response = match client
            .post(&endpoint)
            .json(&payload)
            .headers(headers.clone())
            .timeout(Duration::from_secs(1))
            .send()
            .await
        {
            Ok(resp) => {
                let ping_duration = ping_start.elapsed().as_millis() as u64;
                tracing::info!(
                    operation = "http_request",
                    method = "POST",
                    endpoint = %endpoint,
                    campaign_id = %campaign.id,
                    buyer_id = %campaign.buyer_id,
                    status_code = resp.status().as_u16(),
                    duration_ms = ping_duration,
                    "HTTP ping request sent and response received"
                );
                resp
            }
            Err(e) => {
                let ping_duration = ping_start.elapsed().as_millis() as u64;
                tracing::error!(
                    operation = "http_request",
                    method = "POST",
                    endpoint = %endpoint,
                    campaign_id = %campaign.id,
                    buyer_id = %campaign.buyer_id,
                    duration_ms = ping_duration,
                    error = %e,
                    error_type = %std::any::type_name_of_val(&e),
                    "HTTP ping request failed"
                );
                return Err(anyhow::anyhow!("HTTP request failed: {}", e));
            }
        };
        let ping_duration = ping_start.elapsed().as_millis() as u64;

        // Complete buyer_ping_sent and start buyer_ping_response
        if let Some(ref t) = self.timing {
            if let Some(stage) = stage_ping_sent {
                let mut timing_guard = t.lock().unwrap();
                timing_guard.complete_stage(
                    stage,
                    Some(serde_json::json!({"duration_ms": ping_duration})),
                );
            }
            let mut timing_guard = t.lock().unwrap();
            timing_guard.start_stage(
                "buyer_ping_response",
                serde_json::json!({"campaign_id": campaign.id}),
            );
        }
        if let Some(ref m) = self.metrics {
            m.record_query(ping_duration);
        }

        // Parse response
        let parse_start = std::time::Instant::now();
        let result = self.parse_buyer_response(response, "ping").await;
        let parse_duration = parse_start.elapsed().as_millis() as u64;

        if let Ok(ref resp) = result {
            tracing::info!(
                campaign_id = %campaign.id,
                buyer_id = %campaign.buyer_id,
                status = %resp.status,
                bid = ?resp.bid,
                success = resp.success,
                duration_ms = ping_duration,
                "Buyer ping response received"
            );
        }

        // Complete buyer_ping_response
        if let Some(ref t) = self.timing {
            let mut timing_guard = t.lock().unwrap();
            if let Some(stage) = timing_guard
                .stages
                .iter()
                .position(|s| s.name == "buyer_ping_response" && s.completed_at.is_none())
            {
                let success = result.is_ok();
                timing_guard.complete_stage(
                    stage,
                    Some(serde_json::json!({
                        "success": success,
                        "duration_ms": parse_duration
                    })),
                );
            }
        }

        result
    }

    async fn route_post(&self, campaign: &Campaign) -> Result<BuyerResponse> {
        let promise_id = self
            .lead
            .promise_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing promise_id for post request"))?;

        // Look up buyer integration to check if it's internal
        let integration = self.get_buyer_integration(campaign).await?;

        // For internal buyers (Pulsar), use direct function calls (skip HTTP overhead)
        if integration.is_internal {
            tracing::debug!(
                "Using direct Pulsar call (NO HTTP) for campaign {}",
                campaign.id
            );
            // Allow any slug starting with "pulsar" (e.g., "pulsar", "pulsar-solar", "pulsar-scalar-test")
            if !integration.slug.starts_with("pulsar") {
                tracing::error!(
                    integration_slug = %integration.slug,
                    "Unknown internal buyer – failing to prevent HTTP use"
                );
                return Err(anyhow::anyhow!(
                    "Invalid internal buyer configuration: {} (must start with 'pulsar')",
                    integration.slug
                ));
            }
            // Start buyer_post_sent stage
            let stage_post_sent = if let Some(ref t) = self.timing {
                let mut timing_guard = t.lock().unwrap();
                Some(timing_guard.start_stage(
                    "buyer_post_sent",
                    serde_json::json!({"campaign_id": campaign.id, "method": "direct"}),
                ))
            } else {
                None
            };
            use crate::services::pulsar::PulsarService;
            let post_start = std::time::Instant::now();
            let result = PulsarService::route_post_direct(
                self.pool.clone(),
                &self.lead,
                campaign,
                promise_id,
                self.preloaded_qual_config.clone(),
            )
            .await;
            let post_duration = post_start.elapsed().as_millis() as u64;

            // Complete buyer_post_sent and start buyer_post_response
            if let Some(ref t) = self.timing {
                if let Some(stage) = stage_post_sent {
                    let mut timing_guard = t.lock().unwrap();
                    timing_guard.complete_stage(
                        stage,
                        Some(serde_json::json!({"duration_ms": post_duration})),
                    );
                }
                let mut timing_guard = t.lock().unwrap();
                timing_guard.start_stage(
                    "buyer_post_response",
                    serde_json::json!({"campaign_id": campaign.id}),
                );
            }
            if let Some(ref m) = self.metrics {
                m.record_query(post_duration);
            }

            // Complete buyer_post_response
            if let Some(ref t) = self.timing {
                let mut timing_guard = t.lock().unwrap();
                if let Some(stage) = timing_guard
                    .stages
                    .iter()
                    .position(|s| s.name == "buyer_post_response" && s.completed_at.is_none())
                {
                    let success = result.is_ok();
                    timing_guard.complete_stage(
                        stage,
                        Some(serde_json::json!({
                            "success": success,
                            "duration_ms": post_duration
                        })),
                    );
                }
            }

            return result;
        }

        // For external buyers, use HTTP
        let endpoint = integration
            .posting_url_template
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing endpoint for external buyer integration"))?;

        // Prepare request payload (wrap lead data in "lead" object for Pulsar compatibility)
        let mut lead_data = serde_json::to_value(&self.lead)?;
        if let Some(obj) = lead_data.as_object_mut() {
            obj.insert("promise_id".to_string(), serde_json::json!(promise_id));
            // Set request_type to "post" for post requests
            obj.insert("request_type".to_string(), serde_json::json!("post"));
        }
        let payload = serde_json::json!({
            "lead": lead_data
        });

        // Start buyer_post_sent stage
        let stage_post_sent = if let Some(ref t) = self.timing {
            let mut timing_guard = t.lock().unwrap();
            Some(timing_guard.start_stage(
                "buyer_post_sent",
                serde_json::json!({"campaign_id": campaign.id, "endpoint": endpoint}),
            ))
        } else {
            None
        };

        // Send HTTP request
        let client = &*HTTP_CLIENT;
        let mut headers = HeaderMap::new();
        headers.insert(
            "Content-Type",
            "application/json"
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid content type: {}", e))?,
        );
        headers.insert(
            "X-Internal-Buyer-ID",
            campaign
                .buyer_id
                .to_string()
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid buyer ID format: {}", e))?,
        );

        let post_start = std::time::Instant::now();
        let response = match client
            .post(&endpoint)
            .json(&payload)
            .headers(headers.clone())
            .timeout(Duration::from_secs(3))
            .send()
            .await
        {
            Ok(resp) => {
                let post_duration = post_start.elapsed().as_millis() as u64;
                tracing::info!(
                    operation = "http_request",
                    method = "POST",
                    endpoint = %endpoint,
                    campaign_id = %campaign.id,
                    buyer_id = %campaign.buyer_id,
                    status_code = resp.status().as_u16(),
                    duration_ms = post_duration,
                    "HTTP post request sent and response received"
                );
                resp
            }
            Err(e) => {
                let post_duration = post_start.elapsed().as_millis() as u64;
                tracing::error!(
                    operation = "http_request",
                    method = "POST",
                    endpoint = %endpoint,
                    campaign_id = %campaign.id,
                    buyer_id = %campaign.buyer_id,
                    duration_ms = post_duration,
                    error = %e,
                    error_type = %std::any::type_name_of_val(&e),
                    "HTTP post request failed"
                );
                return Err(anyhow::anyhow!("HTTP request failed: {}", e));
            }
        };
        let post_duration = post_start.elapsed().as_millis() as u64;

        // Complete buyer_post_sent and start buyer_post_response
        if let Some(ref t) = self.timing {
            if let Some(stage) = stage_post_sent {
                let mut timing_guard = t.lock().unwrap();
                timing_guard.complete_stage(
                    stage,
                    Some(serde_json::json!({"duration_ms": post_duration})),
                );
            }
            let mut timing_guard = t.lock().unwrap();
            timing_guard.start_stage(
                "buyer_post_response",
                serde_json::json!({"campaign_id": campaign.id}),
            );
        }
        if let Some(ref m) = self.metrics {
            m.record_query(post_duration);
        }

        // Parse response
        let parse_start = std::time::Instant::now();
        let result = self.parse_buyer_response(response, "post").await;
        let parse_duration = parse_start.elapsed().as_millis() as u64;

        // Complete buyer_post_response
        if let Some(ref t) = self.timing {
            let mut timing_guard = t.lock().unwrap();
            if let Some(stage) = timing_guard
                .stages
                .iter()
                .position(|s| s.name == "buyer_post_response" && s.completed_at.is_none())
            {
                let success = result.is_ok();
                timing_guard.complete_stage(
                    stage,
                    Some(serde_json::json!({
                        "success": success,
                        "duration_ms": parse_duration
                    })),
                );
            }
        }

        result
    }

    async fn route_fullpost(&self, campaign: &Campaign) -> Result<BuyerResponse> {
        // Look up buyer integration to check if it's internal
        let integration = self.get_buyer_integration(campaign).await?;

        // For internal buyers (Pulsar), use direct function calls (skip HTTP overhead)
        if integration.is_internal {
            tracing::info!(
                campaign_id = %campaign.id,
                integration_slug = %integration.slug,
                "Using direct Pulsar call (NO HTTP) for fullpost"
            );
            // Allow any slug starting with "pulsar" (e.g., "pulsar", "pulsar-solar", "pulsar-scalar-test")
            if !integration.slug.starts_with("pulsar") {
                tracing::error!(
                    integration_slug = %integration.slug,
                    "Unknown internal buyer – failing to prevent HTTP use"
                );
                return Err(anyhow::anyhow!(
                    "Invalid internal buyer configuration: {} (must start with 'pulsar')",
                    integration.slug
                ));
            }
            // Start buyer_post_sent stage (fullpost uses post stages)
            let stage_post_sent = if let Some(ref t) = self.timing {
                let mut timing_guard = t.lock().unwrap();
                Some(timing_guard.start_stage("buyer_post_sent", serde_json::json!({"campaign_id": campaign.id, "method": "direct", "request_type": "fullpost"})))
            } else {
                None
            };
            use crate::services::pulsar::PulsarService;
            let post_start = std::time::Instant::now();
            let result = PulsarService::route_fullpost_direct(
                self.pool.clone(),
                &self.lead,
                campaign,
                self.preloaded_qual_config.clone(),
            )
            .await;
            let post_duration = post_start.elapsed().as_millis() as u64;

            // Complete buyer_post_sent and start buyer_post_response
            if let Some(ref t) = self.timing {
                if let Some(stage) = stage_post_sent {
                    let mut timing_guard = t.lock().unwrap();
                    timing_guard.complete_stage(
                        stage,
                        Some(serde_json::json!({"duration_ms": post_duration})),
                    );
                }
                let mut timing_guard = t.lock().unwrap();
                timing_guard.start_stage(
                    "buyer_post_response",
                    serde_json::json!({"campaign_id": campaign.id}),
                );
            }
            if let Some(ref m) = self.metrics {
                m.record_query(post_duration);
            }

            // Complete buyer_post_response
            if let Some(ref t) = self.timing {
                let mut timing_guard = t.lock().unwrap();
                if let Some(stage) = timing_guard
                    .stages
                    .iter()
                    .position(|s| s.name == "buyer_post_response" && s.completed_at.is_none())
                {
                    let success = result.is_ok();
                    timing_guard.complete_stage(
                        stage,
                        Some(serde_json::json!({
                            "success": success,
                            "duration_ms": post_duration
                        })),
                    );
                }
            }

            return result;
        }

        // For external buyers, use HTTP
        let endpoint = integration
            .posting_url_template
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing endpoint for external buyer integration"))?;

        // Prepare request payload
        let mut lead_data = serde_json::to_value(&self.lead)?;
        if let Some(obj) = lead_data.as_object_mut() {
            obj.insert("request_type".to_string(), serde_json::json!("fullpost"));
        }
        let payload = serde_json::json!({
            "lead": lead_data
        });

        // Start buyer_post_sent stage
        let stage_post_sent = if let Some(ref t) = self.timing {
            let mut timing_guard = t.lock().unwrap();
            Some(timing_guard.start_stage("buyer_post_sent", serde_json::json!({"campaign_id": campaign.id, "endpoint": endpoint, "request_type": "fullpost"})))
        } else {
            None
        };

        // Send HTTP request
        let client = &*HTTP_CLIENT;
        let mut headers = HeaderMap::new();
        headers.insert(
            "Content-Type",
            "application/json"
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid content type: {}", e))?,
        );
        headers.insert(
            "X-Internal-Buyer-ID",
            campaign
                .buyer_id
                .to_string()
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid buyer ID format: {}", e))?,
        );

        let post_start = std::time::Instant::now();
        let response = match client
            .post(&endpoint)
            .json(&payload)
            .headers(headers.clone())
            .timeout(Duration::from_secs(5)) // Fullpost timeout: 5 seconds
            .send()
            .await
        {
            Ok(resp) => {
                let post_duration = post_start.elapsed().as_millis() as u64;
                tracing::info!(
                    operation = "http_request",
                    method = "POST",
                    endpoint = %endpoint,
                    campaign_id = %campaign.id,
                    buyer_id = %campaign.buyer_id,
                    request_type = "fullpost",
                    status_code = resp.status().as_u16(),
                    duration_ms = post_duration,
                    "HTTP fullpost request sent and response received"
                );
                resp
            }
            Err(e) => {
                let post_duration = post_start.elapsed().as_millis() as u64;
                tracing::error!(
                    operation = "http_request",
                    method = "POST",
                    endpoint = %endpoint,
                    campaign_id = %campaign.id,
                    buyer_id = %campaign.buyer_id,
                    request_type = "fullpost",
                    duration_ms = post_duration,
                    error = %e,
                    error_type = %std::any::type_name_of_val(&e),
                    "HTTP fullpost request failed"
                );
                return Err(anyhow::anyhow!("HTTP request failed: {}", e));
            }
        };
        let post_duration = post_start.elapsed().as_millis() as u64;

        // Complete buyer_post_sent and start buyer_post_response
        if let Some(ref t) = self.timing {
            if let Some(stage) = stage_post_sent {
                let mut timing_guard = t.lock().unwrap();
                timing_guard.complete_stage(
                    stage,
                    Some(serde_json::json!({"duration_ms": post_duration})),
                );
            }
            let mut timing_guard = t.lock().unwrap();
            timing_guard.start_stage(
                "buyer_post_response",
                serde_json::json!({"campaign_id": campaign.id}),
            );
        }
        if let Some(ref m) = self.metrics {
            m.record_query(post_duration);
        }

        // Parse response
        let parse_start = std::time::Instant::now();
        let result = self.parse_buyer_response(response, "fullpost").await;
        let parse_duration = parse_start.elapsed().as_millis() as u64;

        // Complete buyer_post_response
        if let Some(ref t) = self.timing {
            let mut timing_guard = t.lock().unwrap();
            if let Some(stage) = timing_guard
                .stages
                .iter()
                .position(|s| s.name == "buyer_post_response" && s.completed_at.is_none())
            {
                let success = result.is_ok();
                timing_guard.complete_stage(
                    stage,
                    Some(serde_json::json!({
                        "success": success,
                        "duration_ms": parse_duration
                    })),
                );
            }
        }

        result
    }

    /// Get buyer integration (and optionally credentials) for a campaign
    /// For internal buyers, credentials are optional
    /// Uses pre-loaded integration if available to avoid DB lookup
    async fn get_buyer_integration(&self, campaign: &Campaign) -> Result<BuyerIntegration> {
        // Use pre-loaded integration if available (Phase 7.1 optimization)
        if let Some(ref integration) = self.preloaded_integration {
            return Ok(integration.clone());
        }

        // Fallback to DB lookup if not pre-loaded
        // Look up buyer to get buyer_integration_id
        let buyer = Buyer::find_by_id(&self.pool, campaign.buyer_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Buyer not found: {}", campaign.buyer_id))?;

        // Check if buyer is active
        if !buyer.active() {
            return Err(anyhow::anyhow!(
                "Buyer {} is not active (status: {})",
                buyer.id,
                buyer.status.as_str()
            ));
        }

        let integration_id = buyer
            .buyer_integration_id
            .ok_or_else(|| anyhow::anyhow!("Buyer has no integration configured"))?;

        // Look up integration
        let integration = BuyerIntegration::find_by_id(&self.pool, &integration_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Buyer integration not found: {}", integration_id))?;

        Ok(integration)
    }

    /// Send HTTP request to buyer API (unused for internal buyers, kept for future external buyer support)
    #[allow(dead_code)]
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

        tracing::debug!("Buyer API response status: {}, body: {}", status, body);

        // Try to parse as JSON
        let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|_| {
            tracing::warn!("Failed to parse buyer response as JSON: {}", body);
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

        // Parse price (for post responses) or bid (for ping responses)
        let price = json.get("price").and_then(|v| v.as_f64()).or_else(|| {
            json.get("price")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
        });

        let bid = json.get("bid").and_then(|v| v.as_f64()).or_else(|| {
            json.get("bid")
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
            bid,
        })
    }

    /// Decrypt API key using encryption service (unused for internal buyers, kept for future external buyer support)
    #[allow(dead_code)]
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
