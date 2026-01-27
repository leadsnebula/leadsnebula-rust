// Write-behind queue for batching background database writes
// Reduces spawn overhead and batches writes for better performance

use crate::models::enums::LeadStatus;
use anyhow::Result;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Async logging helpers - fire-and-forget to avoid blocking operations
/// All logging is spawned as separate tasks to ensure 0ms impact on critical path
mod async_log {
    use super::*;

    /// Log batch flush summary (async, non-blocking)
    pub fn log_batch_flush(
        batch_size: usize,
        flush_duration_ms: u64,
        pulsar_count: usize,
        buyer_count: usize,
        lead_count: usize,
        payload_count: usize,
        creation_count: usize,
    ) {
        tokio::spawn(async move {
            // Must have: Batch flush timing (especially if slow)
            if flush_duration_ms > 500 {
                tracing::warn!(
                    target: "write_behind_queue",
                    operation = "batch_flush",
                    batch_size = batch_size,
                    flush_duration_ms = flush_duration_ms,
                    pulsar_logs = pulsar_count,
                    buyer_responses = buyer_count,
                    lead_updates = lead_count,
                    payload_updates = payload_count,
                    lead_creations = creation_count,
                    "Slow background DB batch flush"
                );
            } else {
                // Nice to have: Batch flush frequency (info level for monitoring)
                tracing::info!(
                    target: "write_behind_queue",
                    operation = "batch_flush",
                    batch_size = batch_size,
                    flush_duration_ms = flush_duration_ms,
                    "Background DB batch flushed"
                );
            }
        });
    }

    /// Log lead creation duration (async, non-blocking)
    pub fn log_lead_creation(lead_uuid: Uuid, duration_ms: u64) {
        tokio::spawn(async move {
            // Must have: Lead creation duration (especially if slow)
            if duration_ms > 500 {
                tracing::warn!(
                    target: "write_behind_queue",
                    operation = "lead_creation",
                    lead_id = %lead_uuid,
                    duration_ms = duration_ms,
                    "Slow lead creation (encryption/DB write)"
                );
            } else {
                // Nice to have: Per-operation duration when fast (debug level)
                tracing::debug!(
                    target: "write_behind_queue",
                    operation = "lead_creation",
                    lead_id = %lead_uuid,
                    duration_ms = duration_ms,
                    "Lead created in background"
                );
            }
        });
    }

    /// Log lead update (async, non-blocking)
    pub fn log_lead_update(lead_id: Uuid, status: LeadStatus, duration_ms: u64) {
        tokio::spawn(async move {
            // SENTRY ALERT: Slow persist operations
            #[cfg(feature = "sentry")]
            if duration_ms > 300 {
                sentry::capture_message(
                    &format!(
                        "Slow lead update persist: {}ms for lead {}",
                        duration_ms, lead_id
                    ),
                    sentry::Level::Warning,
                );
            }

            // Must have: Lead status changes (especially Sold transitions)
            let is_sold = matches!(status, LeadStatus::Sold);
            let is_slow = duration_ms > 200;

            if is_sold || is_slow {
                tracing::info!(
                    target: "write_behind_queue",
                    operation = "lead_update",
                    lead_id = %lead_id,
                    status = ?status,
                    duration_ms = duration_ms,
                    "Lead updated in background"
                );
            } else {
                // Nice to have: Per-operation duration when fast (debug level)
                tracing::debug!(
                    target: "write_behind_queue",
                    operation = "lead_update",
                    lead_id = %lead_id,
                    status = ?status,
                    duration_ms = duration_ms,
                    "Lead updated"
                );
            }
        });
    }

    /// Log buyer response batch insert (async, non-blocking)
    pub fn log_buyer_responses_batch(count: usize, duration_ms: u64) {
        tokio::spawn(async move {
            // Must have: Batch sizes (helps spot batching issues)
            // Nice to have: Per-operation duration when slow
            if duration_ms > 200 {
                tracing::warn!(
                    target: "write_behind_queue",
                    operation = "buyer_responses_batch",
                    count = count,
                    duration_ms = duration_ms,
                    "Slow buyer responses batch insert"
                );
            } else {
                tracing::debug!(
                    target: "write_behind_queue",
                    operation = "buyer_responses_batch",
                    count = count,
                    duration_ms = duration_ms,
                    "Buyer responses persisted"
                );
            }
        });
    }
}

// Type aliases to reduce complexity
type BuyerResponseTuple = (
    Uuid,
    Uuid,
    Option<String>,
    Option<String>,
    Option<Uuid>,
    Value,
);
type LeadUpdateTuple = (
    Uuid,
    LeadStatus, // Changed from String to LeadStatus enum
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,           // sold_at
    Option<String>, // inprog_token
    Option<Value>,  // vertical_data (JSONB)
);
type PayloadUpdateTuple = (
    Uuid,
    String,
    Value,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<uuid::Uuid>,
    Option<String>,
);

/// Background tasks that can be batched
#[derive(Clone)]
#[allow(clippy::large_enum_variant)] // LeadCreation variant is large but necessary for decoupling
pub enum BackgroundTask {
    /// Pulsar decision log
    PulsarLog {
        lead_id: Uuid,
        ping_id: Option<String>,
        buyer_id: Uuid,
        accepted: bool,
        final_bid_price: rust_decimal::Decimal,
    },
    /// Lead update
    LeadUpdate {
        lead_id: Uuid,
        status: LeadStatus, // Changed from String to LeadStatus enum for type safety
        campaign_id: Option<Uuid>,
        buyer_id: Option<Uuid>,
        promise_id: Option<String>,
        ping_id: Option<String>,
        post_id: Option<String>,
        sold_at: bool, // If true, set sold_at = NOW() when status = LeadStatus::Sold
        inprog_token: Option<String>, // For conditional update (WHERE post_id = inprog_token)
        vertical_data: Option<Value>, // Optional JSONB data (e.g., auction timing)
    },
    /// Buyer response batch insert
    BuyerResponse {
        lead_id: Uuid,
        campaign_id: Uuid,
        ping_id: Option<String>,
        post_id: Option<String>,
        buyer_id: Option<Uuid>,
        payload: Value,
    },
    /// Payload update (ping_payloads or post_payloads)
    PayloadUpdate {
        lead_id: Uuid,
        payload_type: String, // "ping" or "post"
        payload: Value,
        // For post_payloads INSERT:
        post_id: Option<String>,
        request_payload_encrypted: Option<String>,
        response_payload_encrypted: Option<String>,
        // For ping_payloads UPDATE:
        ping_payloads_row_id: Option<uuid::Uuid>, // row id for ping_payloads UPDATE (actually lead_id, used to find the row)
        external_ping_id: Option<String>,         // external_ping_id for ping_payloads UPDATE
    },
    /// Lead creation (decoupled from critical path)
    /// All encryption happens here in batch
    LeadCreation {
        event_id: String,
        lead_id: Option<String>,
        publisher_id: Uuid,
        vertical_id: Uuid,
        request_type: String,
        strategy: String,
        promise_id: Option<String>,
        buyer_id: Uuid,
        campaign_id: Uuid,
        tcpa_consent: bool,
        tcpa_language: String,
        is_test: bool,
        session_id: String,
        vertical_data: Value,
        // Raw PII fields (will be encrypted in batch processor)
        first_name: Option<String>,
        last_name: Option<String>,
        email: Option<String>,
        cell_phone: Option<String>,
        street_address: Option<String>,
        city: Option<String>,
        state: Option<String>,
        zip: Option<String>,
        ip_address: Option<String>,
        // Raw request payload (will be encrypted in batch processor)
        request_payload: Value,
        // SSM encryption key (derived from det_key + salt, passed to avoid re-fetching)
        pii_encryption_key: Option<Vec<u8>>,
    },
}

/// Write-behind queue that batches background tasks
pub struct WriteBehindQueue {
    sender: mpsc::UnboundedSender<BackgroundTask>,
    shutdown_flag: Arc<AtomicBool>,
    batcher_handle: tokio::task::JoinHandle<()>,
}

impl WriteBehindQueue {
    pub fn new(pool: Arc<PgPool>) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let pool_clone = pool.clone();
        let shutdown_flag_clone = Arc::clone(&shutdown_flag);

        // Spawn batcher task
        let batcher_handle = tokio::spawn(Self::batcher_task(
            receiver,
            shutdown_flag_clone,
            pool_clone,
        ));

        Self {
            sender,
            shutdown_flag,
            batcher_handle,
        }
    }

    /// Enqueue a background task (non-blocking)
    pub fn enqueue(&self, task: BackgroundTask) {
        if let Err(e) = self.sender.send(task) {
            tracing::warn!("Write-behind queue receiver dropped: {}", e);
        }
    }

    /// Batcher task that collects tasks and flushes them periodically
    async fn batcher_task(
        mut receiver: mpsc::UnboundedReceiver<BackgroundTask>,
        shutdown_flag: Arc<AtomicBool>,
        pool: Arc<PgPool>,
    ) {
        let mut batch = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                // Collect tasks
                task = receiver.recv() => {
                    match task {
                        Some(task) => {
                            batch.push(task);
                            // Flush if batch is full (10 items)
                            if batch.len() >= 10 {
                                Self::flush_batch(&mut batch, &pool).await;
                            }
                        }
                        None => {
                            // Receiver closed - flush remaining and exit
                            Self::flush_batch(&mut batch, &pool).await;
                            break;
                        }
                    }
                }
                // Flush on interval (100ms)
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        Self::flush_batch(&mut batch, &pool).await;
                    }
                    // Check shutdown flag
                    if shutdown_flag.load(Ordering::Relaxed) {
                        // Flush remaining batch and exit
                        Self::flush_batch(&mut batch, &pool).await;
                        break;
                    }
                }
            }
        }
    }

    /// Flush a batch of tasks to the database
    async fn flush_batch(batch: &mut Vec<BackgroundTask>, pool: &PgPool) {
        if batch.is_empty() {
            return;
        }

        let flush_start = Instant::now();

        // Group tasks by type for batch processing
        let mut pulsar_logs = Vec::new();
        let mut buyer_responses = Vec::new();
        let mut lead_updates = Vec::new();
        let mut payload_updates = Vec::new();
        let mut lead_creations = Vec::new();

        for task in batch.drain(..) {
            match task {
                BackgroundTask::PulsarLog {
                    lead_id,
                    ping_id,
                    buyer_id,
                    accepted,
                    final_bid_price,
                } => pulsar_logs.push((lead_id, ping_id, buyer_id, accepted, final_bid_price)),
                BackgroundTask::BuyerResponse {
                    lead_id,
                    campaign_id,
                    ping_id,
                    post_id,
                    buyer_id,
                    payload,
                } => buyer_responses.push((
                    lead_id,
                    campaign_id,
                    ping_id,
                    post_id,
                    buyer_id,
                    payload,
                )),
                BackgroundTask::LeadUpdate {
                    lead_id,
                    status,
                    campaign_id,
                    buyer_id,
                    promise_id,
                    ping_id,
                    post_id,
                    sold_at,
                    inprog_token,
                    vertical_data,
                } => lead_updates.push((
                    lead_id,
                    status,
                    campaign_id,
                    buyer_id,
                    promise_id,
                    ping_id,
                    post_id,
                    sold_at,
                    inprog_token,
                    vertical_data,
                )),
                BackgroundTask::PayloadUpdate {
                    lead_id,
                    payload_type,
                    payload,
                    post_id,
                    request_payload_encrypted,
                    response_payload_encrypted,
                    ping_payloads_row_id,
                    external_ping_id,
                } => payload_updates.push((
                    lead_id,
                    payload_type,
                    payload,
                    post_id,
                    request_payload_encrypted,
                    response_payload_encrypted,
                    ping_payloads_row_id,
                    external_ping_id,
                )),
                BackgroundTask::LeadCreation { .. } => lead_creations.push(task),
            }
        }

        let batch_size = pulsar_logs.len()
            + buyer_responses.len()
            + lead_updates.len()
            + payload_updates.len()
            + lead_creations.len();

        // Execute batches in parallel
        let (pulsar_result, buyer_result, lead_result, payload_result, creation_result) = tokio::join!(
            Self::batch_insert_pulsar_logs(&pulsar_logs, pool),
            Self::batch_insert_buyer_responses(&buyer_responses, pool),
            Self::batch_update_leads(&lead_updates, pool),
            Self::batch_update_payloads(&payload_updates, pool),
            Self::batch_create_leads(&lead_creations, pool),
        );

        let flush_duration_ms = flush_start.elapsed().as_millis() as u64;

        // Log batch flush summary (async, non-blocking - 0ms impact)
        async_log::log_batch_flush(
            batch_size,
            flush_duration_ms,
            pulsar_logs.len(),
            buyer_responses.len(),
            lead_updates.len(),
            payload_updates.len(),
            lead_creations.len(),
        );

        // Log errors (non-blocking)
        if let Err(e) = pulsar_result {
            tracing::warn!("Failed to batch insert pulsar logs: {}", e);
        }
        if let Err(e) = buyer_result {
            tracing::warn!("Failed to batch insert buyer responses: {}", e);
        }
        if let Err(e) = lead_result {
            tracing::warn!("Failed to batch update leads: {}", e);
        }
        if let Err(e) = payload_result {
            tracing::warn!("Failed to batch update payloads: {}", e);
        }
        if let Err(e) = creation_result {
            tracing::warn!("Failed to batch create leads: {}", e);
        }
    }

    /// Batch insert pulsar logs
    async fn batch_insert_pulsar_logs(
        logs: &[(Uuid, Option<String>, Uuid, bool, rust_decimal::Decimal)],
        pool: &PgPool,
    ) -> Result<()> {
        if logs.is_empty() {
            return Ok(());
        }

        // Use UNNEST for batch insert
        let lead_ids: Vec<Uuid> = logs.iter().map(|(id, _, _, _, _)| *id).collect();
        let ping_ids: Vec<Option<String>> =
            logs.iter().map(|(_, id, _, _, _)| (*id).clone()).collect();
        let buyer_ids: Vec<Uuid> = logs.iter().map(|(_, _, id, _, _)| *id).collect();
        let accepteds: Vec<bool> = logs.iter().map(|(_, _, _, a, _)| *a).collect();
        let prices: Vec<Option<rust_decimal::Decimal>> =
            logs.iter().map(|(_, _, _, _, p)| Some(*p)).collect();

        sqlx::query(
            r#"
            INSERT INTO pulsar_decision_logs (lead_id, ping_id, buyer_id, accepted, final_bid_price, evaluated_at)
            SELECT * FROM UNNEST($1::uuid[], $2::text[], $3::uuid[], $4::boolean[], $5::decimal[], $6::timestamp[])
            "#,
        )
        .bind(&lead_ids[..])
        .bind(&ping_ids[..])
        .bind(&buyer_ids[..])
        .bind(&accepteds[..])
        .bind(&prices[..])
        .bind(&vec![chrono::Utc::now(); logs.len()][..])
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Batch insert buyer responses
    async fn batch_insert_buyer_responses(
        responses: &[BuyerResponseTuple],
        pool: &PgPool,
    ) -> Result<()> {
        if responses.is_empty() {
            return Ok(());
        }

        let insert_start = Instant::now();
        let mut success_count = 0;
        let mut error_count = 0;

        // Use existing batch_insert_buyer_responses from ping_tree_router
        // For now, insert individually (can optimize later with UNNEST)
        for (lead_id, campaign_id, ping_id, post_id, buyer_id, payload) in responses {
            match sqlx::query(
                r#"
                INSERT INTO buyer_responses (lead_id, campaign_id, ping_id, post_id, buyer_id, payload, created_at)
                VALUES ($1, $2, $3, $4, $5, $6, NOW())
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(lead_id)
            .bind(campaign_id)
            .bind(ping_id)
            .bind(post_id)
            .bind(buyer_id)
            .bind(sqlx::types::Json(payload))
            .execute(pool)
            .await
            {
                Ok(_) => {
                    success_count += 1;
                }
                Err(e) => {
                    error_count += 1;
                    tracing::error!(
                        lead_id = %lead_id,
                        campaign_id = ?campaign_id,
                        ping_id = ?ping_id,
                        post_id = ?post_id,
                        error = %e,
                        "Failed to insert buyer response in write-behind queue"
                    );
                }
            }
        }

        let insert_duration_ms = insert_start.elapsed().as_millis() as u64;

        if error_count > 0 {
            tracing::warn!(
                total_responses = responses.len(),
                success_count = success_count,
                error_count = error_count,
                duration_ms = insert_duration_ms,
                "Buyer response batch insert completed with errors"
            );
        } else {
            tracing::info!(
                total_responses = responses.len(),
                success_count = success_count,
                duration_ms = insert_duration_ms,
                "Buyer response batch insert completed successfully"
            );
        }

        // Log buyer response batch insert (async, non-blocking - 0ms impact)
        async_log::log_buyer_responses_batch(responses.len(), insert_duration_ms);

        Ok(())
    }

    /// Batch update leads
    async fn batch_update_leads(updates: &[LeadUpdateTuple], pool: &PgPool) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        // Update individually (can optimize later)
        for (
            lead_id,
            status,
            campaign_id,
            buyer_id,
            promise_id,
            ping_id,
            post_id,
            sold_at,
            inprog_token,
            vertical_data,
        ) in updates
        {
            let update_start = Instant::now();
            // CRITICAL: Also update if campaign_id and buyer_id are set (indicating sold) even if sold_at flag is false
            // This handles cases where the sold_at flag wasn't set correctly but the lead was actually sold
            let should_update_as_sold = (*sold_at && *status == LeadStatus::Sold)
                || (campaign_id.is_some() && buyer_id.is_some() && *status == LeadStatus::Sold);

            if should_update_as_sold {
                // Update with sold_at and ALL fields (campaign_id, buyer_id, ping_id, promise_id, post_id, vertical_data)
                // CRITICAL: Must update all fields, not just post_id and status, so leads show complete information
                if let Some(token) = inprog_token {
                    tracing::debug!(
                        "Updating sold lead {} with inprog_token (conditional update)",
                        lead_id
                    );
                    // CRITICAL: buyer_id, campaign_id, and post_id are NOT NULL in schema
                    // Only update if we have valid values, otherwise use COALESCE to keep existing values
                    match sqlx::query(
                        r#"
                        UPDATE leads
                        SET status = $2, 
                            campaign_id = COALESCE($3, campaign_id), 
                            buyer_id = COALESCE($4, buyer_id), 
                            promise_id = COALESCE($5, promise_id), 
                            ping_id = COALESCE($6, ping_id), 
                            post_id = COALESCE($7, post_id),
                            sold_at = NOW(), 
                            vertical_data = COALESCE($8, vertical_data), 
                            updated_at = NOW()
                        WHERE uuid = $1 AND post_id = $9
                        "#,
                    )
                    .bind(lead_id)
                    .bind(status)
                    .bind(campaign_id)
                    .bind(buyer_id)
                    .bind(promise_id)
                    .bind(ping_id)
                    .bind(post_id.as_deref().unwrap_or(""))
                    .bind(vertical_data.as_ref().map(sqlx::types::Json))
                    .bind(token)
                    .execute(pool)
                    .await
                    {
                        Ok(_) => {
                            let update_duration_ms = update_start.elapsed().as_millis() as u64;
                            // Log lead update (async, non-blocking - 0ms impact)
                            async_log::log_lead_update(
                                *lead_id,
                                status.clone(),
                                update_duration_ms,
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to update lead {} to sold status: {}",
                                lead_id,
                                e
                            );
                        }
                    }
                } else {
                    tracing::debug!(
                        "Updating sold lead {} without inprog_token (unconditional update), post_id: {:?}",
                        lead_id,
                        post_id
                    );
                    // CRITICAL: buyer_id, campaign_id, and post_id are NOT NULL in schema
                    // Only update if we have valid values, otherwise use COALESCE to keep existing values
                    match sqlx::query(
                        r#"
                        UPDATE leads
                        SET status = $2, 
                            campaign_id = COALESCE($3, campaign_id), 
                            buyer_id = COALESCE($4, buyer_id), 
                            promise_id = COALESCE($5, promise_id), 
                            ping_id = COALESCE($6, ping_id), 
                            post_id = COALESCE($7, post_id),
                            sold_at = NOW(), 
                            vertical_data = COALESCE($8, vertical_data), 
                            updated_at = NOW()
                        WHERE uuid = $1
                        "#,
                    )
                    .bind(lead_id)
                    .bind(status)
                    .bind(campaign_id)
                    .bind(buyer_id)
                    .bind(promise_id)
                    .bind(ping_id)
                    .bind(post_id.as_deref().unwrap_or(""))
                    .bind(vertical_data.as_ref().map(sqlx::types::Json))
                    .execute(pool)
                    .await
                    {
                        Ok(result) => {
                            let update_duration_ms = update_start.elapsed().as_millis() as u64;
                            let rows_affected = result.rows_affected();
                            if rows_affected == 0 {
                                tracing::warn!(
                                    "Lead {} update to sold status completed but affected 0 rows (lead may not exist or already updated)",
                                    lead_id
                                );
                            } else {
                                tracing::info!(
                                    "Successfully updated lead {} to sold status in {}ms (rows affected: {})",
                                    lead_id,
                                    update_duration_ms,
                                    rows_affected
                                );
                            }
                            // Log lead update (async, non-blocking - 0ms impact)
                            async_log::log_lead_update(
                                *lead_id,
                                status.clone(),
                                update_duration_ms,
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to update lead {} to sold status: {}",
                                lead_id,
                                e
                            );
                        }
                    }
                }
            } else {
                tracing::debug!(
                    "Skipping sold_at update for lead {}: sold_at={}, status={:?}",
                    lead_id,
                    sold_at,
                    status
                );
            }

            // Handle inprog_token reset or standard update
            if let Some(token) = inprog_token {
                // Reset placeholder (clear inprog_token)
                if let Err(e) = sqlx::query(
                    r#"
                    UPDATE leads SET post_id = '' WHERE uuid = $1 AND post_id = $2
                    "#,
                )
                .bind(lead_id)
                .bind(token)
                .execute(pool)
                .await
                {
                    tracing::error!("Failed to reset inprog_token for lead {}: {}", lead_id, e);
                }
            } else {
                // Standard update (with optional vertical_data)
                if let Some(vd) = vertical_data {
                    // CRITICAL: buyer_id, campaign_id, and post_id are NOT NULL in schema
                    // Only update if we have valid values, otherwise use COALESCE to keep existing values
                    match sqlx::query(
                        r#"
                        UPDATE leads
                        SET status = $2, 
                            campaign_id = COALESCE($3, campaign_id), 
                            buyer_id = COALESCE($4, buyer_id), 
                            promise_id = COALESCE($5, promise_id), 
                            ping_id = COALESCE($6, ping_id), 
                            post_id = COALESCE($7, post_id), 
                            vertical_data = $8, 
                            updated_at = NOW()
                        WHERE uuid = $1
                        "#,
                    )
                    .bind(lead_id)
                    .bind(status)
                    .bind(campaign_id)
                    .bind(buyer_id)
                    .bind(promise_id)
                    .bind(ping_id)
                    .bind(post_id.as_deref().unwrap_or(""))
                    .bind(sqlx::types::Json(vd))
                    .execute(pool)
                    .await
                    {
                        Ok(_) => {
                            let update_duration_ms = update_start.elapsed().as_millis() as u64;
                            // Log lead update (async, non-blocking - 0ms impact)
                            async_log::log_lead_update(
                                *lead_id,
                                status.clone(),
                                update_duration_ms,
                            );
                        }
                        Err(e) => {
                            tracing::error!("Failed to update lead {} status: {}", lead_id, e);
                        }
                    }
                } else {
                    // CRITICAL: buyer_id, campaign_id, and post_id are NOT NULL in schema
                    // Only update if we have valid values, otherwise use COALESCE to keep existing values
                    match sqlx::query(
                        r#"
                        UPDATE leads
                        SET status = $2, 
                            campaign_id = COALESCE($3, campaign_id), 
                            buyer_id = COALESCE($4, buyer_id), 
                            promise_id = COALESCE($5, promise_id), 
                            ping_id = COALESCE($6, ping_id), 
                            post_id = COALESCE($7, post_id), 
                            updated_at = NOW()
                        WHERE uuid = $1
                        "#,
                    )
                    .bind(lead_id)
                    .bind(status)
                    .bind(campaign_id)
                    .bind(buyer_id)
                    .bind(promise_id)
                    .bind(ping_id)
                    .bind(post_id.as_deref().unwrap_or(""))
                    .execute(pool)
                    .await
                    {
                        Ok(_) => {
                            let update_duration_ms = update_start.elapsed().as_millis() as u64;
                            // Log lead update (async, non-blocking - 0ms impact)
                            async_log::log_lead_update(
                                *lead_id,
                                status.clone(),
                                update_duration_ms,
                            );
                        }
                        Err(e) => {
                            tracing::error!("Failed to update lead {} status: {}", lead_id, e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Batch update payloads
    async fn batch_update_payloads(updates: &[PayloadUpdateTuple], pool: &PgPool) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        // Update individually (can optimize later)
        for (
            lead_id,
            payload_type,
            payload,
            post_id,
            request_payload_encrypted,
            response_payload_encrypted,
            ping_payloads_row_id,
            external_ping_id,
        ) in updates
        {
            if payload_type == "ping" {
                // Upsert ping_payloads with response_payload_encrypted and external_ping_id
                // Handle race condition where UPDATE happens before INSERT by trying UPDATE first, then INSERT if no rows affected
                // Note: ping_payloads_row_id is actually lead_id (used to find/create the row)
                // Since ping_payloads doesn't have UNIQUE constraint on lead_id, we update the most recent row or insert if none exists
                if let Some(_row_id) = ping_payloads_row_id {
                    // Try UPDATE first (most common case - row exists from LeadCreation)
                    let update_result = sqlx::query(
                        r#"
                        UPDATE ping_payloads
                        SET payload = COALESCE(payload, 'null'::jsonb),
                            response_payload_encrypted = COALESCE($1, response_payload_encrypted),
                            external_ping_id = COALESCE($2, external_ping_id),
                            updated_at = NOW()
                        WHERE lead_id = $3
                        AND id = (SELECT id FROM ping_payloads WHERE lead_id = $3 ORDER BY created_at DESC LIMIT 1)
                        "#,
                    )
                    .bind(response_payload_encrypted)
                    .bind(external_ping_id)
                    .bind(lead_id)
                    .execute(pool)
                    .await;

                    // If UPDATE affected 0 rows, INSERT a new row (handles race condition)
                    if let Ok(rows_affected) = update_result {
                        if rows_affected.rows_affected() == 0 {
                            let _ = sqlx::query(
                                r#"
                                INSERT INTO ping_payloads (lead_id, payload, response_payload_encrypted, external_ping_id, created_at, updated_at)
                                VALUES ($1, COALESCE($2::jsonb, 'null'::jsonb), $3, $4, NOW(), NOW())
                                "#,
                            )
                            .bind(lead_id)
                            .bind(sqlx::types::Json(payload))
                            .bind(response_payload_encrypted)
                            .bind(external_ping_id)
                            .execute(pool)
                            .await;
                        }
                    }
                } else {
                    // Fallback: try UPDATE, then INSERT if no rows affected
                    let update_result = sqlx::query(
                        r#"
                        UPDATE ping_payloads
                        SET payload = COALESCE($2, payload),
                            response_payload_encrypted = COALESCE($3, response_payload_encrypted),
                            external_ping_id = COALESCE($4, external_ping_id),
                            updated_at = NOW()
                        WHERE lead_id = $1
                        AND id = (SELECT id FROM ping_payloads WHERE lead_id = $1 ORDER BY created_at DESC LIMIT 1)
                        "#,
                    )
                    .bind(lead_id)
                    .bind(sqlx::types::Json(payload))
                    .bind(response_payload_encrypted)
                    .bind(external_ping_id)
                    .execute(pool)
                    .await;

                    // If UPDATE affected 0 rows, INSERT a new row
                    if let Ok(rows_affected) = update_result {
                        if rows_affected.rows_affected() == 0 {
                            let _ = sqlx::query(
                                r#"
                                INSERT INTO ping_payloads (lead_id, payload, response_payload_encrypted, external_ping_id, created_at, updated_at)
                                VALUES ($1, $2, $3, $4, NOW(), NOW())
                                "#,
                            )
                            .bind(lead_id)
                            .bind(sqlx::types::Json(payload))
                            .bind(response_payload_encrypted)
                            .bind(external_ping_id)
                            .execute(pool)
                            .await;
                        }
                    }
                }
            } else if payload_type == "post" {
                // Insert into post_payloads with all fields
                if let (Some(er), Some(epr)) =
                    (request_payload_encrypted, response_payload_encrypted)
                {
                    let _ = sqlx::query(
                        r#"
                        INSERT INTO post_payloads (lead_id, post_id, payload, request_payload_encrypted, response_payload_encrypted, created_at)
                        VALUES ($1, $2, $3, $4, $5, now())
                        "#,
                    )
                    .bind(lead_id)
                    .bind(post_id)
                    .bind(sqlx::types::Json(payload))
                    .bind(er)
                    .bind(epr)
                    .execute(pool)
                    .await;
                } else {
                    let _ = sqlx::query(
                        r#"
                        INSERT INTO post_payloads (lead_id, post_id, payload, created_at)
                        VALUES ($1, $2, $3, now())
                        "#,
                    )
                    .bind(lead_id)
                    .bind(post_id)
                    .bind(sqlx::types::Json(payload))
                    .execute(pool)
                    .await;
                }
            }
        }

        Ok(())
    }

    /// Batch create leads with encryption
    async fn batch_create_leads(creations: &[BackgroundTask], pool: &PgPool) -> Result<()> {
        if creations.is_empty() {
            return Ok(());
        }

        // Process each lead creation individually (encryption happens here)
        for task in creations {
            if let BackgroundTask::LeadCreation {
                event_id,
                lead_id,
                publisher_id,
                vertical_id,
                request_type,
                strategy,
                promise_id,
                buyer_id,
                campaign_id,
                tcpa_consent,
                tcpa_language,
                is_test,
                session_id,
                vertical_data,
                first_name,
                last_name,
                email,
                cell_phone,
                street_address,
                city,
                state,
                zip,
                ip_address,
                request_payload,
                pii_encryption_key,
            } = task
            {
                // Encrypt PII fields in batch (using shared key if available)
                let encrypt_pii = |value: Option<String>| -> Option<String> {
                    if let (Some(val), Some(key)) = (value, pii_encryption_key) {
                        if !val.is_empty() {
                            if let Ok(envelope) =
                                crate::encryption::EncryptionService::encrypt_envelope(
                                    key, &val, true,
                                )
                            {
                                return Some(envelope);
                            }
                        }
                    }
                    None
                };

                let first_name_encrypted = encrypt_pii(first_name.clone());
                let last_name_encrypted = encrypt_pii(last_name.clone());
                let email_encrypted = encrypt_pii(email.clone());
                let cell_phone_encrypted = encrypt_pii(cell_phone.clone());
                let street_address_encrypted = encrypt_pii(street_address.clone());
                let city_encrypted = encrypt_pii(city.clone());
                let state_encrypted = encrypt_pii(state.clone());
                let zip_encrypted = encrypt_pii(zip.clone());
                let ip_address_encrypted = encrypt_pii(ip_address.clone());

                // Encrypt request payload if key is available
                let encrypted_request = if let Some(key) = pii_encryption_key {
                    if let Ok(bytes) = simd_json::to_vec(request_payload) {
                        if let Ok(req_str) = String::from_utf8(bytes) {
                            crate::encryption::EncryptionService::encrypt_envelope(
                                key, &req_str, true,
                            )
                            .ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                let creation_start = Instant::now();

                // Start transaction
                let mut tx = match pool.begin().await {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::error!("Failed to start transaction for lead creation: {}", e);
                        continue;
                    }
                };

                // Insert lead
                let lead_uuid_result = sqlx::query(
                    r#"
                    INSERT INTO leads (
                        event_id, lead_id, publisher_id, vertical_id, request_type, strategy, status,
                        promise_id, tcpa_consent, tcpa_language, is_test, session_id, vertical_data,
                        buyer_id, campaign_id, post_id, submitted_at, created_at,
                        first_name_encrypted, last_name_encrypted, email_encrypted, cell_phone_encrypted,
                        street_address_encrypted, city_encrypted, state_encrypted, zip_encrypted, ip_address_encrypted
                    ) VALUES (
                        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                        $14, $15, $16, NOW(), NOW(),
                        $17, $18, $19, $20, $21, $22, $23, $24, $25
                    ) RETURNING uuid
                    "#,
                )
                .bind(event_id)
                .bind(lead_id.as_ref())
                .bind(publisher_id)
                .bind(vertical_id)
                .bind(request_type)
                .bind(strategy)
                .bind(LeadStatus::Processing)
                .bind(promise_id.as_ref())
                .bind(*tcpa_consent)
                .bind(tcpa_language)
                .bind(*is_test)
                .bind(session_id)
                .bind(sqlx::types::Json(vertical_data))
                .bind(buyer_id)
                .bind(campaign_id)
                .bind("")
                .bind(first_name_encrypted)
                .bind(last_name_encrypted)
                .bind(email_encrypted)
                .bind(cell_phone_encrypted)
                .bind(street_address_encrypted)
                .bind(city_encrypted)
                .bind(state_encrypted)
                .bind(zip_encrypted)
                .bind(ip_address_encrypted)
                .fetch_one(&mut *tx)
                .await;

                let lead_uuid = match lead_uuid_result {
                    Ok(row) => {
                        use sqlx::Row;
                        row.get::<uuid::Uuid, _>(0)
                    }
                    Err(e) => {
                        tracing::error!("Database error creating lead: {}", e);
                        let _ = tx.rollback().await;
                        continue;
                    }
                };

                // Generate ping_id
                let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
                let lead_id_str = lead_uuid.to_string();
                let payload_str = format!("{}|{}|pending", lead_id_str, timestamp);
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD.encode(payload_str);
                let ping_id_text = format!("FP_{}", encoded);

                // Insert ping record
                let ping_id_result = sqlx::query(
                    "INSERT INTO pings (ping_id, lead_id, promise_id, state, sent_at, created_at) VALUES ($1, $2, $3, $4, now(), now()) RETURNING id"
                )
                .bind(&ping_id_text)
                .bind(lead_uuid)
                .bind(promise_id.as_ref())
                .bind("processing")
                .fetch_one(&mut *tx)
                .await;

                let ping_id_val = match ping_id_result {
                    Ok(row) => {
                        use sqlx::Row;
                        row.get::<i64, _>("id")
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to create ping record for lead {} (non-critical): {}",
                            lead_uuid,
                            e
                        );
                        0i64
                    }
                };

                // Insert ping_payloads
                if ping_id_val > 0 {
                    if let Err(e) = sqlx::query(
                        "INSERT INTO ping_payloads (ping_id, lead_id, payload, request_payload_encrypted, created_at) VALUES ($1, $2, $3, $4, now())"
                    )
                    .bind(ping_id_val)
                    .bind(lead_uuid)
                    .bind(sqlx::types::Json(request_payload))
                    .bind(encrypted_request)
                    .execute(&mut *tx)
                    .await
                    {
                        tracing::warn!("Failed to create ping_payloads record for lead {} (non-critical): {}", lead_uuid, e);
                    }
                }

                // Commit transaction
                if let Err(e) = tx.commit().await {
                    tracing::error!("Failed to commit transaction for lead {}: {}", lead_uuid, e);
                } else {
                    let creation_duration_ms = creation_start.elapsed().as_millis() as u64;
                    // Log lead creation (async, non-blocking - 0ms impact)
                    async_log::log_lead_creation(lead_uuid, creation_duration_ms);
                }
            }
        }

        Ok(())
    }

    /// Flush remaining batch (for shutdown)
    /// Returns Result for error handling, uses internal timeout for safety
    pub async fn flush(&self) -> Result<(), anyhow::Error> {
        // Signal shutdown to batcher task
        self.shutdown_flag.store(true, Ordering::Relaxed);

        // Also close the sender to ensure receiver gets None
        drop(self.sender.clone());

        // Wait for batcher task to complete with timeout
        // Poll the handle until it's finished or timeout
        let start = std::time::Instant::now();
        loop {
            if self.batcher_handle.is_finished() {
                // Task finished - we can't get the result without moving the handle,
                // but if it finished without panicking, we consider it successful
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(5) {
                #[cfg(feature = "tracing")]
                tracing::warn!("Write-behind queue flush timed out after 5s");
                return Err(anyhow::anyhow!("Flush timed out after 5s"));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl Drop for WriteBehindQueue {
    fn drop(&mut self) {
        // On drop, signal shutdown (best-effort)
        // The batcher task will flush remaining batch and exit
        // Note: In practice, flush() should be called explicitly before drop
        self.shutdown_flag.store(true, Ordering::Relaxed);
    }
}
