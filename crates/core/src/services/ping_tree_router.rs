use crate::cache::CacheService;
use crate::models::{
    buyer_qualification_config::BuyerQualificationConfig, campaign::Campaign, enums::LeadStatus,
    lead::Lead, ping_tree::PingTree, ping_tree_campaign::PingTreeCampaign,
};
use crate::services::auction_timing::AtomicAuctionTiming;
use crate::services::buyer_router::BuyerResponse;
use crate::services::diagnostic_metrics::DiagnosticMetrics;
use anyhow::Result;
use futures::stream::{FuturesUnordered, StreamExt};
use hex;
use rand;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::sleep;
use tokio_retry::{strategy::ExponentialBackoff, Retry};
use uuid::Uuid;

// Price comparison epsilon: prices within this value are considered equal
// Used for floating-point comparison to handle rounding differences
const PRICE_EPSILON: f64 = 0.01;

// Chaos mode: inject random delays for testing resilience
// Set CHAOS=1 environment variable to enable
fn should_inject_chaos() -> bool {
    std::env::var("CHAOS").unwrap_or_default() == "1"
}

async fn inject_chaos_delay() {
    if should_inject_chaos() {
        let delay_ms = rand::random::<u64>() % 150 + 50; // 50-200ms
        sleep(Duration::from_millis(delay_ms)).await;
    }
}

// Type alias for buyer response batch insert rows
type BuyerResponseRow = (
    Uuid,
    Option<String>,
    Option<String>,
    Option<Uuid>,
    Uuid,
    serde_json::Value,
);

// Type alias for ping task results to reduce complexity
type PingTaskResult = (
    Result<Result<BuyerResponse, anyhow::Error>, anyhow::Error>,
    Uuid,
    u64,
);

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
    pub per_buyer_timings: Option<Vec<serde_json::Value>>,
}

pub struct PingTreeRouter {
    lead: Lead,
    publisher_id: Uuid,
    vertical: String,
    request_type: String,
    cache: Option<Arc<CacheService>>,
    timing: Option<Arc<AtomicAuctionTiming>>,
    metrics: Option<Arc<DiagnosticMetrics>>,
    write_behind_queue: Option<Arc<crate::services::write_behind_queue::WriteBehindQueue>>,
}

impl PingTreeRouter {
    pub fn new(
        lead: Lead,
        publisher_id: Uuid,
        vertical: String,
        request_type: String,
        cache: Option<Arc<CacheService>>,
        write_behind_queue: Option<Arc<crate::services::write_behind_queue::WriteBehindQueue>>,
    ) -> Self {
        Self {
            lead,
            publisher_id,
            vertical,
            request_type,
            cache,
            timing: None,
            metrics: None,
            write_behind_queue,
        }
    }

    pub fn with_timing_and_metrics(
        mut self,
        timing: Arc<AtomicAuctionTiming>,
        metrics: Arc<DiagnosticMetrics>,
    ) -> Self {
        self.timing = Some(timing);
        self.metrics = Some(metrics);
        self
    }

    pub async fn route(
        &self,
        pool: Arc<PgPool>,
        encryption_key: Arc<Vec<u8>>,
    ) -> Result<RoutingResult> {
        // Initialize timing and metrics (create locally if not provided)
        let local_timing = AtomicAuctionTiming::new();
        let local_metrics = Arc::new(DiagnosticMetrics::new());
        let timing = self
            .timing
            .as_ref()
            .map(|t| t.clone())
            .unwrap_or_else(|| Arc::new(local_timing));
        let metrics = self
            .metrics
            .as_ref()
            .map(|m| m.clone())
            .unwrap_or_else(|| local_metrics);

        // Find active ping tree for publisher and vertical with revshare info (CACHED - 6h TTL)
        // DEBUG: Detailed routing steps (only in debug mode to reduce overhead)
        tracing::debug!(
            lead_id = %self.lead.uuid,
            stage = "ping_tree_lookup_start",
            "Starting ping tree lookup"
        );
        let _ping_tree_lookup_start = std::time::Instant::now();
        let ping_tree_start = std::time::Instant::now();
        let cache_key = format!("pingtree:pub:{}:vert:{}", self.publisher_id, self.vertical);
        let cache_stats_before = self.cache.as_ref().map(|cache| cache.get_stats());
        let ping_tree_result = if let Some(cache) = &self.cache {
            cache
                .get_or_insert_with(&cache_key, 21600, || async {
                    PingTree::find_for_routing(pool.as_ref(), &self.publisher_id, &self.vertical)
                        .await
                        .map_err(|e| anyhow::anyhow!("Database error: {}", e))
                })
                .await?
        } else {
            PingTree::find_for_routing(pool.as_ref(), &self.publisher_id, &self.vertical).await?
        };
        let ping_tree_duration = ping_tree_start.elapsed().as_millis() as u64;
        let cache_stats_after = self.cache.as_ref().map(|cache| cache.get_stats());
        let was_cache_hit =
            if let (Some(before), Some(after)) = (cache_stats_before, cache_stats_after) {
                // If hits increased, it was a cache hit
                after.hits > before.hits
            } else {
                false
            };
        if was_cache_hit {
            metrics.record_cache_hit();
        } else if ping_tree_duration > 0 {
            metrics.record_cache_miss();
        }
        // DEBUG: Detailed timing (only in debug mode to reduce overhead)
        tracing::debug!(
            ping_tree_lookup_ms = ping_tree_duration,
            cache_hit = was_cache_hit,
            "Ping tree lookup completed"
        );

        let (ping_tree, _revshare_percentage, _revshare_flat_amount) = match ping_tree_result {
            Some((pt, revshare_pct, revshare_flat)) => {
                #[cfg(all(feature = "tracing", debug_assertions))]
                tracing::debug!(
                    lead_id = %self.lead.uuid,
                    ping_tree_id = %pt.id,
                    publisher_id = %self.publisher_id,
                    vertical = %self.vertical,
                    duration_ms = ping_tree_duration,
                    cache_hit = was_cache_hit,
                    "Ping tree lookup completed"
                );
                timing.record_pre_checks(ping_tree_duration);
                (pt, revshare_pct, revshare_flat)
            }
            None => {
                timing.record_pre_checks(ping_tree_duration);
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
                    per_buyer_timings: None,
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
                per_buyer_timings: None,
            });
        }

        // Get enabled campaigns from ping tree (CACHED - 6h TTL)
        // DEBUG: Detailed routing steps (only in debug mode)
        tracing::debug!(
            lead_id = %self.lead.uuid,
            stage = "campaigns_load_start",
            ping_tree_id = %ping_tree.id,
            "Starting campaigns loading"
        );
        let campaigns_start = std::time::Instant::now();
        let campaigns_cache_key = format!("campaigns:pingtree:{}", ping_tree.id);
        let cache_hit_before = if let Some(cache) = &self.cache {
            let stats = cache.get_stats();
            stats.hits + stats.misses
        } else {
            0
        };
        let ping_tree_campaigns = if let Some(cache) = &self.cache {
            cache
                .get_or_insert_with(&campaigns_cache_key, 21600, || async {
                    PingTreeCampaign::find_enabled_for_ping_tree(pool.as_ref(), &ping_tree.id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Database error: {}", e))
                })
                .await?
        } else {
            PingTreeCampaign::find_enabled_for_ping_tree(pool.as_ref(), &ping_tree.id).await?
        };
        let campaigns_duration = campaigns_start.elapsed().as_millis() as u64;
        let cache_hit_after = if let Some(cache) = &self.cache {
            let stats = cache.get_stats();
            stats.hits + stats.misses
        } else {
            0
        };
        let was_cache_hit = cache_hit_after > cache_hit_before;
        if was_cache_hit {
            metrics.record_cache_hit();
        } else if campaigns_duration > 0 {
            metrics.record_cache_miss();
        }
        // DETAILED TIMING: Log campaigns loading
        // DEBUG: Detailed timing (only in debug mode)
        tracing::debug!(
            campaigns_load_ms = campaigns_duration,
            cache_hit = was_cache_hit,
            ping_tree_campaign_count = ping_tree_campaigns.len(),
            "Campaigns loaded"
        );
        #[cfg(all(feature = "tracing", debug_assertions))]
        tracing::debug!(
            lead_id = %self.lead.uuid,
            ping_tree_id = %ping_tree.id,
            campaign_count = ping_tree_campaigns.len(),
            duration_ms = campaigns_duration,
            cache_hit = was_cache_hit,
            "Campaigns loaded"
        );
        timing.record_pre_checks(campaigns_duration);
        metrics.record_query(campaigns_duration);

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
                per_buyer_timings: None,
            });
        }

        // OPTIMIZED: Load campaigns with associations (CACHED) and qualification configs in PARALLEL
        // Both operations depend on campaign_ids but are independent of each other
        // DEBUG: Detailed routing steps (only in debug mode)
        tracing::debug!(
            lead_id = %self.lead.uuid,
            stage = "campaigns_associations_start",
            campaign_count = ping_tree_campaigns.len(),
            "Starting campaigns with associations and qualification configs loading (parallelized)"
        );
        let campaigns_load_start = std::time::Instant::now();
        let buyer_ids_extraction_start = std::time::Instant::now();
        let campaign_ids: Vec<Uuid> = ping_tree_campaigns
            .iter()
            .map(|ptc| ptc.campaign_id)
            .collect();

        // Extract buyer_ids from ping_tree_campaigns by querying campaigns first (lightweight query)
        // We need buyer_ids to parallelize qualification configs loading
        let buyer_ids_from_campaigns = if let Some(cache) = &self.cache {
            // Quick query to get buyer_ids (can be cached per campaign_id)
            let mut buyer_ids = std::collections::HashSet::new();
            for campaign_id in &campaign_ids {
                let cache_key = format!("campaign:buyer_id:{}", campaign_id);
                if let Ok(Some(buyer_id)) = cache
                    .get_or_insert_with(&cache_key, 3600, || async {
                        sqlx::query_scalar::<_, uuid::Uuid>(
                            "SELECT buyer_id FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
                        )
                        .bind(campaign_id)
                        .fetch_optional(pool.as_ref())
                        .await
                        .map_err(|e| anyhow::anyhow!("DB error: {}", e))
                    })
                    .await
                {
                    buyer_ids.insert(buyer_id);
                }
            }
            buyer_ids.into_iter().collect()
        } else {
            // Fallback: query buyer_ids directly
            sqlx::query_scalar::<_, uuid::Uuid>(
                "SELECT DISTINCT buyer_id FROM campaigns WHERE id = ANY($1) AND deleted_at IS NULL",
            )
            .bind(&campaign_ids)
            .fetch_all(pool.as_ref())
            .await
            .unwrap_or_default()
        };
        let buyer_ids_extraction_duration = buyer_ids_extraction_start.elapsed().as_millis() as u64;
        // DEBUG: Detailed timing (only in debug mode)
        tracing::debug!(
            buyer_ids_extraction_ms = buyer_ids_extraction_duration,
            buyer_count = buyer_ids_from_campaigns.len(),
            "Buyer IDs extracted from campaigns"
        );

        // PARALLELIZE: Load campaigns_with_associations (CACHED) and qualification configs simultaneously
        let parallel_load_start = std::time::Instant::now();
        let (campaigns_with_associations_result, qualification_configs_result) = tokio::join!(
            // Cache campaigns with associations (1h TTL - campaigns rarely change)
            async {
                if let Some(cache) = &self.cache {
                    let mut sorted_ids = campaign_ids.clone();
                    sorted_ids.sort();
                    let cache_key = format!(
                        "campaigns:associations:{}",
                        sorted_ids
                            .iter()
                            .map(|id| id.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    cache
                        .get_or_insert_with(&cache_key, 3600, || async {
                            Campaign::find_by_ids_with_associations(pool.as_ref(), &campaign_ids)
                                .await
                                .map_err(|e| anyhow::anyhow!("DB error: {}", e))
                        })
                        .await
                        .unwrap_or_default()
                } else {
                    Campaign::find_by_ids_with_associations(pool.as_ref(), &campaign_ids)
                        .await
                        .unwrap_or_default()
                }
            },
            // Load qualification configs (already cached, but parallelize the cache lookup)
            async {
                if !buyer_ids_from_campaigns.is_empty() {
                    let mut sorted_buyer_ids = buyer_ids_from_campaigns.clone();
                    sorted_buyer_ids.sort();
                    let qual_cache_key = format!(
                        "qual:buyers:{}",
                        sorted_buyer_ids
                            .iter()
                            .map(|id| id.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    if let Some(cache) = &self.cache {
                        cache
                            .get_or_insert_with(&qual_cache_key, 3600, || async {
                                BuyerQualificationConfig::find_by_buyer_ids(
                                    pool.as_ref(),
                                    &buyer_ids_from_campaigns,
                                )
                                .await
                                .map_err(|e| anyhow::anyhow!("DB error: {}", e))
                            })
                            .await
                            .unwrap_or_default()
                    } else {
                        BuyerQualificationConfig::find_by_buyer_ids(
                            pool.as_ref(),
                            &buyer_ids_from_campaigns,
                        )
                        .await
                        .unwrap_or_default()
                    }
                } else {
                    std::collections::HashMap::new()
                }
            }
        );

        let campaigns_with_associations = campaigns_with_associations_result;
        let qualification_configs = qualification_configs_result;
        let parallel_load_duration = parallel_load_start.elapsed().as_millis() as u64;
        let campaigns_load_duration = campaigns_load_start.elapsed().as_millis() as u64;
        metrics.record_query(campaigns_load_duration);
        timing.record_pre_checks(campaigns_load_duration);

        // DETAILED TIMING: Log parallel loading breakdown
        // DEBUG: Detailed timing (only in debug mode)
        tracing::debug!(
            campaigns_associations_load_ms = campaigns_load_duration,
            parallel_load_ms = parallel_load_duration,
            buyer_ids_extraction_ms = buyer_ids_extraction_duration,
            campaign_count = campaign_ids.len(),
            buyer_count = buyer_ids_from_campaigns.len(),
            "Campaigns with associations and qualification configs loaded in parallel"
        );

        // Store buyer/integration data to avoid redundant DB lookups in BuyerRouter (Phase 7.1 optimization)
        use crate::models::buyer_integration::BuyerIntegration;
        let mut buyer_integration_map: std::collections::HashMap<Uuid, Option<BuyerIntegration>> =
            std::collections::HashMap::new();

        // Extract campaigns and store buyer/integration data
        let mut all_campaigns: Vec<Campaign> = Vec::new();
        for (campaign, _buyer, integration) in &campaigns_with_associations {
            all_campaigns.push(campaign.clone());
            // Store integration for this campaign's buyer_id (we only need integration for routing)
            buyer_integration_map.insert(campaign.buyer_id, integration.clone());
        }

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

        let qual_preload_duration = campaigns_load_duration; // Same duration since parallelized
        metrics.record_query(campaigns_load_duration);
        timing.record_qualification(campaigns_load_duration);
        // DETAILED TIMING: Log qualification config loading (now parallelized with campaigns)
        // DEBUG: Detailed timing (only in debug mode)
        tracing::debug!(
            qual_configs_ms = campaigns_load_duration,
            qual_preload_total_ms = qual_preload_duration,
            buyer_count = buyer_ids_from_campaigns.len(),
            "Qualification configs loaded (parallelized with campaigns)"
        );
        // Note: qualification_configs are preloaded and ready for use in buyer router tasks

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
                per_buyer_timings: None,
            });
        }

        // Route based on request type
        match self.request_type.as_str() {
            "ping" => {
                self.route_ping_auction(
                    pool.clone(),
                    &campaigns,
                    &priority_map,
                    &buyer_integration_map,
                    &qualification_configs,
                    encryption_key.clone(),
                    timing.clone(),
                    metrics.clone(),
                )
                .await
            }
            "post" => {
                self.route_post(
                    pool.clone(),
                    &campaigns,
                    encryption_key.clone(),
                    Some(timing.clone()),
                    Some(metrics.clone()),
                )
                .await
            }
            "fullpost" => {
                self.route_fullpost(
                    pool.clone(),
                    &campaigns,
                    &ping_tree,
                    &priority_map,
                    &buyer_integration_map,
                    &qualification_configs,
                    encryption_key.clone(),
                    timing.clone(),
                    metrics.clone(),
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
                    per_buyer_timings: None,
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // Complex routing function legitimately needs all parameters
    async fn route_ping_auction(
        &self,
        pool: Arc<PgPool>,
        campaigns: &[Campaign],
        priority_map: &std::collections::HashMap<Uuid, Option<i32>>,
        buyer_integration_map: &std::collections::HashMap<
            Uuid,
            Option<crate::models::buyer_integration::BuyerIntegration>,
        >,
        qualification_configs: &std::collections::HashMap<Uuid, Option<BuyerQualificationConfig>>,
        encryption_key: Arc<Vec<u8>>,
        timing: Arc<AtomicAuctionTiming>,
        metrics: Arc<DiagnosticMetrics>,
    ) -> Result<RoutingResult> {
        use crate::services::buyer_router::BuyerRouter;
        use tokio::time::{timeout, Duration};
        // OPTIMIZED: Reduced timeout for internal buyers (Pulsar is sync and instant)
        // External buyers would need longer timeout, but we're only using Pulsar now
        const PING_AUCTION_TIMEOUT: Duration = Duration::from_millis(100); // 100ms is plenty for sync Pulsar calls

        // Start ping auction stage
        // VERIFICATION: Check if all buyers are internal (Pulsar) or if external HTTP calls are needed
        let internal_buyer_count = buyer_integration_map
            .values()
            .filter(|opt_int| opt_int.as_ref().map(|i| i.is_internal).unwrap_or(false))
            .count();
        let external_buyer_count = buyer_integration_map
            .values()
            .filter(|opt_int| opt_int.as_ref().map(|i| !i.is_internal).unwrap_or(false))
            .count();
        let unknown_buyer_count = buyer_integration_map
            .values()
            .filter(|opt_int| opt_int.is_none())
            .count();

        // DEBUG: Detailed routing steps (only in debug mode)
        // WARN: External buyers will be logged separately if present
        tracing::debug!(
            lead_id = %self.lead.uuid,
            stage = "ping_auction_start",
            campaign_count = campaigns.len(),
            request_type = %self.request_type,
            internal_buyers = internal_buyer_count,
            external_buyers = external_buyer_count,
            unknown_buyers = unknown_buyer_count,
            "Starting ping auction - buyer type breakdown"
        );

        // WARNING: If external buyers exist, HTTP calls will block the response
        if external_buyer_count > 0 {
            tracing::warn!(
                lead_id = %self.lead.uuid,
                external_buyer_count = external_buyer_count,
                "EXTERNAL BUYERS DETECTED - HTTP calls will add latency to response"
            );
        }

        let _ping_auction_start = std::time::Instant::now();

        // Send concurrent pings to all campaigns with semaphore to limit concurrency
        // This prevents Neon overload and ensures more consistent latency
        let semaphore = Arc::new(Semaphore::new(10)); // Limit to 10 concurrent pings
        let mut task_futures = Vec::new();
        for campaign in campaigns {
            let lead = self.lead.clone();
            let campaign_clone = campaign.clone();
            let request_type = self.request_type.clone();

            let pool_clone = pool.clone();
            let encryption_key_clone = encryption_key.clone();
            let campaign_id = campaign.id;
            let semaphore_clone = semaphore.clone();

            // Get pre-loaded integration for this campaign's buyer (Phase 7.1 optimization)
            let preloaded_integration = buyer_integration_map
                .get(&campaign_clone.buyer_id)
                .and_then(|integration| integration.clone());

            // Get pre-loaded qualification config for this campaign's buyer
            let preloaded_qual_config = qualification_configs
                .get(&campaign_clone.buyer_id)
                .and_then(|config| config.clone());

            // OPTIMIZED: Check if this buyer is internal (Pulsar) to optimize path
            let is_internal_buyer = preloaded_integration
                .as_ref()
                .map(|i| i.is_internal)
                .unwrap_or(false);

            // DEBUG: Detailed routing steps (only in debug mode)
            tracing::debug!(
                lead_id = %self.lead.uuid,
                campaign_id = %campaign_id,
                buyer_id = %campaign_clone.buyer_id,
                is_internal = is_internal_buyer,
                has_preloaded_integration = preloaded_integration.is_some(),
                has_preloaded_qual_config = preloaded_qual_config.is_some(),
                "Creating buyer task"
            );

            // Wrap each task with timeout and store campaign_id for result mapping
            // Track individual buyer processing time
            // OPTIMIZED: Skip semaphore and timeout for internal buyers (they're sync and instant)
            let task_future = async move {
                // Only acquire semaphore for external buyers (prevents connection pool exhaustion)
                let _permit = if !is_internal_buyer {
                    Some(semaphore_clone.acquire().await.unwrap())
                } else {
                    None // Skip semaphore for internal buyers (they're instant)
                };

                // Inject chaos delay if enabled (for testing)
                let chaos_delay_start = std::time::Instant::now();
                inject_chaos_delay().await;
                let chaos_delay_duration = chaos_delay_start.elapsed().as_millis() as u64;
                if chaos_delay_duration > 0 {
                    // DEBUG: Chaos mode logging (only in debug mode)
                    tracing::debug!(
                        campaign_id = %campaign_id,
                        chaos_delay_ms = chaos_delay_duration,
                        "Chaos delay injected"
                    );
                }

                let buyer_start = std::time::Instant::now();
                let router_creation_start = std::time::Instant::now();
                let router = BuyerRouter::new(
                    lead,
                    vec![campaign_clone],
                    request_type,
                    pool_clone,
                    encryption_key_clone,
                )
                .with_preloaded_integration(preloaded_integration)
                .with_preloaded_qual_config(preloaded_qual_config)
                .with_cache(self.cache.clone());
                let router_creation_duration = router_creation_start.elapsed().as_millis() as u64;

                // OPTIMIZED: For internal buyers (Pulsar), skip timeout wrapper (they're sync and instant)
                // Timeout only needed for external HTTP buyers
                let route_call_start = std::time::Instant::now();
                let result = if is_internal_buyer {
                    // Direct call for internal buyers (no timeout overhead)
                    // DEBUG: Detailed routing steps (only in debug mode)
                    tracing::debug!(
                        campaign_id = %campaign_id,
                        stage = "pulsar_direct_call_start",
                        "Calling Pulsar directly (no timeout)"
                    );
                    let pulsar_result = router.route().await;
                    let route_call_duration = route_call_start.elapsed().as_millis() as u64;
                    tracing::debug!(
                        campaign_id = %campaign_id,
                        stage = "pulsar_direct_call_complete",
                        pulsar_call_ms = route_call_duration,
                        router_creation_ms = router_creation_duration,
                        "Pulsar direct call completed"
                    );
                    Ok(pulsar_result)
                } else {
                    // Timeout wrapper for external buyers
                    // DEBUG: External buyer calls (only in debug mode)
                    tracing::debug!(
                        campaign_id = %campaign_id,
                        stage = "external_buyer_call_start",
                        timeout_ms = PING_AUCTION_TIMEOUT.as_millis(),
                        "Calling external buyer (with timeout)"
                    );
                    let external_result = timeout(PING_AUCTION_TIMEOUT, router.route())
                        .await
                        .map_err(|e| anyhow::anyhow!("Timeout: {}", e));
                    let route_call_duration = route_call_start.elapsed().as_millis() as u64;
                    tracing::debug!(
                        campaign_id = %campaign_id,
                        stage = "external_buyer_call_complete",
                        external_call_ms = route_call_duration,
                        router_creation_ms = router_creation_duration,
                        "External buyer call completed"
                    );
                    external_result
                };

                let processing_time_ms = buyer_start.elapsed().as_millis() as u64;
                // DEBUG: Detailed timing (only in debug mode)
                tracing::debug!(
                    campaign_id = %campaign_id,
                    total_processing_ms = processing_time_ms,
                    router_creation_ms = router_creation_duration,
                    route_call_ms = route_call_start.elapsed().as_millis() as u64,
                    "Buyer task completed"
                );
                (result, campaign_id, processing_time_ms)
            };
            task_futures.push(task_future);
        }

        // Wait for all responses concurrently using FuturesUnordered for better parallelism
        // DEBUG: Detailed routing steps (only in debug mode)
        tracing::debug!(
            lead_id = %self.lead.uuid,
            stage = "futures_unordered_start",
            task_count = task_futures.len(),
            "Starting FuturesUnordered collection"
        );
        let ping_auction_start = std::time::Instant::now();
        let mut futures_unordered = FuturesUnordered::new();
        for task_future in task_futures {
            futures_unordered.push(task_future);
        }
        let mut task_results: Vec<PingTaskResult> = Vec::new();
        let mut first_response_time: Option<u64> = None;
        while let Some(result) = futures_unordered.next().await {
            if first_response_time.is_none() {
                first_response_time = Some(ping_auction_start.elapsed().as_millis() as u64);
                // DEBUG: Detailed timing (only in debug mode)
                tracing::debug!(
                    lead_id = %self.lead.uuid,
                    first_response_ms = first_response_time.unwrap(),
                    "First buyer response received"
                );
            }
            task_results.push(result);
        }
        let ping_auction_duration = ping_auction_start.elapsed().as_millis() as u64;
        // DEBUG: Detailed timing (only in debug mode)
        tracing::debug!(
            lead_id = %self.lead.uuid,
            stage = "futures_unordered_complete",
            ping_auction_total_ms = ping_auction_duration,
            first_response_ms = first_response_time,
            response_count = task_results.len(),
            "All buyer responses collected"
        );
        metrics.record_ping_auction(ping_auction_duration); // Track ping auction duration
        metrics.record_stage_timing("ping_auction", ping_auction_duration);
        // DEBUG: Detailed timing (only in debug mode)
        tracing::debug!(
            ping_auction_ms = ping_auction_duration,
            campaign_count = campaigns.len(),
            response_count = task_results.len(),
            "Ping auction completed"
        );
        let mut responses: Vec<(BuyerResponse, Uuid, Option<i32>)> = Vec::new();
        let mut per_buyer_timings: Vec<serde_json::Value> = Vec::new();

        // DEBUG: Detailed routing steps (only in debug mode)
        tracing::debug!(
            lead_id = %self.lead.uuid,
            stage = "processing_responses_start",
            response_count = task_results.len(),
            "Starting response processing"
        );
        for (result, campaign_id, processing_time_ms) in task_results {
            match result {
                Ok(Ok(response)) => {
                    // Success case: timeout OK, router OK
                    // Moved to debug level to reduce tracing overhead in hot path
                    #[cfg(all(feature = "tracing", debug_assertions))]
                    tracing::debug!(
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
                    // Track per-buyer timing
                    if let Some(campaign) = campaigns.iter().find(|c| c.id == campaign_id) {
                        per_buyer_timings.push(serde_json::json!({
                            "campaign_id": campaign_id,
                            "buyer_id": campaign.buyer_id,
                            "processing_time_ms": processing_time_ms,
                            "status": response.status,
                            "bid": response.bid,
                            "success": response.success
                        }));
                    }
                    responses.push((response, campaign_id, priority));
                }
                Ok(Err(e)) => {
                    // Router error case: timeout OK, router error
                    #[cfg(feature = "tracing")]
                    tracing::error!("BuyerRouter error for campaign {}: {}", campaign_id, e);
                    // Track per-buyer timing for errors
                    if let Some(campaign) = campaigns.iter().find(|c| c.id == campaign_id) {
                        per_buyer_timings.push(serde_json::json!({
                            "campaign_id": campaign_id,
                            "buyer_id": campaign.buyer_id,
                            "processing_time_ms": processing_time_ms,
                            "status": "error",
                            "bid": null,
                            "success": false,
                            "error": e.to_string()
                        }));
                    }
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
                Err(_) => {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("Ping auction timeout for campaign {}", campaign_id);
                    // Track per-buyer timing for timeouts
                    if let Some(campaign) = campaigns.iter().find(|c| c.id == campaign_id) {
                        per_buyer_timings.push(serde_json::json!({
                            "campaign_id": campaign_id,
                            "buyer_id": campaign.buyer_id,
                            "processing_time_ms": processing_time_ms,
                            "status": "timeout",
                            "bid": null,
                            "success": false,
                            "error": "Buyer did not respond within timeout period"
                        }));
                    }
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

        #[cfg(all(feature = "tracing", debug_assertions))]
        tracing::debug!(
            lead_id = %self.lead.uuid,
            total_responses = responses.len(),
            accepted_count = responses.iter().filter(|(r, _, _)| r.status == "accepted").count(),
            rejected_count = responses.iter().filter(|(r, _, _)| r.status == "rejected").count(),
            timeout_count = responses.iter().filter(|(r, _, _)| r.status == "timeout").count(),
            error_count = responses.iter().filter(|(r, _, _)| r.status == "error").count(),
            "Ping auction completed - all responses received"
        );

        // Log performance metrics to Sentry for monitoring
        #[cfg(feature = "sentry")]
        {
            sentry::configure_scope(|scope| {
                scope.set_tag("ping_auction_responses", responses.len().to_string());
            });
        }

        // Batch persist buyer responses for audit (optimized with bulk INSERT)
        // Store plaintext JSON; API layer will encrypt rows when SSM keys are available.
        let mut batch_responses = Vec::new();
        for (resp, campaign_id, _pri) in &responses {
            // Find buyer_id from campaigns list
            let buyer_id_opt = campaigns
                .iter()
                .find(|c| c.id == *campaign_id)
                .map(|c| c.buyer_id);
            // Serialize response - use empty object on error (best-effort persistence)
            let resp_json = serde_json::to_value(resp).unwrap_or_else(|_| serde_json::json!({}));
            // Ensure ping_id is set - if response doesn't have one, generate one for ping requests
            let mut ping_id_val = resp.ping_id.clone().or_else(|| {
                if self.request_type == "ping" {
                    Some(format!("ping_{}", uuid::Uuid::new_v4()))
                } else {
                    None
                }
            });

            // Make ping_id unique per campaign (prevents duplicates when multiple campaigns ping same lead)
            // Ruby does this: append _C{campaign_id_first_8_chars} if not already present
            if let Some(ref mut ping_id) = ping_id_val {
                if !ping_id.contains("_C") {
                    // Extract first 8 characters of campaign_id (without dashes)
                    let campaign_suffix = campaign_id
                        .to_string()
                        .replace('-', "")
                        .chars()
                        .take(8)
                        .collect::<String>();
                    *ping_id = format!("{}_C{}", ping_id, campaign_suffix);
                }
            }

            batch_responses.push((
                self.lead.uuid,
                ping_id_val,
                None, // post_id is None for ping responses
                buyer_id_opt,
                *campaign_id,
                resp_json,
            ));
        }

        // Enqueue buyer responses to write-behind queue (non-blocking)
        // This removes ~2.8s sync DB write from critical path
        if !batch_responses.is_empty() {
            if let Some(queue) = &self.write_behind_queue {
                #[cfg(all(feature = "tracing", debug_assertions))]
                tracing::debug!(
                    lead_id = %self.lead.uuid,
                    response_count = batch_responses.len(),
                    "Enqueueing buyer responses to write-behind queue"
                );
                for (lead_id, ping_id, post_id, buyer_id, campaign_id, payload) in batch_responses {
                    queue.enqueue(
                        crate::services::write_behind_queue::BackgroundTask::BuyerResponse {
                            lead_id,
                            campaign_id,
                            ping_id,
                            post_id,
                            buyer_id,
                            payload,
                        },
                    );
                }
            } else {
                // Fallback: synchronous insert if queue unavailable (shouldn't happen in production)
                #[cfg(feature = "tracing")]
                tracing::warn!(
                    lead_id = %self.lead.uuid,
                    "Write-behind queue unavailable, falling back to synchronous buyer response insert"
                );
                let result =
                    PingTreeRouter::batch_insert_buyer_responses(pool.as_ref(), batch_responses)
                        .await;
                if let Err(_e) = &result {
                    #[cfg(feature = "tracing")]
                    tracing::error!("Failed to batch insert buyer responses: {}", _e);
                    #[cfg(feature = "sentry")]
                    {
                        sentry::capture_message(
                            &format!("Batch insert buyer responses failed: {}", _e),
                            sentry::Level::Error,
                        );
                    }
                }
            }
        }

        // Record ping auction timing
        timing.record_ping_auction(ping_auction_duration);

        // Filter valid responses
        // For ping requests: success=true, bid > 0, status != timeout, promise_id required
        // For post requests: success=true, price > 0, status != timeout
        let valid_responses: Vec<_> = responses
            .iter()
            .filter(|(resp, _campaign_id, _)| {
                // For ping auctions, check for bid (not price)
                let has_bid = resp.bid.is_some() && resp.bid.unwrap_or(0.0) > 0.0;
                let has_promise_id = resp.promise_id.is_some();
                let is_valid =
                    resp.success && has_bid && has_promise_id && resp.status != "timeout";

                if !is_valid {
                    #[cfg(feature = "tracing")]
                    let _reason = if !resp.success {
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

                    #[cfg(feature = "tracing")]
                    tracing::warn!(
                        lead_id = %self.lead.uuid,
                        campaign_id = %_campaign_id,
                        invalid_reason = %_reason,
                        success = resp.success,
                        bid = ?resp.bid,
                        promise_id = ?resp.promise_id,
                        status = %resp.status,
                        error = ?resp.error,
                        ping_id = ?resp.ping_id,
                        "Invalid buyer ping response - response does not meet validation criteria"
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

            #[cfg(feature = "tracing")]
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
                per_buyer_timings: if per_buyer_timings.is_empty() {
                    None
                } else {
                    Some(per_buyer_timings)
                },
            });
        }

        // Select winner: highest bid, then priority, then random
        // DEBUG: Detailed routing steps (only in debug mode)
        tracing::debug!(
            lead_id = %self.lead.uuid,
            stage = "select_winner_start",
            total_responses = responses.len(),
            valid_responses = valid_responses.len(),
            "Selecting winner from valid responses"
        );
        let winner_selection_start = std::time::Instant::now();
        let winner = select_winner(valid_responses);
        let (winner_response, winner_campaign_id, _) = winner;
        let winner_selection_duration = winner_selection_start.elapsed().as_millis() as u64;
        // DEBUG: Detailed routing steps (only in debug mode)
        tracing::debug!(
            lead_id = %self.lead.uuid,
            stage = "select_winner_complete",
            winner_selection_ms = winner_selection_duration,
            winner_campaign_id = %winner_campaign_id,
            winner_bid = ?winner_response.bid,
            winner_status = %winner_response.status,
            "Winner selected"
        );
        // Timing is tracked atomically (winner selection is part of ping_auction)

        // Find winning campaign
        let find_winner_campaign_start = std::time::Instant::now();
        let winner_campaign = campaigns
            .iter()
            .find(|c| c.id == winner_campaign_id)
            .ok_or_else(|| anyhow::anyhow!("Winner campaign not found"))?;
        let find_winner_campaign_duration = find_winner_campaign_start.elapsed().as_millis() as u64;
        // DEBUG: Detailed timing (only in debug mode)
        tracing::debug!(
            lead_id = %self.lead.uuid,
            find_winner_campaign_ms = find_winner_campaign_duration,
            "Winner campaign found"
        );

        // Enqueue lead update with winner to background queue (non-blocking)
        // This removes 120-554ms sync DB write from critical path
        if let Some(queue) = &self.write_behind_queue {
            queue.enqueue(
                crate::services::write_behind_queue::BackgroundTask::LeadUpdate {
                    lead_id: self.lead.uuid,
                    status: crate::models::enums::LeadStatus::PingAccepted,
                    campaign_id: Some(winner_campaign.id),
                    buyer_id: Some(winner_campaign.buyer_id),
                    promise_id: winner_response.promise_id.clone(),
                    ping_id: winner_response.ping_id.clone(),
                    post_id: None,
                    sold_at: false,
                    inprog_token: None,
                    vertical_data: None, // Auction timing stored separately after routing completes
                },
            );
        } else {
            // Fallback: synchronous update if queue unavailable (shouldn't happen in production)
            self.update_lead_with_winner(
                pool.as_ref(),
                winner_campaign,
                winner_response.promise_id.as_deref().unwrap_or(""),
                winner_response.ping_id.as_deref(),
                winner_response.price,
            )
            .await?;
        }

        let total_route_ping_auction_duration = _ping_auction_start.elapsed().as_millis() as u64;
        // DEBUG: Detailed timing (only in debug mode)
        tracing::debug!(
            lead_id = %self.lead.uuid,
            stage = "route_ping_auction_complete",
            total_ping_auction_ms = total_route_ping_auction_duration,
            ping_auction_collection_ms = ping_auction_duration,
            winner_selection_ms = winner_selection_duration,
            "Ping auction routing completed (lead update enqueued to background)"
        );

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
            per_buyer_timings: if per_buyer_timings.is_empty() {
                None
            } else {
                Some(per_buyer_timings)
            },
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
        timing: Option<Arc<AtomicAuctionTiming>>,
        metrics: Option<Arc<DiagnosticMetrics>>,
    ) -> Result<RoutingResult> {
        use crate::services::buyer_router::BuyerRouter;

        #[allow(unused_variables)]
        let start_time = std::time::Instant::now();

        // Validate that lead.campaign_id exists in the provided campaigns (from ping tree)
        if let Some(campaign_id) = self.lead.campaign_id {
            if !campaigns.iter().any(|c| c.id == campaign_id) {
                // Log performance metrics to Sentry for monitoring (even on error)

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
                    per_buyer_timings: None,
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
            )
            .with_cache(self.cache.clone());

            let post_start = std::time::Instant::now();
            let post_result = buyer_router.route().await;
            let post_duration = post_start.elapsed().as_millis() as u64;
            if let Some(ref m) = metrics {
                m.record_post(post_duration);
                m.record_stage_timing("post_sent", post_duration);
            }

            // Record post timing
            if let Some(ref t) = timing {
                t.record_post_sent(post_duration);
            }

            match post_result {
                Ok(bresp) => {
                    // Persist buyer response for this post attempt (with retry logic)
                    let bresp_json =
                        serde_json::to_value(&bresp).unwrap_or_else(|_| serde_json::json!({}));
                    // Persist via write-behind queue (eliminates spawn overhead)
                    if let Some(queue) = &self.write_behind_queue {
                        queue.enqueue(
                            crate::services::write_behind_queue::BackgroundTask::BuyerResponse {
                                lead_id: self.lead.uuid,
                                campaign_id: campaign.id,
                                ping_id: None,
                                post_id: bresp.post_id.clone(),
                                buyer_id: Some(campaign.buyer_id),
                                payload: bresp_json,
                            },
                        );
                    } else {
                        // Fallback: log warning if queue not available (shouldn't happen in production)
                        #[cfg(feature = "tracing")]
                        tracing::warn!(
                            lead_id = %self.lead.uuid,
                            campaign_id = %campaign.id,
                            "Write-behind queue unavailable, skipping buyer response persistence (best-effort)"
                        );
                    }

                    // Validate post response: must have success=true, post_id, and price > 0
                    if bresp.success
                        && bresp.post_id.is_some()
                        && bresp.price.is_some()
                        && bresp.price.unwrap_or(0.0) > 0.0
                    {
                        // Log performance metrics to Sentry for monitoring

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
                            per_buyer_timings: None,
                        })
                    } else {
                        // Log performance metrics to Sentry for monitoring (even on rejection)

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
                            per_buyer_timings: None,
                        })
                    }
                }
                Err(e) => {
                    #[cfg(feature = "tracing")]
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
                        per_buyer_timings: None,
                    })
                }
            }
        } else {
            // Log performance metrics to Sentry for monitoring (even on error)

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
                per_buyer_timings: None,
            })
        }
    }

    #[allow(clippy::too_many_arguments)] // Complex routing function legitimately needs all parameters
    async fn route_fullpost(
        &self,
        pool: Arc<PgPool>,
        campaigns: &[Campaign],
        ping_tree: &PingTree,
        priority_map: &std::collections::HashMap<Uuid, Option<i32>>,
        buyer_integration_map: &std::collections::HashMap<
            Uuid,
            Option<crate::models::buyer_integration::BuyerIntegration>,
        >,
        qualification_configs: &std::collections::HashMap<Uuid, Option<BuyerQualificationConfig>>,
        encryption_key: Arc<Vec<u8>>,
        timing: Arc<AtomicAuctionTiming>,
        metrics: Arc<DiagnosticMetrics>,
    ) -> Result<RoutingResult> {
        // If ping tree strategy is ping_post, split fullpost into ping/post
        if ping_tree.strategy == "ping_post" {
            #[cfg(feature = "tracing")]
            // DEBUG: Detailed routing steps (only in debug mode)
            tracing::debug!(
                lead_id = %self.lead.uuid,
                strategy = %ping_tree.strategy,
                campaign_count = campaigns.len(),
                "Starting fullpost routing with ping_post strategy"
            );

            // Create a temporary router with "ping" request_type for the ping auction phase
            let mut ping_router = PingTreeRouter::new(
                self.lead.clone(),
                self.publisher_id,
                self.vertical.clone(),
                "ping".to_string(), // Force "ping" request_type for ping auction
                self.cache.clone(),
                self.write_behind_queue.clone(),
            );
            // Connect timing and metrics to the ping router
            ping_router = ping_router.with_timing_and_metrics(timing.clone(), metrics.clone());

            let ping_result = ping_router
                .route_ping_auction(
                    pool.clone(),
                    campaigns,
                    priority_map,
                    buyer_integration_map,
                    qualification_configs,
                    encryption_key.clone(),
                    timing.clone(),
                    metrics.clone(),
                )
                .await?;
            if !ping_result.success {
                // Log performance metrics to Sentry for monitoring (even on early return)
                return Ok(ping_result);
            }

            // Construct updated lead from ping_result instead of reloading from DB
            // This eliminates the 421ms DB query overhead
            let mut updated_lead = self.lead.clone();
            // Update promise_id and campaign_id from ping_result
            if let Some(promise_id) = &ping_result.promise_id {
                updated_lead.promise_id = Some(promise_id.clone());
            }
            if let Some(campaign_id) = ping_result.campaign_id {
                updated_lead.campaign_id = Some(campaign_id);
            }
            #[cfg(feature = "tracing")]
            // DEBUG: Detailed routing steps (only in debug mode)
            tracing::debug!(
                lead_id = %self.lead.uuid,
                promise_id = ?updated_lead.promise_id,
                campaign_id = ?updated_lead.campaign_id,
                "Updated lead in-memory from ping_result (skipped DB reload)"
            );

            // Create new router with updated lead for post routing
            let post_router = PingTreeRouter::new(
                updated_lead,
                self.publisher_id,
                self.vertical.clone(),
                "post".to_string(),
                self.cache.clone(),
                self.write_behind_queue.clone(),
            );

            // Route post using the updated lead (which now has promise_id and campaign_id)
            let post_result = post_router
                .route_post(
                    pool.clone(),
                    campaigns,
                    encryption_key.clone(),
                    Some(timing.clone()),
                    Some(metrics.clone()),
                )
                .await?;

            // Log performance metrics to Sentry for monitoring

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
                per_buyer_timings: ping_result
                    .per_buyer_timings
                    .or(post_result.per_buyer_timings),
            })
        } else {
            // Fullpost strategy: send single request with all lead data to all buyers
            // Similar to route_post but sends to all campaigns and selects winner by price
            use crate::services::buyer_router::BuyerRouter;

            // Send fullpost request to all campaigns in parallel (using join_all instead of spawn)
            let mut futures = Vec::new();
            let mut campaign_ids = Vec::new();
            for campaign in campaigns {
                let pool_clone = pool.clone();
                let encryption_key_clone = encryption_key.clone();
                let lead_clone = self.lead.clone();
                let campaign_clone = campaign.clone();
                let request_type = "fullpost".to_string();
                let campaign_id = campaign.id;
                let cache_clone = self.cache.clone();

                campaign_ids.push(campaign_id);
                futures.push(async move {
                    let buyer_router = BuyerRouter::new(
                        lead_clone,
                        vec![campaign_clone.clone()],
                        request_type,
                        pool_clone,
                        encryption_key_clone,
                    )
                    .with_cache(cache_clone);
                    buyer_router.route().await
                });
            }

            // Use FuturesUnordered for true concurrent handling (better than join_all for parallelism)
            let mut futures_unordered = FuturesUnordered::new();
            for future in futures {
                futures_unordered.push(future);
            }
            let mut results = Vec::new();
            while let Some(result) = futures_unordered.next().await {
                results.push(result);
            }

            // Collect responses
            let mut responses: Vec<(
                crate::services::buyer_router::BuyerResponse,
                Uuid,
                Option<i32>,
            )> = Vec::new();
            for (campaign_id, result) in campaign_ids.into_iter().zip(results.into_iter()) {
                match result {
                    Ok(bresp) => {
                        let priority = priority_map.get(&campaign_id).copied().flatten();
                        responses.push((bresp, campaign_id, priority));
                    }
                    Err(_e) => {
                        #[cfg(feature = "tracing")]
                        tracing::warn!("Buyer router error for campaign {}: {}", campaign_id, _e);
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

                #[cfg(feature = "tracing")]
                let _error_count = responses
                    .iter()
                    .filter(|(r, _, _)| r.status == "error")
                    .count();
                #[cfg(feature = "tracing")]
                let _invalid_details: Vec<serde_json::Value> = responses
                    .iter()
                    .filter(|(resp, _, _)| {
                        !resp.success
                            || resp.price.is_none()
                            || resp.price.unwrap_or(0.0) <= 0.0
                            || resp.status == "timeout"
                    })
                    .map(|(resp, campaign_id, _)| {
                        serde_json::json!({
                            "campaign_id": campaign_id,
                            "success": resp.success,
                            "price": resp.price,
                            "status": resp.status,
                            "error": resp.error,
                            "post_id": resp.post_id
                        })
                    })
                    .collect();

                #[cfg(feature = "tracing")]
                tracing::warn!(
                    lead_id = %self.lead.uuid,
                    total_responses = responses.len(),
                    timeout_count = timeout_count,
                    rejected_count = rejected_count,
                    error_count = _error_count,
                    invalid_responses = %simd_json::to_string(&_invalid_details).unwrap_or_else(|_| serde_json::to_string(&_invalid_details).unwrap_or_default()),
                    "No valid buyer responses for fullpost - all responses were invalid"
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
                    per_buyer_timings: None,
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

            // Update lead in database (inject chaos delay if enabled)
            inject_chaos_delay().await;
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

            // Batch persist buyer responses asynchronously (optimized with bulk INSERT)
            let mut batch_responses = Vec::new();
            for (resp, campaign_id, _) in responses {
                let resp_json =
                    serde_json::to_value(&resp).unwrap_or_else(|_| serde_json::json!({}));
                let buyer_id_opt = campaigns
                    .iter()
                    .find(|c| c.id == campaign_id)
                    .map(|c| c.buyer_id);

                batch_responses.push((
                    self.lead.uuid,
                    None, // ping_id is None for post responses
                    resp.post_id.clone(),
                    buyer_id_opt,
                    campaign_id,
                    resp_json,
                ));
            }

            // Enqueue all responses to write-behind queue (will be batched automatically)
            if !batch_responses.is_empty() {
                if let Some(queue) = &self.write_behind_queue {
                    for (lead_id, ping_id, post_id, buyer_id, campaign_id, payload) in
                        batch_responses
                    {
                        queue.enqueue(
                            crate::services::write_behind_queue::BackgroundTask::BuyerResponse {
                                lead_id,
                                campaign_id,
                                ping_id,
                                post_id,
                                buyer_id,
                                payload,
                            },
                        );
                    }
                } else {
                    // Fallback: log warning if queue not available (shouldn't happen in production)
                    #[cfg(feature = "tracing")]
                    tracing::warn!(
                        lead_id = %self.lead.uuid,
                        batch_size = batch_responses.len(),
                        "Write-behind queue unavailable, skipping batch buyer response persistence (best-effort)"
                    );
                }
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
                per_buyer_timings: None,
            })
        }
    }

    /// Persist buyer response with retry logic for transient database errors
    /// Batch insert buyer responses using UNNEST for better performance
    async fn batch_insert_buyer_responses(
        pool: &PgPool,
        responses: Vec<BuyerResponseRow>,
    ) -> anyhow::Result<()> {
        if responses.is_empty() {
            return Ok(());
        }

        #[cfg(feature = "tracing")]
        // DEBUG: Detailed DB operations (only in debug mode)
        tracing::debug!(
            response_count = responses.len(),
            "Executing batch insert for buyer responses"
        );

        // Validate no duplicate ping_ids before batch insert
        let ping_ids: Vec<Option<String>> = responses.iter().map(|r| r.1.clone()).collect();
        let unique_ping_ids: HashSet<Option<String>> = ping_ids.iter().cloned().collect();
        if ping_ids.len() != unique_ping_ids.len() {
            #[cfg(feature = "tracing")]
            tracing::error!("Duplicate ping_ids detected - aborting insert");
            return Err(anyhow::anyhow!("Duplicate ping_ids in responses"));
        }

        let lead_ids: Vec<Uuid> = responses.iter().map(|r| r.0).collect();
        let post_ids: Vec<Option<String>> = responses.iter().map(|r| r.2.clone()).collect();
        let buyer_ids: Vec<Option<Uuid>> = responses.iter().map(|r| r.3).collect();
        let campaign_ids: Vec<Uuid> = responses.iter().map(|r| r.4).collect();
        let payloads: Vec<serde_json::Value> = responses.iter().map(|r| r.5.clone()).collect();
        let created_ats: Vec<chrono::DateTime<chrono::Utc>> =
            (0..responses.len()).map(|_| chrono::Utc::now()).collect();

        // Convert payloads to Json type for binding
        let json_payloads: Vec<sqlx::types::Json<serde_json::Value>> = payloads
            .iter()
            .map(|p| sqlx::types::Json(p.clone()))
            .collect();

        // Inject chaos delay if enabled (for testing)
        inject_chaos_delay().await;

        #[cfg(feature = "tracing")]
        let db_query_start = std::time::Instant::now();

        // Use retry with exponential backoff for transient DB errors
        // Retry strategy: 100ms initial delay, max 3 attempts, exponential backoff with max 1000ms
        let retry_strategy = ExponentialBackoff::from_millis(100)
            .max_delay(Duration::from_millis(1000))
            .take(3);

        // Helper function to check if error is retryable (transient DB errors)
        fn is_retryable_error(e: &sqlx::Error) -> bool {
            matches!(
                e,
                sqlx::Error::PoolTimedOut | sqlx::Error::Io(_) | sqlx::Error::Tls(_)
            ) || e.to_string().contains("connection")
                || e.to_string().contains("timeout")
                || e.to_string().contains("pool")
        }

        // Clone variables for the closure
        let lead_ids_clone = lead_ids.clone();
        let ping_ids_clone = ping_ids.clone();
        let post_ids_clone = post_ids.clone();
        let buyer_ids_clone = buyer_ids.clone();
        let campaign_ids_clone = campaign_ids.clone();
        let json_payloads_clone = json_payloads.clone();
        let created_ats_clone = created_ats.clone();
        let pool_clone = pool.clone();

        let query_result = Retry::spawn(retry_strategy, move || {
            let lead_ids = lead_ids_clone.clone();
            let ping_ids = ping_ids_clone.clone();
            let post_ids = post_ids_clone.clone();
            let buyer_ids = buyer_ids_clone.clone();
            let campaign_ids = campaign_ids_clone.clone();
            let json_payloads = json_payloads_clone.clone();
            let created_ats = created_ats_clone.clone();
            let pool = pool_clone.clone();

            async move {
                let result = sqlx::query(
                    r#"
                    INSERT INTO buyer_responses (lead_id, ping_id, post_id, buyer_id, campaign_id, payload, created_at)
                    SELECT * FROM UNNEST($1::uuid[], $2::text[], $3::text[], $4::uuid[], $5::uuid[], $6::jsonb[], $7::timestamptz[])
                    "#,
                )
                .bind(&lead_ids[..])
                .bind(&ping_ids[..])
                .bind(&post_ids[..])
                .bind(&buyer_ids[..])
                .bind(&campaign_ids[..])
                .bind(&json_payloads[..])
                .bind(&created_ats[..])
                .execute(&pool)
                .await;

                match result {
                    Ok(r) => Ok(r),
                    Err(e) => {
                        if is_retryable_error(&e) {
                            #[cfg(feature = "tracing")]
                            tracing::warn!(
                                error = %e,
                                "Transient DB error, will retry"
                            );
                            Err(e)
                        } else {
                            // Non-retryable error (e.g., constraint violation, syntax error)
                            #[cfg(feature = "tracing")]
                            tracing::error!(
                                error = %e,
                                "Non-retryable DB error, aborting"
                            );
                            Err(e)
                        }
                    }
                }
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("DB error after retries: {}", e));

        match &query_result {
            Ok(_result) => {
                #[cfg(feature = "tracing")]
                {
                    let duration_ms = db_query_start.elapsed().as_millis() as u64;
                    // DEBUG: Detailed DB operations (only in debug mode)
                    tracing::debug!(
                        operation = "db_query",
                        query_type = "insert",
                        table = "buyer_responses",
                        query = "INSERT INTO buyer_responses ... UNNEST",
                        rows_affected = _result.rows_affected(),
                        batch_size = responses.len(),
                        duration_ms = duration_ms,
                        "Database query: batch insert buyer responses"
                    );
                }
            }
            Err(_e) => {
                #[cfg(feature = "tracing")]
                {
                    let duration_ms = db_query_start.elapsed().as_millis() as u64;
                    tracing::error!(
                    operation = "db_query",
                    query_type = "insert",
                    table = "buyer_responses",
                    query = "INSERT INTO buyer_responses ... UNNEST",
                    batch_size = responses.len(),
                    duration_ms = duration_ms,
                    error = %_e,
                    error_type = %std::any::type_name_of_val(&_e),
                    "Database query error: batch insert buyer responses failed"
                    );
                }
            }
        }

        query_result?;

        #[cfg(feature = "tracing")]
        // DEBUG: Detailed DB operations (only in debug mode)
        tracing::debug!(
            response_count = responses.len(),
            "Batch insert completed successfully"
        );

        Ok(())
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

    // Get highest bid
    let highest_bid = sorted[0].0.bid.unwrap_or(0.0);

    // Find all candidates with highest bid (within epsilon tolerance)
    let candidates: Vec<_> = sorted
        .iter()
        .take_while(|(resp, _, _)| (resp.bid.unwrap_or(0.0) - highest_bid).abs() < PRICE_EPSILON)
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

    // Random selection from candidates with same bid
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
