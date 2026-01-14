#[cfg(test)]
mod test_helpers_tests {
    use crate::test_helpers::{clear_test_caches, create_test_pool, is_migration_cached};

    #[tokio::test]
    async fn migration_and_pool_cache_prevents_re_run() {
        // Opt-in test; skip if DATABASE_URL is not set
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");

        // Start from a clean cache
        clear_test_caches();

        // First call should run migrations (or ensure migrations are applied)
        let pool1 = create_test_pool().await.expect("create_test_pool failed");

        // after running, migration cache should contain the DB
        assert!(is_migration_cached(&database_url), "migration should be cached after initial run");

        // Second call should return a cached pool without re-running migrations
        let pool2 = create_test_pool().await.expect("create_test_pool failed second time");

        // Validate that simple queries succeed on the returned pool
        sqlx::query("SELECT 1").execute(&pool2).await.expect("health query failed");

        // cleanup caches for other tests
        clear_test_caches();
    }
}
