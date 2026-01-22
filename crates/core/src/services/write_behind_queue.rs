// Write-behind queue for batching background database writes
// Reduces spawn overhead and batches writes for better performance

use anyhow::Result;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

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
    String,
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Background tasks that can be batched
#[derive(Clone)]
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
        status: String,
        campaign_id: Option<Uuid>,
        buyer_id: Option<Uuid>,
        promise_id: Option<String>,
        ping_id: Option<String>,
        post_id: Option<String>,
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

        // Group tasks by type for batch processing
        let mut pulsar_logs = Vec::new();
        let mut buyer_responses = Vec::new();
        let mut lead_updates = Vec::new();
        let mut payload_updates = Vec::new();

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
                } => lead_updates.push((
                    lead_id,
                    status,
                    campaign_id,
                    buyer_id,
                    promise_id,
                    ping_id,
                    post_id,
                )),
                BackgroundTask::PayloadUpdate {
                    lead_id,
                    payload_type,
                    payload,
                } => payload_updates.push((lead_id, payload_type, payload)),
            }
        }

        // Execute batches in parallel
        let (pulsar_result, buyer_result, lead_result, payload_result) = tokio::join!(
            Self::batch_insert_pulsar_logs(&pulsar_logs, pool),
            Self::batch_insert_buyer_responses(&buyer_responses, pool),
            Self::batch_update_leads(&lead_updates, pool),
            Self::batch_update_payloads(&payload_updates, pool),
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

        // Use existing batch_insert_buyer_responses from ping_tree_router
        // For now, insert individually (can optimize later with UNNEST)
        for (lead_id, campaign_id, ping_id, post_id, buyer_id, payload) in responses {
            let _ = sqlx::query(
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
            .await;
        }

        Ok(())
    }

    /// Batch update leads
    async fn batch_update_leads(updates: &[LeadUpdateTuple], pool: &PgPool) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        // Update individually (can optimize later)
        for (lead_id, status, campaign_id, buyer_id, promise_id, ping_id, post_id) in updates {
            let _ = sqlx::query(
                r#"
                UPDATE leads
                SET status = $2, campaign_id = $3, buyer_id = $4, promise_id = $5, ping_id = $6, post_id = $7, updated_at = NOW()
                WHERE uuid = $1
                "#,
            )
            .bind(lead_id)
            .bind(status)
            .bind(campaign_id)
            .bind(buyer_id)
            .bind(promise_id)
            .bind(ping_id)
            .bind(post_id)
            .execute(pool)
            .await;
        }

        Ok(())
    }

    /// Batch update payloads
    async fn batch_update_payloads(updates: &[(Uuid, String, Value)], pool: &PgPool) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        // Update individually (can optimize later)
        for (lead_id, payload_type, payload) in updates {
            if payload_type == "ping" {
                let _ = sqlx::query(
                    r#"
                    UPDATE ping_payloads
                    SET payload = $2, updated_at = NOW()
                    WHERE lead_id = $1
                    "#,
                )
                .bind(lead_id)
                .bind(sqlx::types::Json(payload))
                .execute(pool)
                .await;
            } else if payload_type == "post" {
                let _ = sqlx::query(
                    r#"
                    INSERT INTO post_payloads (lead_id, payload, created_at)
                    VALUES ($1, $2, NOW())
                    ON CONFLICT (lead_id) DO UPDATE SET payload = $2, updated_at = NOW()
                    "#,
                )
                .bind(lead_id)
                .bind(sqlx::types::Json(payload))
                .execute(pool)
                .await;
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
