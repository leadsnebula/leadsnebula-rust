use sqlx::{postgres::PgPoolOptions, Connection, PgPool};
use std::time::Duration;
use tokio_retry::{strategy::ExponentialBackoff, Retry};
use tracing::{info, warn};

pub async fn create_pool(database_url: &str) -> anyhow::Result<PgPool> {
    info!("Creating database connection pool");

    // Wake up Neon free-tier instance with a simple connection attempt
    // This helps reduce cold-start delays when Neon has suspended
    if let Ok(mut conn) = sqlx::PgConnection::connect(database_url).await {
        if let Err(e) = sqlx::query("SELECT 1").execute(&mut conn).await {
            warn!("Neon wake-up query failed (non-critical): {}", e);
        } else {
            info!("Neon instance woken up successfully");
        }
        drop(conn); // Close the wake-up connection
    }

    // Retry pool creation with exponential backoff for transient connection errors
    // Neon free-tier can take 5-30+ seconds to wake from suspend
    let retry_strategy = ExponentialBackoff::from_millis(500)
        .max_delay(Duration::from_secs(5))
        .take(5); // More retries for Neon wake-up

    let database_url_clone = database_url.to_string();
    let pool = Retry::spawn(retry_strategy, move || {
        let url = database_url_clone.clone();
        async move {
            // Conservative pool config for Neon free-tier compatibility
            // Free-tier Neon suspends after idle and has connection limits
            // - Lower min_connections to avoid overwhelming on cold start
            // - Longer acquire_timeout to allow Neon wake-up time
            // - Disable test_before_acquire to reduce startup queries
            PgPoolOptions::new()
                .max_connections(30) // Reduced from 100 for free-tier compatibility
                .min_connections(2) // Reduced from 20 - free Neon can't handle 20 at once
                .acquire_timeout(Duration::from_secs(10)) // Increased from 100ms to allow Neon wake-up
                .idle_timeout(Some(Duration::from_secs(300))) // Reduced from 600s - Neon free-tier may close idle connections
                .max_lifetime(Some(Duration::from_secs(1800))) // Reduced from 3600s - rotate connections more frequently
                .test_before_acquire(true) // Re-enabled to detect stale connections (Neon may close idle connections)
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
                .connect(&url)
                .await
        }
    })
    .await?;

    info!("Database connection pool created successfully");
    Ok(pool)
}
