use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;
use tokio_retry::{strategy::ExponentialBackoff, Retry};
use tracing::info;

pub async fn create_pool(database_url: &str) -> anyhow::Result<PgPool> {
    info!("Creating database connection pool");

    // Retry pool creation with exponential backoff for transient connection errors
    let retry_strategy = ExponentialBackoff::from_millis(100)
        .max_delay(Duration::from_secs(2))
        .take(3);

    let pool = Retry::spawn(retry_strategy, || async {
        PgPoolOptions::new()
            .max_connections(100) // Increase for concurrent auctions
            .min_connections(20) // Increase warm connections to reduce acquire latency
            .acquire_timeout(Duration::from_millis(100)) // Fail fast on cold (reduced from 10s)
            .idle_timeout(Some(Duration::from_secs(300))) // Keep connections alive longer (reduced from 600s)
            .max_lifetime(Some(Duration::from_secs(3600))) // Longer lifetime (increased from 1800s)
            .test_before_acquire(true) // Verify connections are alive
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    // Pre-warm with a simple query
                    sqlx::query("SELECT 1").execute(&mut *conn).await?;
                    // Set statement timeout to prevent long-running queries
                    sqlx::query("SET statement_timeout = '5s'")
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(database_url)
            .await
    })
    .await?;

    Ok(pool)
}
