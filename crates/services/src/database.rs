use anyhow::Result;
use sqlx::postgres::{PgPoolOptions, Postgres};
use sqlx::Pool;
use std::time::Duration;

/// Create a database connection pool
pub async fn create_pool(database_url: &str) -> Result<Pool<Postgres>> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .min_connections(2) // Pre-warm connections for faster response
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(1800))
        .connect(database_url)
        .await?;

    Ok(pool)
}
