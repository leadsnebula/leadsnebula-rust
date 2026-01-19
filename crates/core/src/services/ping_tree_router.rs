use crate::models::{
    campaign::Campaign, enums::LeadStatus, lead::Lead, ping_tree::PingTree,
    ping_tree_campaign::PingTreeCampaign,
};
use crate::services::buyer_router::BuyerResponse;
use anyhow::Result;
use hex;
use rand;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

// Price comparison epsilon: prices within this value are considered equal
// Used for floating-point comparison to handle rounding differences
const PRICE_EPSILON: f64 = 0.01;

// Retry configuration for persistence operations
const PERSISTENCE_MAX_RETRIES: u32 = 3;
const PERSISTENCE_RETRY_DELAY_MS: u64 = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingResult {
    pub success: bool,
    pub status: String,
    pub error: Option<String>,
    pub promise_id: Option<String>,
    pub ping_id: Option<String>,
    pub post_id: Option<String>,
    pub price: Option<f64>,
    pub campaign_id: Option<Uuid>,
    pub buyer_id: Option<Uuid>,
}

pub struct PingTreeRouter {
    lead: Lead,
    publisher_id: Uuid,
    vertical: String,
    request_type: String,
}

impl PingTreeRouter {
    pub fn new(lead: Lead, publisher_id: Uuid, vertical: String, request_type: String) -> Self {
        Self {
            lead,
            publisher_id,
            vertical,
            request_type,
        }
    }

    pub async fn route(
        &self,
        pool: Arc<PgPool>,
        encryption_key: Arc<Vec<u8>>,
    ) -> Result<RoutingResult> {
        // Find active ping tree for publisher and vertical with revshare info
        let (ping_tree, _revshare_percentage, _revshare_flat_amount) =
            match PingTree::find_for_routing(pool.as_ref(), &self.publisher_id, &self.vertical)
                .await?
            {
                Some((pt, revshare_pct, revshare_flat)) => (pt, revshare_pct, revshare_flat),
                None => {
                    // Update lead status to error
                    self.update_lead_status(
                        pool.as_ref(),
                        LeadStatus::Error,
                        Some("Publisher not assigned to any ping tree"),
                    )
                    .await?;
                    return Ok(RoutingResult {
                        success: false,
                        status: "error".to_string(),
                        error: Some(format!(
                            "Publisher not assigned to any ping tree for vertical: {}",
                            self.vertical
                        )),
                        promise_id: None,
                        ping_id: None,
                        post_id: None,
                        price: None,
                        campaign_id: None,
                        buyer_id: None,
                    });
                }
            };

        // Check ping tree status
        if !ping_tree.is_active() {
            self.update_lead_status(
                pool.as_ref(),
                LeadStatus::Error,
                Some(&format!("Ping tree is {}", ping_tree.status)),
            )
            .await?;
            return Ok(RoutingResult {
                success: false,
                status: "error".to_string(),
                error: Some(format!(
                    "Ping tree is {}. Only active ping trees accept requests.",
                    ping_tree.status
                )),
                promise_id: None,
                ping_id: None,
                post_id: None,
                price: None,
                campaign_id: None,
                buyer_id: None,
            });
        }

        // Get enabled campaigns from ping tree
        let ping_tree_campaigns =
            PingTreeCampaign::find_enabled_for_ping_tree(pool.as_ref(), &ping_tree.id).await?;

        if ping_tree_campaigns.is_empty() {
            self.update_lead_status(
                pool.as_ref(),
                LeadStatus::Error,
                Some("No active campaigns found in ping tree"),
            )
            .await?;
            return Ok(RoutingResult {
                success: false,
                status: "error".to_string(),
                error: Some("No active campaigns found in ping tree".to_string()),
                promise_id: None,
                ping_id: None,
                post_id: None,
                price: None,
                campaign_id: None,
                buyer_id: None,
            });
        }

        // Load campaigns in batch (optimize N+1 queries)
        let campaign_ids: Vec<Uuid> = ping_tree_campaigns
            .iter()
            .map(|ptc| ptc.campaign_id)
            .collect();
        let all_campaigns = Campaign::find_by_ids(pool.as_ref(), &campaign_ids).await?;

        let mut campaigns = Vec::new();
        let mut priority_map = std::collections::HashMap::new();

        for ptc in ping_tree_campaigns {
            if let Some(campaign) = all_campaigns.iter().find(|c| c.id == ptc.campaign_id) {
                if campaign.active() {
                    campaigns.push(campaign.clone());
                    priority_map.insert(campaign.id, ptc.priority);
                }
            }
        }

        if campaigns.is_empty() {
            self.update_lead_status(
                pool.as_ref(),
                LeadStatus::Error,
                Some("No valid campaigns found"),
            )
            .await?;
            return Ok(RoutingResult {
                success: false,
                status: "error".to_string(),
                error: Some("No valid campaigns found".to_string()),
                promise_id: None,
                ping_id: None,
                post_id: None,
                price: None,
                campaign_id: None,
                buyer_id: None,
            });
        }

        // Route based on request type
        match self.request_type.as_str() {
            "ping" => {
                self.route_ping_auction(
                    pool.clone(),
                    &campaigns,
                    &priority_map,
                    encryption_key.clone(),
                )
                .await
            }
            "post" => {
                self.route_post(pool.clone(), &campaigns, encryption_key.clone())
                    .await
            }
            "fullpost" => {
                self.route_fullpost(
                    pool.clone(),
                    &campaigns,
                    &ping_tree,
                    &priority_map,
                    encryption_key.clone(),
                )
                .await
            }
            _ => {
                self.update_lead_status(
                    pool.as_ref(),
                    LeadStatus::Error,
                    Some(&format!("Unknown request_type: {}", self.request_type)),
                )
                .await?;
                Ok(RoutingResult {
                    success: false,
                    status: "error".to_string(),
                    error: Some(format!("Unknown request_type: {}", self.request_type)),
                    promise_id: None,
                    ping_id: None,
                    post_id: None,
                    price: None,
                    campaign_id: None,
                    buyer_id: None,
                })
            }
        }
    }

    async fn route_ping_auction(
        &self,
        pool: Arc<PgPool>,
        campaigns: &[Campaign],
        priority_map: &std::collections::HashMap<Uuid, Option<i32>>,
        encryption_key: Arc<Vec<u8>>,
    ) -> Result<RoutingResult> {
        use crate::services::buyer_router::BuyerRouter;
        use futures::future::join_all;
        use tokio::time::{timeout, Duration};

        let start_time = std::time::Instant::now();
        const PING_AUCTION_TIMEOUT: Duration = Duration::from_millis(1200); // 1.2 seconds

        // Send concurrent pings to all campaigns
        let mut task_futures = Vec::new();
        for campaign in campaigns {
            let lead = self.lead.clone();
            let campaign_clone = campaign.clone();
            let request_type = self.request_type.clone();

            let pool_clone = pool.clone();
            let encryption_key_clone = encryption_key.clone();
            let campaign_id = campaign.id;

            // Wrap each task with timeout and store campaign_id for result mapping
            let task_future = async move {
                let task = tokio::spawn(async move {
                    let router = BuyerRouter::new(
                        lead,
                        vec![campaign_clone],
                        request_type,
                        pool_clone,
                        encryption_key_clone,
                    );
                    router.route().await
                });
                let result = timeout(PING_AUCTION_TIMEOUT, task).await;
                (result, campaign_id)
            };
            task_futures.push(task_future);
        }

        // Wait for all responses concurrently (instead of sequentially)
        let task_results = join_all(task_futures).await;
        let mut responses: Vec<(BuyerResponse, Uuid, Option<i32>)> = Vec::new();

        for (result, campaign_id) in task_results {
            match result {
                Ok(Ok(Ok(response))) => {
                    tracing::info!(
                        "Buyer response for campaign {}: success={}, status={}, bid={:?}, price={:?}, promise_id={:?}, error={:?}",
                        campaign_id,
                        response.success,
                        response.status,
                        response.bid,
                        response.price,
                        response.promise_id,
                        response.error
                    );
                    let priority = priority_map.get(&campaign_id).copied().flatten();
                    responses.push((response, campaign_id, priority));
                }
                Ok(Ok(Err(e))) => {
                    tracing::error!("BuyerRouter error for campaign {}: {}", campaign_id, e);
                    // Add error response - generate ping_id for ping requests so it shows up in leads report
                    let ping_id = if self.request_type == "ping" {
                        Some(format!("ping_error_{}", uuid::Uuid::new_v4()))
                    } else {
                        None
                    };
                    responses.push((
                        BuyerResponse {
                            success: false,
                            status: "error".to_string(),
                            error: Some(e.to_string()),
                            message: None,
                            promise_id: None,
                            ping_id,
                            post_id: None,
                            price: None,
                            bid: None,
                        },
                        campaign_id,
                        priority_map.get(&campaign_id).copied().flatten(),
                    ));
                }
                Ok(Err(e)) => {
                    tracing::error!("Task error for campaign {}: {}", campaign_id, e);
                    // Add error response to track task failures - generate ping_id for ping requests
                    let ping_id = if self.request_type == "ping" {
                        Some(format!("ping_error_{}", uuid::Uuid::new_v4()))
                    } else {
                        None
                    };
                    responses.push((
                        BuyerResponse {
                            success: false,
                            status: "error".to_string(),
                            error: Some(format!("Task failed: {}", e)),
                            message: None,
                            promise_id: None,
                            ping_id,
                            post_id: None,
                            price: None,
                            bid: None,
                        },
                        campaign_id,
                        priority_map.get(&campaign_id).copied().flatten(),
                    ));
                }
                Err(_) => {
                    tracing::warn!("Ping auction timeout for campaign {}", campaign_id);
                    // Add timeout response - generate ping_id for ping requests so it shows up in leads report
                    let ping_id = if self.request_type == "ping" {
                        Some(format!("ping_timeout_{}", uuid::Uuid::new_v4()))
                    } else {
                        None
                    };
                    responses.push((
                        BuyerResponse {
                            success: false,
                            status: "timeout".to_string(),
                            error: Some("Buyer did not respond within timeout period".to_string()),
                            message: None,
                            promise_id: None,
                            ping_id,
                            post_id: None,
                            price: None,
                            bid: None,
                        },
                        campaign_id,
                        priority_map.get(&campaign_id).copied().flatten(),
                    ));
                }
            }
        }

        let total_time_ms = start_time.elapsed().as_millis() as u64;
        tracing::info!(
            "Ping auction completed in {}ms with {} responses",
            total_time_ms,
            responses.len()
        );

        // Log performance metrics to Sentry for monitoring
        #[cfg(feature = "sentry")]
        {
            sentry::configure_scope(|scope| {
                scope.set_extra("ping_auction_duration_ms", total_time_ms.to_string().into());
                scope.set_tag("ping_auction_responses", responses.len().to_string());
            });
        }

        // Persist each buyer response for audit (with retry logic). Store plaintext JSON; API layer will encrypt rows when SSM keys are available.
        for (resp, campaign_id, _pri) in &responses {
            // Find buyer_id from campaigns list
            let buyer_id_opt = campaigns
                .iter()
                .find(|c| c.id == *campaign_id)
                .map(|c| c.buyer_id);
            // Serialize response - use empty object on error (best-effort persistence)
            let resp_json = serde_json::to_value(resp).unwrap_or_else(|_| serde_json::json!({}));
            // Ensure ping_id is set - if response doesn't have one, generate one for ping requests
            let ping_id_val = resp.ping_id.clone().or_else(|| {
                if self.request_type == "ping" {
                    Some(format!("ping_{}", uuid::Uuid::new_v4()))
                } else {
                    None
                }
            });

            // Persist with retry logic for transient database errors asynchronously
            // Clone only what's necessary for the async task
            let pool_clone = pool.as_ref().clone();
            let lead_id_val = self.lead.uuid;
            let campaign_id_val = *campaign_id;
            let payload_owned = resp_json; // Move instead of clone since we don't need it after
            let ping_owned = ping_id_val; // Move instead of clone
            let post_owned: Option<String> = None;
            let buyer_id_owned = buyer_id_opt;

            let handle = tokio::spawn(async move {
                PingTreeRouter::persist_buyer_response_with_retry(
                    pool_clone,
                    lead_id_val,
                    ping_owned,
                    post_owned,
                    buyer_id_owned,
                    campaign_id_val,
                    payload_owned,
                )
                .await;
            });
            // Log errors if task panics (fire-and-forget, but log for observability)
            tokio::spawn(async move {
                if let Err(e) = handle.await {
                    tracing::error!(
                        "Persistence task panicked for lead {} campaign {}: {:?}",
                        lead_id_val,
                        campaign_id_val,
                        e
                    );
                    #[cfg(feature = "sentry")]
                    {
                        sentry::capture_message(
                            &format!("Persistence task panicked: {:?}", e),
                            sentry::Level::Error,
                        );
                    }
                }
            });
        }

        // Filter valid responses
        // For ping requests: success=true, bid > 0, status != timeout, promise_id required
        // For post requests: success=true, price > 0, status != timeout
        let valid_responses: Vec<_> = responses
            .iter()
            .filter(|(resp, campaign_id, _)| {
                // For ping auctions, check for bid (not price)
                let has_bid = resp.bid.is_some() && resp.bid.unwrap_or(0.0) > 0.0;
                let has_promise_id = resp.promise_id.is_some();
                let is_valid = resp.success
                    && has_bid
                    && has_promise_id
                    && resp.status != "timeout";

                if !is_valid {
                    let reason = if !resp.success {
                        "success=false".to_string()
                    } else if !has_bid {
                        format!("missing or invalid bid (bid={:?})", resp.bid)
                    } else if !has_promise_id {
                        "missing promise_id".to_string()
                    } else if resp.status == "timeout" {
                        "timeout".to_string()
                    } else {
                        format!("status={}", resp.status)
                    };

                    tracing::warn!(
                        "Invalid buyer ping response for campaign {}: {} (success={}, bid={:?}, promise_id={:?}, status={}, error={:?})",
                        campaign_id,
                        reason,
                        resp.success,
                        resp.bid,
                        resp.promise_id,
                        resp.status,
                        resp.error
                    );
                }

                is_valid
            })
            .collect();

        if valid_responses.is_empty() {
            let timeout_count = responses
                .iter()
                .filter(|(r, _, _)| r.status == "timeout")
                .count();
            let rejected_count = responses
                .iter()
                .filter(|(r, _, _)| r.status == "rejected")
                .count();

            tracing::error!(
                "No valid buyer responses. Total responses: {}, timeouts: {}, rejected: {}, errors: {}",
                responses.len(),
                timeout_count,
                rejected_count,
                responses.iter().filter(|(r, _, _)| r.status == "error").count()
            );

            let final_status = if timeout_count == responses.len() {
                LeadStatus::Timeout
            } else if rejected_count > 0 {
                LeadStatus::Rejected
            } else {
                LeadStatus::Error
            };

            let status_str = final_status.as_str().to_string();
            self.update_lead_status(
                pool.as_ref(),
                final_status,
                Some("No valid buyer responses"),
            )
            .await?;

            return Ok(RoutingResult {
                success: false,
                status: status_str,
                error: Some(format!(
                    "No valid buyer responses ({} timeouts, {} rejected)",
                    timeout_count, rejected_count
                )),
                promise_id: None,
                ping_id: None,
                post_id: None,
                price: None,
                campaign_id: None,
                buyer_id: None,
            });
        }

        // Select winner: highest price, then priority, then random
        let winner = select_winner(valid_responses);
        let (winner_response, winner_campaign_id, _) = winner;

        // Find winning campaign
        let winner_campaign = campaigns
            .iter()
            .find(|c| c.id == winner_campaign_id)
            .ok_or_else(|| anyhow::anyhow!("Winner campaign not found"))?;

        // Update lead with winner
        self.update_lead_with_winner(
            pool.as_ref(),
            winner_campaign,
            winner_response.promise_id.as_deref().unwrap_or(""),
            winner_response.ping_id.as_deref(),
            winner_response.price,
        )
        .await?;

        Ok(RoutingResult {
            success: winner_response.success,
            status: Self::map_ping_status_to_lead_status(
                &winner_response.status,
                winner_response.success,
            ),
            error: winner_response.error,
            promise_id: winner_response.promise_id,
            ping_id: winner_response.ping_id,
            post_id: winner_response.post_id,
            price: winner_response.price,
            campaign_id: Some(winner_campaign_id),
            buyer_id: Some(winner_campaign.buyer_id),
        })
    }

    fn map_ping_status_to_lead_status(buyer_status: &str, success: bool) -> String {
        match buyer_status.to_lowercase().as_str() {
            "rejected" | "declined" | "denied" => "rejected".to_string(),
            "accepted" => {
                if success {
                    "accepted".to_string()
                } else {
                    "error".to_string()
                }
            }
            "timeout" => "timeout".to_string(),
            "invalid" | "invalid_lead" | "validation_error" => "invalid".to_string(),
            "error" | "server_error" | "internal_error" => "error".to_string(),
            _ => {
                if success {
                    "ping_accepted".to_string()
                } else {
                    "error".to_string()
                }
            }
        }
    }

    async fn route_post(
        &self,
        pool: Arc<PgPool>,
        campaigns: &[Campaign],
        encryption_key: Arc<Vec<u8>>,
    ) -> Result<RoutingResult> {
        use crate::services::buyer_router::BuyerRouter;

        #[allow(unused_variables)]
        let start_time = std::time::Instant::now();

        // Validate that lead.campaign_id exists in the provided campaigns (from ping tree)
        if let Some(campaign_id) = self.lead.campaign_id {
            if !campaigns.iter().any(|c| c.id == campaign_id) {
                // Log performance metrics to Sentry for monitoring (even on error)
                #[cfg(feature = "sentry")]
                {
                    let total_time_ms = start_time.elapsed().as_millis() as u64;
                    sentry::configure_scope(|scope| {
                        scope.set_extra("post_duration_ms", total_time_ms.to_string().into());
                    });
                }

                self.update_lead_status(
                    pool.as_ref(),
                    LeadStatus::Error,
                    Some("Campaign from ping not found in ping tree"),
                )
                .await?;
                return Ok(RoutingResult {
                    success: false,
                    status: "error".to_string(),
                    error: Some("Campaign from ping not found in ping tree".to_string()),
                    promise_id: None,
                    ping_id: None,
                    post_id: None,
                    price: None,
                    campaign_id: None,
                    buyer_id: None,
                });
            }
        }

        // For post, prefer campaign_id from the lead but fallback to first available
        let campaign_opt = if let Some(campaign_id) = self.lead.campaign_id {
            campaigns.iter().find(|c| c.id == campaign_id).cloned()
        } else {
            campaigns.first().cloned()
        };

        if let Some(campaign) = campaign_opt {
            // Delegate to BuyerRouter for post handling
            let pool_clone = pool.clone();
            let encryption_key_clone = encryption_key.clone();
            let buyer_router = BuyerRouter::new(
                self.lead.clone(),
                vec![campaign.clone()],
                self.request_type.clone(),
                pool_clone,
                encryption_key_clone,
            );
            match buyer_router.route().await {
                Ok(bresp) => {
                    // Persist buyer response for this post attempt (with retry logic)
                    let bresp_json =
                        serde_json::to_value(&bresp).unwrap_or_else(|_| serde_json::json!({}));
                    // Persist asynchronously
                    let pool_clone = pool.as_ref().clone();
                    let lead_id_val = self.lead.uuid;
                    let campaign_id_val = campaign.id;
                    let payload_owned = bresp_json; // Move instead of clone
                    let ping_owned: Option<String> = None;
                    let post_owned = bresp.post_id.clone(); // Clone since bresp is used later
                    let buyer_id_owned = Some(campaign.buyer_id);

                    let handle = tokio::spawn(async move {
                        PingTreeRouter::persist_buyer_response_with_retry(
                            pool_clone,
                            lead_id_val,
                            ping_owned,
                            post_owned,
                            buyer_id_owned,
                            campaign_id_val,
                            payload_owned,
                        )
                        .await;
                    });
                    // Log errors if task panics (fire-and-forget, but log for observability)
                    tokio::spawn(async move {
                        if let Err(e) = handle.await {
                            tracing::error!(
                                "Post persistence task panicked for lead {} campaign {}: {:?}",
                                lead_id_val,
                                campaign_id_val,
                                e
                            );
                            #[cfg(feature = "sentry")]
                            {
                                sentry::capture_message(
                                    &format!("Post persistence task panicked: {:?}", e),
                                    sentry::Level::Error,
                                );
                            }
                        }
                    });

                    // Validate post response: must have success=true, post_id, and price > 0
                    if bresp.success
                        && bresp.post_id.is_some()
                        && bresp.price.is_some()
                        && bresp.price.unwrap_or(0.0) > 0.0
                    {
                        // Log performance metrics to Sentry for monitoring
                        #[cfg(feature = "sentry")]
                        {
                            let total_time_ms = start_time.elapsed().as_millis() as u64;
                            sentry::configure_scope(|scope| {
                                scope.set_extra(
                                    "post_duration_ms",
                                    total_time_ms.to_string().into(),
                                );
                            });
                        }

                        let post_id = bresp.post_id.clone().unwrap();
                        // Persist post acceptance
                        self.update_lead_with_post(pool.as_ref(), &post_id).await?;

                        Ok(RoutingResult {
                            success: true,
                            status: "sold".to_string(),
                            error: None,
                            promise_id: bresp.promise_id.clone(),
                            ping_id: bresp.ping_id.clone(),
                            post_id: Some(post_id),
                            price: bresp.price,
                            campaign_id: Some(campaign.id),
                            buyer_id: Some(campaign.buyer_id),
                        })
                    } else {
                        // Log performance metrics to Sentry for monitoring (even on rejection)
                        #[cfg(feature = "sentry")]
                        {
                            let total_time_ms = start_time.elapsed().as_millis() as u64;
                            sentry::configure_scope(|scope| {
                                scope.set_extra(
                                    "post_duration_ms",
                                    total_time_ms.to_string().into(),
                                );
                            });
                        }

                        // Buyer rejected or errored
                        let final_status = match bresp.status.as_str() {
                            "rejected" => LeadStatus::Rejected,
                            "timeout" => LeadStatus::Timeout,
                            _ => LeadStatus::Error,
                        };
                        self.update_lead_status(
                            pool.as_ref(),
                            final_status.clone(),
                            bresp.error.as_deref(),
                        )
                        .await?;
                        Ok(RoutingResult {
                            success: false,
                            status: match final_status.clone() {
                                LeadStatus::Rejected => "rejected".to_string(),
                                LeadStatus::Timeout => "timeout".to_string(),
                                _ => "error".to_string(),
                            },
                            error: bresp.error.clone(),
                            promise_id: bresp.promise_id.clone(),
                            ping_id: bresp.ping_id.clone(),
                            post_id: bresp.post_id.clone(),
                            price: bresp.price,
                            campaign_id: Some(campaign.id),
                            buyer_id: Some(campaign.buyer_id),
                        })
                    }
                }
                Err(e) => {
                    tracing::error!("BuyerRouter error during post: {}", e);
                    self.update_lead_status(
                        pool.as_ref(),
                        LeadStatus::Error,
                        Some("Buyer routing failed"),
                    )
                    .await?;
                    Ok(RoutingResult {
                        success: false,
                        status: "error".to_string(),
                        error: Some(e.to_string()),
                        promise_id: None,
                        ping_id: None,
                        post_id: None,
                        price: None,
                        campaign_id: Some(campaign.id),
                        buyer_id: Some(campaign.buyer_id),
                    })
                }
            }
        } else {
            // Log performance metrics to Sentry for monitoring (even on error)
            #[cfg(feature = "sentry")]
            {
                let total_time_ms = start_time.elapsed().as_millis() as u64;
                sentry::configure_scope(|scope| {
                    scope.set_extra("post_duration_ms", total_time_ms.into());
                });
            }

            self.update_lead_status(
                pool.as_ref(),
                LeadStatus::Error,
                Some("No campaign found for post"),
            )
            .await?;
            Ok(RoutingResult {
                success: false,
                status: "error".to_string(),
                error: Some("No campaign found for post".to_string()),
                promise_id: None,
                ping_id: None,
                post_id: None,
                price: None,
                campaign_id: None,
                buyer_id: None,
            })
        }
    }

    async fn route_fullpost(
        &self,
        pool: Arc<PgPool>,
        campaigns: &[Campaign],
        ping_tree: &PingTree,
        priority_map: &std::collections::HashMap<Uuid, Option<i32>>,
        encryption_key: Arc<Vec<u8>>,
    ) -> Result<RoutingResult> {
        #[allow(unused_variables)]
        let start_time = std::time::Instant::now();

        // If ping tree strategy is ping_post, split fullpost into ping/post
        if ping_tree.strategy == "ping_post" {
            // Create a temporary router with "ping" request_type for the ping auction phase
            let ping_router = PingTreeRouter::new(
                self.lead.clone(),
                self.publisher_id,
                self.vertical.clone(),
                "ping".to_string(), // Force "ping" request_type for ping auction
            );
            let ping_result = ping_router
                .route_ping_auction(
                    pool.clone(),
                    campaigns,
                    priority_map,
                    encryption_key.clone(),
                )
                .await?;
            if !ping_result.success {
                // Log performance metrics to Sentry for monitoring (even on early return)
                #[cfg(feature = "sentry")]
                {
                    let total_time_ms = start_time.elapsed().as_millis() as u64;
                    sentry::configure_scope(|scope| {
                        scope.set_extra("fullpost_duration_ms", total_time_ms.to_string().into());
                    });
                }
                return Ok(ping_result);
            }

            // Reload lead from database to get promise_id and campaign_id set by ping auction
            let updated_lead = sqlx::query_as::<_, Lead>("SELECT * FROM leads WHERE uuid = $1")
                .bind(self.lead.uuid)
                .fetch_one(pool.as_ref())
                .await?;

            // Create new router with updated lead for post routing
            let post_router = PingTreeRouter::new(
                updated_lead,
                self.publisher_id,
                self.vertical.clone(),
                "post".to_string(),
            );

            // Route post using the updated lead (which now has promise_id and campaign_id)
            let post_result = post_router
                .route_post(pool.clone(), campaigns, encryption_key.clone())
                .await?;

            // Log performance metrics to Sentry for monitoring
            #[cfg(feature = "sentry")]
            {
                let total_time_ms = start_time.elapsed().as_millis() as u64;
                sentry::configure_scope(|scope| {
                    scope.set_extra("fullpost_duration_ms", total_time_ms.to_string().into());
                });
            }

            // Merge ping and post results
            Ok(RoutingResult {
                success: post_result.success,
                status: post_result.status,
                error: post_result.error,
                promise_id: ping_result.promise_id.or(post_result.promise_id),
                ping_id: ping_result.ping_id.or(post_result.ping_id),
                post_id: post_result.post_id,
                price: post_result.price.or(ping_result.price),
                campaign_id: post_result.campaign_id.or(ping_result.campaign_id),
                buyer_id: post_result.buyer_id.or(ping_result.buyer_id),
            })
        } else {
            // Fullpost strategy: send single request with all lead data to all buyers
            // Similar to route_post but sends to all campaigns and selects winner by price
            use crate::services::buyer_router::BuyerRouter;

            // Send fullpost request to all campaigns in parallel
            let mut handles = Vec::new();
            for campaign in campaigns {
                let pool_clone = pool.clone();
                let encryption_key_clone = encryption_key.clone();
                let lead_clone = self.lead.clone();
                let campaign_clone = campaign.clone();
                let request_type = "fullpost".to_string();

                let handle = tokio::spawn(async move {
                    let buyer_router = BuyerRouter::new(
                        lead_clone,
                        vec![campaign_clone.clone()],
                        request_type,
                        pool_clone,
                        encryption_key_clone,
                    );
                    buyer_router.route().await
                });
                handles.push((campaign.id, handle));
            }

            // Collect responses
            let mut responses: Vec<(
                crate::services::buyer_router::BuyerResponse,
                Uuid,
                Option<i32>,
            )> = Vec::new();
            for (campaign_id, handle) in handles {
                match handle.await {
                    Ok(Ok(bresp)) => {
                        let priority = priority_map.get(&campaign_id).copied().flatten();
                        responses.push((bresp, campaign_id, priority));
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("Buyer router error for campaign {}: {}", campaign_id, e);
                    }
                    Err(e) => {
                        tracing::warn!("Task panicked for campaign {}: {:?}", campaign_id, e);
                    }
                }
            }

            // Filter valid responses: success=true, price > 0, status != timeout
            let valid_responses: Vec<_> = responses
                .iter()
                .filter(|(resp, _, _)| {
                    resp.success
                        && resp.price.is_some()
                        && resp.price.unwrap_or(0.0) > 0.0
                        && resp.status != "timeout"
                })
                .collect();

            if valid_responses.is_empty() {
                let timeout_count = responses
                    .iter()
                    .filter(|(r, _, _)| r.status == "timeout")
                    .count();
                let rejected_count = responses
                    .iter()
                    .filter(|(r, _, _)| !r.success && r.status != "timeout")
                    .count();

                tracing::warn!(
                    "No valid buyer responses for fullpost. Total responses: {}, timeouts: {}, rejected: {}",
                    responses.len(),
                    timeout_count,
                    rejected_count
                );

                let final_status = if timeout_count == responses.len() {
                    LeadStatus::Timeout
                } else if rejected_count > 0 {
                    LeadStatus::Rejected
                } else {
                    LeadStatus::Error
                };

                let status_str = final_status.as_str().to_string();
                self.update_lead_status(
                    pool.as_ref(),
                    final_status,
                    Some("No valid buyer responses"),
                )
                .await?;

                #[cfg(feature = "sentry")]
                {
                    let total_time_ms = start_time.elapsed().as_millis() as u64;
                    sentry::configure_scope(|scope| {
                        scope.set_extra("fullpost_duration_ms", total_time_ms.to_string().into());
                    });
                }

                return Ok(RoutingResult {
                    success: false,
                    status: status_str,
                    error: Some(format!(
                        "No valid buyer responses ({} timeouts, {} rejected)",
                        timeout_count, rejected_count
                    )),
                    promise_id: None,
                    ping_id: None,
                    post_id: None,
                    price: None,
                    campaign_id: None,
                    buyer_id: None,
                });
            }

            // Select winner: highest price, then priority, then random
            // For fullpost, sort by price (not bid)
            let mut sorted: Vec<_> = valid_responses.iter().collect();
            sorted.sort_by(|a, b| {
                let price_a = a.0.price.unwrap_or(0.0);
                let price_b = b.0.price.unwrap_or(0.0);
                match price_b.partial_cmp(&price_a) {
                    Some(std::cmp::Ordering::Equal) => {
                        // Same price, compare priorities (lower = higher priority)
                        let pri_a = a.2.unwrap_or(i32::MAX);
                        let pri_b = b.2.unwrap_or(i32::MAX);
                        pri_a.cmp(&pri_b)
                    }
                    Some(ord) => ord,
                    None => std::cmp::Ordering::Equal,
                }
            });
            let winner = sorted
                .first()
                .ok_or_else(|| anyhow::anyhow!("No valid responses"))?;
            let (winner_response, winner_campaign_id, _) = (winner.0.clone(), winner.1, winner.2);

            // Find winning campaign
            let winner_campaign = campaigns
                .iter()
                .find(|c| c.id == winner_campaign_id)
                .ok_or_else(|| anyhow::anyhow!("Winner campaign not found"))?;

            // Update lead with winner information
            let post_id = winner_response.post_id.clone();
            let price = winner_response.price;
            let promise_id = winner_response.promise_id.clone();

            // Generate promise_id if not present (for fullpost, it's optional but we'll generate one)
            let final_promise_id = promise_id.or_else(|| {
                Some(format!(
                    "PROMISE_{}",
                    hex::encode(rand::random::<[u8; 6]>()).to_uppercase()
                ))
            });

            // Update lead in database
            sqlx::query(
                "UPDATE leads SET campaign_id = $1, buyer_id = $2, promise_id = $3, post_id = $4, status = $5, updated_at = now() WHERE uuid = $6",
            )
            .bind(winner_campaign_id)
            .bind(winner_campaign.buyer_id)
            .bind(&final_promise_id)
            .bind(&post_id)
            .bind(&LeadStatus::Sold)
            .bind(self.lead.uuid)
            .execute(pool.as_ref())
            .await?;

            // Persist buyer responses asynchronously
            for (resp, campaign_id, _) in responses {
                let pool_clone = pool.as_ref().clone();
                let lead_id_val = self.lead.uuid;
                let campaign_id_val = campaign_id;
                let resp_json =
                    serde_json::to_value(&resp).unwrap_or_else(|_| serde_json::json!({}));
                let payload_owned = resp_json;
                let ping_owned: Option<String> = None;
                let post_owned = resp.post_id.clone();
                let buyer_id_owned = campaigns
                    .iter()
                    .find(|c| c.id == campaign_id)
                    .map(|c| c.buyer_id);

                let handle = tokio::spawn(async move {
                    PingTreeRouter::persist_buyer_response_with_retry(
                        pool_clone,
                        lead_id_val,
                        ping_owned,
                        post_owned,
                        buyer_id_owned,
                        campaign_id_val,
                        payload_owned,
                    )
                    .await;
                });
                tokio::spawn(async move {
                    if let Err(e) = handle.await {
                        tracing::error!("Persistence task panicked: {:?}", e);
                    }
                });
            }

            #[cfg(feature = "sentry")]
            {
                let total_time_ms = start_time.elapsed().as_millis() as u64;
                sentry::configure_scope(|scope| {
                    scope.set_extra("fullpost_duration_ms", total_time_ms.to_string().into());
                });
            }

            Ok(RoutingResult {
                success: true,
                status: "sold".to_string(),
                error: None,
                promise_id: final_promise_id,
                ping_id: None,
                post_id,
                price,
                campaign_id: Some(winner_campaign_id),
                buyer_id: Some(winner_campaign.buyer_id),
            })
        }
    }

    /// Persist buyer response with retry logic for transient database errors
    async fn persist_buyer_response_with_retry(
        pool: PgPool,
        lead_id: Uuid,
        ping_id: Option<String>,
        post_id: Option<String>,
        buyer_id: Option<Uuid>,
        campaign_id: Uuid,
        payload: serde_json::Value,
    ) {
        let mut retries = PERSISTENCE_MAX_RETRIES;

        while retries > 0 {
            let result = if ping_id.is_some() {
                sqlx::query("INSERT INTO buyer_responses (lead_id, ping_id, buyer_id, campaign_id, payload, created_at) VALUES ($1, $2, $3, $4, $5, now())")
                    .bind(lead_id)
                    .bind(ping_id.clone())
                    .bind(buyer_id)
                    .bind(campaign_id)
                    .bind(sqlx::types::Json(&payload))
                    .execute(&pool)
                    .await
            } else {
                sqlx::query("INSERT INTO buyer_responses (lead_id, post_id, buyer_id, campaign_id, payload, created_at) VALUES ($1, $2, $3, $4, $5, now())")
                    .bind(lead_id)
                    .bind(post_id.clone())
                    .bind(buyer_id)
                    .bind(campaign_id)
                    .bind(sqlx::types::Json(&payload))
                    .execute(&pool)
                    .await
            };

            match result {
                Ok(_) => {
                    // Success - no need to retry
                    break;
                }
                Err(e) => {
                    // Check if error is retryable (connection pool exhaustion, network issues)
                    let is_retryable = e.to_string().contains("connection")
                        || e.to_string().contains("timeout")
                        || e.to_string().contains("pool")
                        || matches!(e, sqlx::Error::PoolTimedOut);

                    if is_retryable && retries > 1 {
                        retries -= 1;
                        tracing::warn!(
                            "Failed to persist buyer response (retries remaining: {}): {}",
                            retries,
                            e
                        );
                        sleep(Duration::from_millis(PERSISTENCE_RETRY_DELAY_MS)).await;
                        continue;
                    } else {
                        // Non-retryable error or out of retries
                        tracing::error!(
                            "Failed to persist buyer response after {} retries (lead_id: {}, campaign_id: {}): {}",
                            PERSISTENCE_MAX_RETRIES - retries + 1,
                            lead_id,
                            campaign_id,
                            e
                        );
                        break;
                    }
                }
            }
        }
    }

    async fn update_lead_status(
        &self,
        pool: &PgPool,
        status: LeadStatus,
        error: Option<&str>,
    ) -> Result<()> {
        if self.lead.status == LeadStatus::Processing {
            let mut vertical_data = self.lead.vertical_data.clone();
            if let Some(err) = error {
                if let Some(obj) = vertical_data.as_object_mut() {
                    obj.insert(
                        "error_log".to_string(),
                        serde_json::json!({
                            "timestamp": chrono::Utc::now().to_rfc3339(),
                            "error": err,
                            "component": "ping_tree_router"
                        }),
                    );
                }
            }

            sqlx::query(
                "UPDATE leads SET status = $1, vertical_data = $2, updated_at = NOW() WHERE uuid = $3"
            )
            .bind(&status)
            .bind(&vertical_data)
            .bind(self.lead.uuid)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    async fn update_lead_with_winner(
        &self,
        pool: &PgPool,
        campaign: &Campaign,
        promise_id: &str,
        ping_id: Option<&str>,
        _price: Option<f64>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE leads 
            SET status = $1,
                campaign_id = $2,
                buyer_id = $3,
                promise_id = $4,
                ping_id = $5,
                updated_at = NOW()
            WHERE uuid = $6
            "#,
        )
        .bind(&LeadStatus::PingAccepted)
        .bind(campaign.id)
        .bind(campaign.buyer_id)
        .bind(promise_id)
        .bind(ping_id)
        .bind(self.lead.uuid)
        .execute(pool)
        .await?;
        Ok(())
    }

    async fn update_lead_with_post(&self, pool: &PgPool, post_id: &str) -> Result<()> {
        // Note: 'post_accepted' is not in the enum, using 'sold' as post acceptance typically means the lead was sold
        sqlx::query(
            "UPDATE leads SET status = $1, post_id = $2, updated_at = NOW() WHERE uuid = $3",
        )
        .bind(&LeadStatus::Sold)
        .bind(post_id)
        .bind(self.lead.uuid)
        .execute(pool)
        .await?;
        Ok(())
    }
}

// Make available to both tests and benchmarks
// Benchmarks are compiled as separate binaries, so need pub visibility
// This is a test helper and not part of the public API
#[doc(hidden)]
pub fn select_winner_for_test(
    responses: Vec<(&BuyerResponse, Uuid, Option<i32>)>,
) -> (BuyerResponse, Uuid, Option<i32>) {
    // Convert to owned values and create references for select_winner
    // We need to store owned values to create valid references
    let owned: Vec<(BuyerResponse, Uuid, Option<i32>)> = responses
        .into_iter()
        .map(|(r, id, p)| (r.clone(), id, p))
        .collect();
    // Create references from the owned vector
    // Create references from owned vector - need to reference the tuple itself
    let refs: Vec<&(BuyerResponse, Uuid, Option<i32>)> = owned.iter().collect();
    select_winner(refs)
}

fn select_winner(
    responses: Vec<&(BuyerResponse, Uuid, Option<i32>)>,
) -> (BuyerResponse, Uuid, Option<i32>) {
    // Sort by bid (descending) for ping auctions, then by priority (ascending)
    let mut sorted: Vec<_> = responses
        .iter()
        .filter(|(resp, _, _)| resp.bid.is_some() && resp.bid.unwrap_or(0.0) > 0.0)
        .collect();

    if sorted.is_empty() {
        // No valid responses, return first error response
        if let Some(first) = responses.first() {
            return (first.0.clone(), first.1, first.2);
        }
        // Should never happen, but handle it
        panic!("No responses provided to select_winner");
    }

    // Sort by bid descending, then by priority ascending
    sorted.sort_by(|a, b| {
        let bid_a = a.0.bid.unwrap_or(0.0);
        let bid_b = b.0.bid.unwrap_or(0.0);

        match bid_b.partial_cmp(&bid_a) {
            Some(std::cmp::Ordering::Equal) => {
                // Same bid, compare priorities (lower = higher priority)
                let pri_a = a.2.unwrap_or(i32::MAX);
                let pri_b = b.2.unwrap_or(i32::MAX);
                pri_a.cmp(&pri_b)
            }
            Some(ord) => ord,
            None => std::cmp::Ordering::Equal,
        }
    });

    // Get highest price
    let highest_price = sorted[0].0.price.unwrap_or(0.0);

    // Find all candidates with highest price (within epsilon tolerance)
    let candidates: Vec<_> = sorted
        .iter()
        .take_while(|(resp, _, _)| {
            (resp.price.unwrap_or(0.0) - highest_price).abs() < PRICE_EPSILON
        })
        .collect();

    // If only one, return it
    if candidates.len() == 1 {
        let (resp, id, pri) = candidates[0];
        return (resp.clone(), *id, *pri);
    }

    // Check for priorities (lower number = higher priority)
    let with_priority: Vec<_> = candidates.iter().filter(|(_, _, p)| p.is_some()).collect();

    if !with_priority.is_empty() {
        let winner = with_priority
            .iter()
            .min_by_key(|(_, _, p)| p.unwrap())
            .unwrap();
        let (resp, id, pri) = winner;
        return (resp.clone(), *id, *pri);
    }

    // Random selection from candidates with same price
    let winner = candidates[rand::random::<usize>() % candidates.len()];
    let (resp, id, pri) = winner;
    (resp.clone(), *id, *pri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn create_buyer_response(success: bool, price: Option<f64>, status: &str) -> BuyerResponse {
        // For ping auctions, use bid; for post auctions, use price
        // Since select_winner filters by bid, we need to set bid for ping auction tests
        BuyerResponse {
            success,
            status: status.to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price,
            bid: price, // Use price as bid for ping auction tests
        }
    }

    #[test]
    fn test_select_winner_highest_price() {
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();
        let campaign3 = Uuid::new_v4();

        let resp1 = create_buyer_response(true, Some(100.0), "accepted");
        let resp2 = create_buyer_response(true, Some(150.0), "accepted");
        let resp3 = create_buyer_response(true, Some(120.0), "accepted");

        let responses = vec![
            (&resp1, campaign1, None),
            (&resp2, campaign2, None),
            (&resp3, campaign3, None),
        ];

        let (winner, winner_id, _) = select_winner_for_test(responses);
        assert_eq!(winner_id, campaign2);
        assert_eq!(winner.price, Some(150.0));
    }

    #[test]
    fn test_select_winner_priority_breaks_tie() {
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();
        let campaign3 = Uuid::new_v4();

        let resp1 = create_buyer_response(true, Some(100.0), "accepted");
        let resp2 = create_buyer_response(true, Some(100.0), "accepted");
        let resp3 = create_buyer_response(true, Some(100.0), "accepted");

        let responses = vec![
            (&resp1, campaign1, Some(3)),
            (&resp2, campaign2, Some(1)), // Lower priority number = higher priority
            (&resp3, campaign3, Some(2)),
        ];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        assert_eq!(winner_id, campaign2); // Should win due to priority 1
    }

    #[test]
    fn test_select_winner_filters_invalid_responses() {
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();
        let campaign3 = Uuid::new_v4();

        let resp1 = create_buyer_response(true, Some(100.0), "accepted");
        let resp2 = create_buyer_response(false, Some(0.0), "rejected");
        let resp3 = create_buyer_response(true, None, "accepted");

        let responses = vec![
            (&resp1, campaign1, None),
            (&resp2, campaign2, None),
            (&resp3, campaign3, None),
        ];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        assert_eq!(winner_id, campaign1); // Only resp1 is valid
    }

    #[test]
    fn test_select_winner_no_valid_responses() {
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();

        let resp1 = create_buyer_response(false, Some(0.0), "rejected");
        let resp2 = create_buyer_response(false, None, "error");

        let responses = vec![(&resp1, campaign1, None), (&resp2, campaign2, None)];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        // Should return first error response
        assert_eq!(winner_id, campaign1);
    }

    #[test]
    fn test_select_winner_price_then_priority() {
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();
        let campaign3 = Uuid::new_v4();

        // campaign2 has highest price
        let resp1 = create_buyer_response(true, Some(100.0), "accepted");
        let resp2 = create_buyer_response(true, Some(150.0), "accepted");
        let resp3 = create_buyer_response(true, Some(100.0), "accepted");

        let responses = vec![
            (&resp1, campaign1, Some(1)), // Lower priority but lower price
            (&resp2, campaign2, Some(3)), // Higher price wins regardless of priority
            (&resp3, campaign3, Some(2)),
        ];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        assert_eq!(winner_id, campaign2); // Highest price wins
    }

    #[test]
    fn test_map_ping_status_to_lead_status_various() {
        // accepted + success => accepted
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("accepted", true),
            "accepted"
        );
        // accepted + failure => error
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("accepted", false),
            "error"
        );
        // rejected => rejected
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("rejected", false),
            "rejected"
        );
        // timeout => timeout
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("timeout", false),
            "timeout"
        );
        // unknown but success => ping_accepted
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("weirdstatus", true),
            "ping_accepted"
        );
        // unknown and not success => error
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("weirdstatus", false),
            "error"
        );
    }

    #[test]
    fn test_select_winner_epsilon_tie_by_priority() {
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();

        // Prices within epsilon (PRICE_EPSILON) should be considered tied
        let resp1 = create_buyer_response(true, Some(100.0), "accepted");
        let resp2 = create_buyer_response(true, Some(100.005), "accepted");

        let responses = vec![(&resp1, campaign1, Some(2)), (&resp2, campaign2, Some(1))];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        // campaign2 has better (lower) priority and should win the tie
        assert_eq!(winner_id, campaign2);
    }

    // Property-based test: winner should always have highest price or best priority
    proptest! {
        #[test]
        fn test_select_winner_property(
            prices in prop::collection::vec(1.0f64..1000.0, 2..10),
            priorities in prop::collection::vec(1i32..10, 2..10)
        ) {
            // Store owned responses to avoid lifetime issues
            let mut owned_responses = Vec::new();
            let mut campaign_ids = Vec::new();

            for price in prices.iter() {
                let campaign_id = Uuid::new_v4();
                campaign_ids.push(campaign_id);
                let resp = create_buyer_response(true, Some(*price), "accepted");
                owned_responses.push(resp);
            }

            // Create references from owned responses
            let mut responses = Vec::new();
            for (i, resp) in owned_responses.iter().enumerate() {
                let campaign_id = campaign_ids[i];
                let priority = priorities.get(i).copied();
                responses.push((resp, campaign_id, priority));
            }

            if !responses.is_empty() {
                let (winner, _, _) = select_winner_for_test(responses.clone());
                if let Some(winner_price) = winner.price {
                    // Winner price should be >= all other valid prices
                    for (resp, _, _) in &responses {
                        if let Some(price) = resp.price {
                            if price > 0.0 {
                                prop_assert!(winner_price >= price || (winner_price - price).abs() < PRICE_EPSILON);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "ping_tree_router_unit_tests.rs"]
mod ping_tree_router_unit_tests;

#[cfg(test)]
#[path = "ping_tree_router_integration_tests.rs"]
mod ping_tree_router_integration_tests;

#[cfg(test)]
#[path = "ping_tree_router_fullpost_tests.rs"]
mod ping_tree_router_fullpost_tests;

#[cfg(test)]
#[path = "async_persistence_tests.rs"]
mod async_persistence_tests;

#[cfg(test)]
#[path = "ping_tree_router_edge_case_tests.rs"]
mod ping_tree_router_edge_case_tests;

#[cfg(test)]
#[path = "ping_tree_router_db_integration_tests.rs"]
mod ping_tree_router_db_integration_tests;

#[cfg(test)]
#[path = "duplicate_post_concurrency_tests.rs"]
mod duplicate_post_concurrency_tests;

#[cfg(test)]
#[path = "load_tests.rs"]
mod load_tests;

#[cfg(test)]
#[path = "persistence_error_tests.rs"]
mod persistence_error_tests;
