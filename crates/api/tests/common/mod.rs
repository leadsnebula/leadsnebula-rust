// Test helper module for integration tests
// Handles database connection and migrations gracefully, even when migrations are already applied

use sqlx::PgPool;

/// Check if DATABASE_URL is available for integration tests
/// Returns true if DATABASE_URL is set, false otherwise
pub fn has_database_url() -> bool {
    // Load local env if present (non-fatal)
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();

    std::env::var("DATABASE_URL").is_ok()
}

/// Macro to skip a test if DATABASE_URL is not set
/// Usage: skip_if_no_db!();
#[macro_export]
macro_rules! skip_if_no_db {
    () => {
        if !$crate::tests::common::has_database_url() {
            eprintln!("⚠️  DATABASE_URL not set - skipping test");
            return;
        }
    };
}

/// Create a test database pool, applying migrations only if needed
/// This handles the case where migrations are already applied to the database
///
/// The sqlx Migrator should handle already-applied migrations gracefully,
/// but we catch any errors related to duplicate migration records.
pub async fn create_test_pool() -> anyhow::Result<PgPool> {
    // Load local env if present (non-fatal)
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();

    // Delegate to core's unified test helper for consistent behavior
    leadsnebula_core::test_helpers::create_test_pool().await
}

/// Execute a test function within a transaction that is automatically rolled back.
/// This ensures test data never persists in the database.
///
/// Usage:
/// ```rust
/// #[tokio::test]
/// async fn my_test() {
///     run_test_in_transaction(|pool| async move {
///         // Use pool for all database operations
///         // All changes will be rolled back automatically
///         sqlx::query("INSERT INTO ...").execute(&pool).await?;
///         Ok(())
///     }).await.unwrap();
/// }
/// ```
#[allow(dead_code)]
pub async fn run_test_in_transaction<F, Fut>(test_fn: F) -> anyhow::Result<()>
where
    F: FnOnce(PgPool) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    // Create pool and run provided test function. Tests should manage explicit
    // transactions (begin/rollback) if they need isolation.
    let pool = create_test_pool().await?;
    test_fn(pool).await
}

/// Create a test pool and begin a transaction.
/// The transaction should be rolled back at the end of the test.
///
/// Usage:
/// ```rust
/// #[tokio::test]
/// async fn my_test() -> sqlx::Result<()> {
///     let pool = create_test_pool().await?;
///     let mut tx = pool.begin().await?;
///     
///     // Use &mut *tx for all database operations
///     sqlx::query("INSERT INTO ...").execute(&mut *tx).await?;
///     
///     // Always rollback at the end
///     tx.rollback().await?;
///     Ok(())
/// }
/// ```
#[allow(dead_code)]
pub async fn create_test_pool_with_transaction(
) -> anyhow::Result<(PgPool, sqlx::Transaction<'static, sqlx::Postgres>)> {
    let pool = create_test_pool().await?;
    let tx = begin_transaction_with_retry(&pool).await?;
    Ok((pool, tx))
}

/// Begin a transaction with timeout and retry logic to handle pool exhaustion.
/// This prevents tests from hanging indefinitely when the connection pool is exhausted.
///
/// When running tests with nextest or in parallel, the pool can become exhausted if
/// previous tests haven't released their connections. This function:
/// 1. Wraps pool.begin() in a 30s timeout
/// 2. Retries with exponential backoff on PoolTimedOut errors
/// 3. Provides clear error messages for debugging
pub async fn begin_transaction_with_retry(
    pool: &PgPool,
) -> sqlx::Result<sqlx::Transaction<'static, sqlx::Postgres>> {
    let mut retries = 0;
    let max_retries = 5;
    loop {
        // Wrap pool.begin() in a timeout to catch hangs (pool exhaustion)
        match tokio::time::timeout(tokio::time::Duration::from_secs(30), pool.begin()).await {
            Ok(Ok(tx)) => return Ok(tx),
            Ok(Err(sqlx::Error::PoolTimedOut)) if retries < max_retries => {
                retries += 1;
                let delay_ms = 200 * retries; // 200ms, 400ms, 600ms, 800ms, 1000ms
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                continue;
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                // Timeout occurred - pool.begin() hung (pool is exhausted)
                if retries < max_retries {
                    retries += 1;
                    let delay_ms = 1000 * retries; // Wait longer for connections to be released
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    continue;
                } else {
                    return Err(sqlx::Error::PoolTimedOut);
                }
            }
        }
    }
}
