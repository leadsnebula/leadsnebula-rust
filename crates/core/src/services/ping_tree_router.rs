use crate::models::{
    campaign::Campaign, enums::LeadStatus, lead::Lead, ping_tree::PingTree,
    ping_tree_campaign::PingTreeCampaign,
};
use crate::services::buyer_router::BuyerResponse;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

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

    pub async fn route(&self, pool: &PgPool) -> Result<RoutingResult> {
        // Find active ping tree for publisher and vertical
        let ping_tree = match PingTree::find_for_routing(pool, &self.publisher_id, &self.vertical)
            .await?
        {
            Some(pt) => pt,
            None => {
                // Update lead status to error
                self.update_lead_status(pool, LeadStatus::Error, Some("No active ping tree found"))
                    .await?;
                return Ok(RoutingResult {
                    success: false,
                    status: "error".to_string(),
                    error: Some(format!(
                        "No active ping tree found for publisher {} and vertical {}",
                        self.publisher_id, self.vertical
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
                pool,
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
            PingTreeCampaign::find_enabled_for_ping_tree(pool, &ping_tree.id).await?;

        if ping_tree_campaigns.is_empty() {
            self.update_lead_status(
                pool,
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

        // Load campaigns
        let mut campaigns = Vec::new();
        let mut priority_map = std::collections::HashMap::new();

        for ptc in ping_tree_campaigns {
            if let Some(campaign) = Campaign::find_by_id(pool, &ptc.campaign_id).await? {
                if campaign.active() {
                    campaigns.push(campaign.clone());
                    priority_map.insert(campaign.id, ptc.priority);
                }
            }
        }

        if campaigns.is_empty() {
            self.update_lead_status(pool, LeadStatus::Error, Some("No valid campaigns found"))
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
                self.route_ping_auction(pool, &campaigns, &priority_map)
                    .await
            }
            "post" => self.route_post(pool, &campaigns).await,
            "fullpost" => {
                self.route_fullpost(pool, &campaigns, &ping_tree, &priority_map)
                    .await
            }
            _ => {
                self.update_lead_status(
                    pool,
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
        pool: &PgPool,
        campaigns: &[Campaign],
        priority_map: &std::collections::HashMap<Uuid, Option<i32>>,
    ) -> Result<RoutingResult> {
        use crate::services::buyer_router::BuyerRouter;
        use tokio::time::{timeout, Duration};

        let start_time = std::time::Instant::now();
        const PING_AUCTION_TIMEOUT: Duration = Duration::from_millis(1200); // 1.2 seconds

        // Send concurrent pings to all campaigns
        let mut tasks = Vec::new();
        for campaign in campaigns {
            let lead = self.lead.clone();
            let campaign_clone = campaign.clone();
            let request_type = self.request_type.clone();

            let task = tokio::spawn(async move {
                let router = BuyerRouter::new(lead, vec![campaign_clone], request_type);
                router.route().await
            });
            tasks.push((task, campaign.id));
        }

        // Wait for all responses with timeout
        let mut responses: Vec<(BuyerResponse, Uuid, Option<i32>)> = Vec::new();

        for (task, campaign_id) in tasks {
            match timeout(PING_AUCTION_TIMEOUT, task).await {
                Ok(Ok(Ok(response))) => {
                    let priority = priority_map.get(&campaign_id).copied().flatten();
                    responses.push((response, campaign_id, priority));
                }
                Ok(Ok(Err(e))) => {
                    tracing::error!("BuyerRouter error for campaign {}: {}", campaign_id, e);
                    // Add error response
                    responses.push((
                        BuyerResponse {
                            success: false,
                            status: "error".to_string(),
                            error: Some(e.to_string()),
                            message: None,
                            promise_id: None,
                            ping_id: None,
                            post_id: None,
                            price: None,
                        },
                        campaign_id,
                        priority_map.get(&campaign_id).copied().flatten(),
                    ));
                }
                Ok(Err(e)) => {
                    tracing::error!("Task error for campaign {}: {}", campaign_id, e);
                }
                Err(_) => {
                    tracing::warn!("Ping auction timeout for campaign {}", campaign_id);
                    // Add timeout response
                    responses.push((
                        BuyerResponse {
                            success: false,
                            status: "timeout".to_string(),
                            error: Some("Buyer did not respond within timeout period".to_string()),
                            message: None,
                            promise_id: None,
                            ping_id: None,
                            post_id: None,
                            price: None,
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

        // Filter valid responses (success=true, price > 0, status != timeout)
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
                .filter(|(r, _, _)| r.status == "rejected")
                .count();

            let final_status = if timeout_count == responses.len() {
                LeadStatus::Timeout
            } else if rejected_count > 0 {
                LeadStatus::Rejected
            } else {
                LeadStatus::Error
            };

            let status_str = final_status.as_str().to_string();
            self.update_lead_status(pool, final_status, Some("No valid buyer responses"))
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
            pool,
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
                    "ping_accepted".to_string()
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

    async fn route_post(&self, pool: &PgPool, campaigns: &[Campaign]) -> Result<RoutingResult> {
        // For post, use the campaign_id from the lead (set during ping)
        let campaign = if let Some(campaign_id) = self.lead.campaign_id {
            campaigns.iter().find(|c| c.id == campaign_id)
        } else {
            campaigns.first()
        };

        if let Some(campaign) = campaign {
            let post_id = format!("post_{}", uuid::Uuid::new_v4());
            self.update_lead_with_post(pool, &post_id).await?;

            Ok(RoutingResult {
                success: true,
                status: "post_accepted".to_string(),
                error: None,
                promise_id: self.lead.promise_id.clone(),
                ping_id: self.lead.ping_id.clone(),
                post_id: Some(post_id),
                price: None,
                campaign_id: Some(campaign.id),
                buyer_id: Some(campaign.buyer_id),
            })
        } else {
            self.update_lead_status(pool, LeadStatus::Error, Some("No campaign found for post"))
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
        pool: &PgPool,
        campaigns: &[Campaign],
        ping_tree: &PingTree,
        priority_map: &std::collections::HashMap<Uuid, Option<i32>>,
    ) -> Result<RoutingResult> {
        // If ping tree strategy is ping_post, split fullpost into ping/post
        if ping_tree.strategy == "ping_post" {
            let ping_result = self
                .route_ping_auction(pool, campaigns, priority_map)
                .await?;
            if !ping_result.success {
                return Ok(ping_result);
            }
            // Now send post
            self.route_post(pool, campaigns).await
        } else {
            self.update_lead_status(
                pool,
                LeadStatus::Error,
                Some("Fullpost strategy not yet implemented"),
            )
            .await?;
            Ok(RoutingResult {
                success: false,
                status: "error".to_string(),
                error: Some("Fullpost strategy not yet implemented for ping trees".to_string()),
                promise_id: None,
                ping_id: None,
                post_id: None,
                price: None,
                campaign_id: None,
                buyer_id: None,
            })
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

#[cfg(test)]
pub(crate) fn select_winner_for_test(
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
    // Sort by price (descending), then by priority (ascending)
    let mut sorted: Vec<_> = responses
        .iter()
        .filter(|(resp, _, _)| resp.price.is_some() && resp.price.unwrap_or(0.0) > 0.0)
        .collect();

    if sorted.is_empty() {
        // No valid responses, return first error response
        if let Some(first) = responses.first() {
            return (first.0.clone(), first.1, first.2);
        }
        // Should never happen, but handle it
        panic!("No responses provided to select_winner");
    }

    // Sort by price descending, then by priority ascending
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

    // Get highest price
    let highest_price = sorted[0].0.price.unwrap_or(0.0);

    // Find all candidates with highest price
    let candidates: Vec<_> = sorted
        .iter()
        .take_while(|(resp, _, _)| (resp.price.unwrap_or(0.0) - highest_price).abs() < 0.01)
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

impl Campaign {
    pub async fn find_by_id(pool: &PgPool, id: &Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Campaign>(
            "SELECT * FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn create_buyer_response(success: bool, price: Option<f64>, status: &str) -> BuyerResponse {
        BuyerResponse {
            success,
            status: status.to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price,
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
                                prop_assert!(winner_price >= price || (winner_price - price).abs() < 0.01);
                            }
                        }
                    }
                }
            }
        }
    }
}
