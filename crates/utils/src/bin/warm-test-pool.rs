//! Warm-up binary for test database connectivity.
//!
//! Runs create_test_pool() and a trivial query to warm Neon compute and the migration
//! check path before integration tests. Use in autotestsall.sh and CI after run-migrations.
//!
//! Requires: DATABASE_URL, EPHEMERAL_DB=1 or TEST_MODE=true (for ephemeral DB safeguard).
//! Do not load .env.local when DATABASE_URL is already set - parent (autotestsall) provides
//! the ephemeral branch URL; loading would overwrite it with stale values.

use anyhow::Result;
use leadsnebula_core::test_helpers::create_test_pool;

#[tokio::main]
async fn main() -> Result<()> {
    // Only load .env if DATABASE_URL not set (parent script provides it for Neon ephemeral)
    if std::env::var("DATABASE_URL").is_err() {
        let _ = dotenvy::from_filename(".env.local");
        let _ = dotenvy::dotenv();
    }

    let pool = create_test_pool().await?;
    sqlx::query("SELECT 1").execute(&pool).await?;
    Ok(())
}
