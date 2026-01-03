use sqlx::PgPool;
use tracing::info;

pub async fn create_pool(database_url: &str) -> anyhow::Result<PgPool> {
    info!("Creating database connection pool");
    let pool = PgPool::connect(database_url).await?;
    Ok(pool)
}
