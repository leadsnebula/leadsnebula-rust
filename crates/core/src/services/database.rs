use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;
use tracing::info;

pub async fn create_pool(database_url: &str) -> anyhow::Result<PgPool> {
    info!("Creating database connection pool");
    let pool = PgPoolOptions::new()
        .max_connections(100) // Increase for concurrent auctions
        .min_connections(10) // Keep connections warm to avoid cold start latency
        .acquire_timeout(Duration::from_secs(10)) // Increase timeout for initial connection
        .idle_timeout(Some(Duration::from_secs(600)))
        .max_lifetime(Some(Duration::from_secs(1800)))
        .test_before_acquire(true) // Verify connections are alive
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                // Set statement timeout to prevent long-running queries
                sqlx::query("SET statement_timeout = '8s'")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(database_url)
        .await?;
    Ok(pool)
}
