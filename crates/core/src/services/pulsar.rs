// Pulsar qualification engine service
// Provides direct function calls for internal Pulsar buyers (bypasses HTTP overhead)
// This matches the Ruby implementation's direct method calls

use crate::models::buyer_qualification_config::BuyerQualificationConfig;
use crate::models::campaign::Campaign;
use crate::models::lead::Lead;
use crate::services::buyer_router::BuyerResponse;
use crate::services::qualification_engine::{QualificationEngine, QualificationResult};
use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use base64::Engine;
use chrono::Utc;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

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

fn inject_chaos_delay_sync() {
    if should_inject_chaos() {
        let delay_ms = rand::random::<u64>() % 150 + 50; // 50-200ms
        std::thread::sleep(Duration::from_millis(delay_ms));
    }
}

#[inline(always)]
fn generate_promise_id() -> String {
    let hex_bytes = rand::random::<[u8; 6]>();
    let hex_str = hex::encode(hex_bytes);
    let mut result = String::with_capacity(8 + hex_str.len());
    result.push_str("PROMISE_");
    result.push_str(&hex_str.to_uppercase());
    result
}

/// Generate a unique ping_id per request (millisecond + 6 random hex bytes to avoid collisions).
#[inline(always)]
fn generate_ping_id(lead_id: &str) -> String {
    let ts_ms = Utc::now().timestamp_millis();
    let nonce = hex::encode(rand::random::<[u8; 6]>()).to_uppercase();
    let payload = format!("{}|{}|{}|accepted", lead_id, ts_ms, nonce);
    let encoded = BASE64_STD.encode(payload);
    format!("FP_{}", encoded)
}

#[inline(always)]
fn generate_rejected_ping_id(lead_id: &str) -> String {
    let ts_ms = Utc::now().timestamp_millis();
    let nonce = hex::encode(rand::random::<[u8; 6]>()).to_uppercase();
    let payload = format!("{}|{}|{}|rejected", lead_id, ts_ms, nonce);
    format!("FP_{}", BASE64_STD.encode(payload))
}

#[inline(always)]
fn generate_post_id(lead_id: &str) -> String {
    let timestamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
    let mut payload = String::with_capacity(lead_id.len() + timestamp.len() + 6);
    payload.push_str(lead_id);
    payload.push('|');
    payload.push_str(&timestamp);
    payload.push_str("|sold");
    let encoded = BASE64_STD.encode(payload);
    let mut post_id = String::with_capacity(3 + encoded.len());
    post_id.push_str("RP_");
    post_id.push_str(&encoded);
    post_id
}

pub struct PulsarService;

impl PulsarService {
    /// Direct call to Pulsar for ping requests (bypasses HTTP)
    pub async fn route_ping_direct(
        _pool: Arc<PgPool>,
        lead: &Lead,
        _campaign: &Campaign,
        qualification_config: Option<BuyerQualificationConfig>,
    ) -> Result<BuyerResponse> {
        let lead_id = lead
            .lead_id
            .clone()
            .unwrap_or_else(|| lead.uuid.to_string());

        // Unique ping_id per request: FP_<base64(lead_id|ts_ms|nonce|accepted)>
        let ping_id = generate_ping_id(&lead_id);
        let promise_id = format!(
            "PROMISE_{}",
            hex::encode(rand::random::<[u8; 6]>()).to_uppercase()
        );

        // Inject chaos delay if enabled (for testing)
        inject_chaos_delay().await;

        // Evaluate qualification rules using qualification engine
        let engine =
            QualificationEngine::new(lead.clone(), "ping".to_string(), qualification_config);
        let qual_result: QualificationResult = engine.evaluate();
        let accepted = qual_result.accepted;

        if !accepted {
            let rejected_ping_id = generate_rejected_ping_id(&lead_id);
            return Ok(BuyerResponse {
                success: false,
                status: "rejected".to_string(),
                ping_id: Some(rejected_ping_id),
                post_id: None,
                promise_id: None,
                price: None,
                bid: None,
                error: Some("Lead rejected by qualification rules".to_string()),
                message: Some("Lead did not meet qualification requirements".to_string()),
            });
        }

        // Logging removed - non-critical and spawn overhead eliminated
        // If logging is needed, it should be done via write-behind queue at caller level

        Ok(BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            ping_id: Some(ping_id),
            post_id: None,
            promise_id: Some(promise_id),
            price: None, // Ping responses don't have price
            bid: Some((rand::random::<u32>() % 200 + 100) as f64), // Ping responses have bid
            error: None,
            message: Some("Lead accepted for ping".to_string()),
        })
    }

    /// Direct call to Pulsar for post requests (bypasses HTTP)
    pub async fn route_post_direct(
        _pool: Arc<PgPool>,
        lead: &Lead,
        _campaign: &Campaign,
        promise_id: &str,
        qualification_config: Option<BuyerQualificationConfig>,
    ) -> Result<BuyerResponse> {
        let lead_id = lead
            .lead_id
            .clone()
            .unwrap_or_else(|| lead.uuid.to_string());

        // Duplicate promise_id check removed for performance optimization
        // The check was best-effort anyway (50ms timeout) and idempotency is handled elsewhere
        // Removing it eliminates blocking overhead and reduces post latency

        // Inject chaos delay if enabled (for testing)
        inject_chaos_delay().await;

        // Evaluate qualification rules using qualification engine
        let engine =
            QualificationEngine::new(lead.clone(), "post".to_string(), qualification_config);
        let qual_result: QualificationResult = engine.evaluate();
        let accepted = qual_result.accepted;

        if !accepted {
            return Ok(BuyerResponse {
                success: false,
                status: "rejected".to_string(),
                ping_id: None,
                post_id: None,
                promise_id: Some(promise_id.to_string()),
                price: None,
                bid: None,
                error: Some("Lead rejected by qualification rules".to_string()),
                message: Some("Lead did not meet qualification requirements".to_string()),
            });
        }

        // Generate post id similar to Ruby: RP_<base64(lead_id|timestamp|sold)>
        // Optimize string operations with pre-allocated capacity
        let timestamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
        let mut payload = String::with_capacity(lead_id.len() + timestamp.len() + 6);
        payload.push_str(&lead_id);
        payload.push('|');
        payload.push_str(&timestamp);
        payload.push_str("|sold");
        let encoded = BASE64_STD.encode(payload);
        let mut post_id = String::with_capacity(3 + encoded.len());
        post_id.push_str("RP_");
        post_id.push_str(&encoded);

        // Logging removed - non-critical and spawn overhead eliminated
        // If logging is needed, it should be done via write-behind queue at caller level

        Ok(BuyerResponse {
            success: true,
            status: "sold".to_string(),
            ping_id: None,
            post_id: Some(post_id),
            promise_id: Some(promise_id.to_string()),
            price: Some((rand::random::<u32>() % 200 + 100) as f64),
            bid: None, // Post responses don't have bid
            error: None,
            message: Some("Lead accepted and sold".to_string()),
        })
    }

    /// Direct call to Pulsar for fullpost requests (bypasses HTTP)
    /// Fullpost = ping + post in one call
    pub async fn route_fullpost_direct(
        pool: Arc<PgPool>,
        lead: &Lead,
        campaign: &Campaign,
        qualification_config: Option<BuyerQualificationConfig>,
    ) -> Result<BuyerResponse> {
        // First do ping
        let ping_response =
            Self::route_ping_direct(pool.clone(), lead, campaign, qualification_config.clone())
                .await?;

        // If ping fails, return early
        if !ping_response.success {
            return Ok(ping_response);
        }

        // Extract promise_id from ping response
        let promise_id = ping_response
            .promise_id
            .ok_or_else(|| anyhow::anyhow!("Ping succeeded but no promise_id returned"))?;

        // Then do post
        let post_response =
            Self::route_post_direct(pool, lead, campaign, &promise_id, qualification_config)
                .await?;

        // Return post response (which includes both ping_id and post_id)
        Ok(BuyerResponse {
            success: post_response.success,
            status: post_response.status,
            ping_id: ping_response.ping_id, // Include ping_id from ping phase
            post_id: post_response.post_id,
            promise_id: post_response.promise_id,
            price: post_response.price,
            bid: None, // Fullpost responses have price, not bid
            error: post_response.error,
            message: post_response.message,
        })
    }

    /// SYNC version: Direct call to Pulsar for fullpost requests (bypasses HTTP, no async overhead)
    /// Fullpost = ping + post in one call (both sync)
    #[inline(always)]
    pub fn route_fullpost_direct_sync(
        lead: &Lead,
        campaign: &Campaign,
        qualification_config: Option<BuyerQualificationConfig>,
    ) -> Result<BuyerResponse> {
        #[cfg(all(feature = "tracing", debug_assertions))]
        let fullpost_start = std::time::Instant::now();

        // First do ping (sync)
        #[cfg(all(feature = "tracing", debug_assertions))]
        let ping_start = std::time::Instant::now();
        let ping_response =
            Self::route_ping_direct_sync(lead, campaign, qualification_config.clone())?;
        #[cfg(all(feature = "tracing", debug_assertions))]
        {
            let ping_duration = ping_start.elapsed().as_millis() as u64;
            tracing::debug!(
                "route_fullpost_direct_sync: ping phase took {}ms",
                ping_duration
            );
        }

        // If ping fails, return early
        if !ping_response.success {
            #[cfg(all(feature = "tracing", debug_assertions))]
            {
                let total_duration = fullpost_start.elapsed().as_millis() as u64;
                tracing::debug!(
                    "route_fullpost_direct_sync: rejected in ping phase, total {}ms",
                    total_duration
                );
            }
            return Ok(ping_response);
        }

        // Extract promise_id from ping response
        let promise_id = ping_response
            .promise_id
            .ok_or_else(|| anyhow::anyhow!("Ping succeeded but no promise_id returned"))?;

        // Then do post (sync)
        #[cfg(all(feature = "tracing", debug_assertions))]
        let post_start = std::time::Instant::now();
        let post_response =
            Self::route_post_direct_sync(lead, campaign, &promise_id, qualification_config)?;
        #[cfg(all(feature = "tracing", debug_assertions))]
        {
            let post_duration = post_start.elapsed().as_millis() as u64;
            let total_duration = fullpost_start.elapsed().as_millis() as u64;
            tracing::debug!(
                "route_fullpost_direct_sync: post phase took {}ms, total {}ms",
                post_duration,
                total_duration
            );
        }

        // Return post response (which includes both ping_id and post_id)
        Ok(BuyerResponse {
            success: post_response.success,
            status: post_response.status,
            ping_id: ping_response.ping_id, // Include ping_id from ping phase
            post_id: post_response.post_id,
            promise_id: post_response.promise_id,
            price: post_response.price,
            bid: None, // Fullpost responses have price, not bid
            error: post_response.error,
            message: post_response.message,
        })
    }

    /// SYNC version: Direct call to Pulsar for ping requests (bypasses HTTP, no async overhead)
    /// Returns instantly with random bid + UUID promise
    #[inline(always)]
    pub fn route_ping_direct_sync(
        lead: &Lead,
        _campaign: &Campaign,
        qualification_config: Option<BuyerQualificationConfig>,
    ) -> Result<BuyerResponse> {
        let total_start = std::time::Instant::now();
        let lead_id = lead
            .lead_id
            .clone()
            .unwrap_or_else(|| lead.uuid.to_string());

        // Inject chaos delay if enabled (sync version)
        let chaos_start = std::time::Instant::now();
        inject_chaos_delay_sync();
        let chaos_duration = chaos_start.elapsed().as_millis() as u64;
        if chaos_duration > 0 {
            tracing::info!(
                chaos_delay_ms = chaos_duration,
                "Chaos delay injected (sync)"
            );
        }

        // Evaluate qualification rules using qualification engine (already sync)
        let qual_eval_start = std::time::Instant::now();
        let engine =
            QualificationEngine::new(lead.clone(), "ping".to_string(), qualification_config);
        let qual_result: QualificationResult = engine.evaluate();
        let qual_eval_duration = qual_eval_start.elapsed().as_millis() as u64;
        let accepted = qual_result.accepted;
        tracing::info!(
            qualification_evaluation_ms = qual_eval_duration,
            accepted = accepted,
            "Qualification evaluation completed"
        );

        if !accepted {
            let rejected_ping_id = generate_rejected_ping_id(&lead_id);
            return Ok(BuyerResponse {
                success: false,
                status: "rejected".to_string(),
                ping_id: Some(rejected_ping_id),
                post_id: None,
                promise_id: None,
                price: None,
                bid: None,
                error: Some("Lead rejected by qualification rules".to_string()),
                message: Some("Lead did not meet qualification requirements".to_string()),
            });
        }

        // Generate IDs instantly
        let id_generation_start = std::time::Instant::now();
        let ping_id = generate_ping_id(&lead_id);
        let promise_id = generate_promise_id();
        let id_generation_duration = id_generation_start.elapsed().as_millis() as u64;
        tracing::info!(
            id_generation_ms = id_generation_duration,
            "Pulsar ID generation completed"
        );

        // Return instantly with random bid
        let total_pulsar_duration = total_start.elapsed().as_millis() as u64;
        tracing::info!(
            total_pulsar_ping_ms = total_pulsar_duration,
            qualification_ms = qual_eval_duration,
            id_generation_ms = id_generation_duration,
            chaos_delay_ms = chaos_duration,
            "Pulsar ping_direct_sync completed"
        );
        Ok(BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            ping_id: Some(ping_id),
            post_id: None,
            promise_id: Some(promise_id),
            price: None,
            bid: Some((rand::random::<u32>() % 200 + 100) as f64),
            error: None,
            message: Some("Lead accepted for ping".to_string()),
        })
    }

    /// SYNC version: Direct call to Pulsar for post requests (bypasses HTTP, no async overhead)
    /// Returns instantly with random price + UUID promise
    #[inline(always)]
    pub fn route_post_direct_sync(
        lead: &Lead,
        _campaign: &Campaign,
        promise_id: &str,
        qualification_config: Option<BuyerQualificationConfig>,
    ) -> Result<BuyerResponse> {
        #[cfg(all(feature = "tracing", debug_assertions))]
        let total_start = std::time::Instant::now();

        let lead_id = lead
            .lead_id
            .clone()
            .unwrap_or_else(|| lead.uuid.to_string());

        // Inject chaos delay if enabled (sync version)
        inject_chaos_delay_sync();

        #[cfg(all(feature = "tracing", debug_assertions))]
        let qual_start = std::time::Instant::now();

        // Evaluate qualification rules using qualification engine (already sync)
        let engine =
            QualificationEngine::new(lead.clone(), "post".to_string(), qualification_config);
        let qual_result: QualificationResult = engine.evaluate();
        let accepted = qual_result.accepted;

        #[cfg(all(feature = "tracing", debug_assertions))]
        {
            let qual_duration = qual_start.elapsed().as_millis() as u64;
            tracing::debug!(
                "route_post_direct_sync: qualification took {}ms",
                qual_duration
            );
        }

        if !accepted {
            return Ok(BuyerResponse {
                success: false,
                status: "rejected".to_string(),
                ping_id: None,
                post_id: None,
                promise_id: Some(promise_id.to_string()),
                price: None,
                bid: None,
                error: Some("Lead rejected by qualification rules".to_string()),
                message: Some("Lead did not meet qualification requirements".to_string()),
            });
        }

        #[cfg(all(feature = "tracing", debug_assertions))]
        let gen_start = std::time::Instant::now();

        // Generate post_id instantly
        let post_id = generate_post_id(&lead_id);

        #[cfg(all(feature = "tracing", debug_assertions))]
        {
            let gen_duration = gen_start.elapsed().as_millis() as u64;
            let total_duration = total_start.elapsed().as_millis() as u64;
            tracing::debug!(
                "route_post_direct_sync: post_id generation took {}ms, total {}ms",
                gen_duration,
                total_duration
            );
        }

        // Return instantly with random price
        Ok(BuyerResponse {
            success: true,
            status: "sold".to_string(),
            ping_id: None,
            post_id: Some(post_id),
            promise_id: Some(promise_id.to_string()),
            price: Some((rand::random::<u32>() % 200 + 100) as f64),
            bid: None,
            error: None,
            message: Some("Lead accepted and sold".to_string()),
        })
    }
}
