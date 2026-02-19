// Integration tests for Publisher CRUD operations
// These tests require a test database connection
//
// To run these tests locally:
// 1. Add DATABASE_URL to .env.local (recommended for local development)
//    Example: DATABASE_URL=postgresql://user:password@localhost:5432/test_db
// 2. Or export DATABASE_URL in your shell before running tests
//    export DATABASE_URL="postgresql://user:password@localhost:5432/test_db"
//
// Run with: cargo test --test integration_publisher_crud (from workspace root or crates/api)

mod common;

use common::create_test_pool;
use leadsnebula_core::auth::hash_password;
use uuid::Uuid;

// Helper function to create a test instance_user
// Works with both PgPool and Transaction (via sqlx::Executor trait)
async fn create_test_instance_user<'e, E>(executor: E) -> Uuid
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let user_id = Uuid::new_v4();
    let unique_email = format!(
        "test_user_{}_{}@test.invalid",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        Uuid::new_v4().to_string().split('-').next().unwrap()
    );
    let password_hash = hash_password("TestPassword123!").unwrap();

    sqlx::query(
        r#"
        INSERT INTO instance_users (id, email, encrypted_password, status, confirmed_at, created_at, updated_at)
        VALUES ($1, $2, $3, 'active', NOW(), NOW(), NOW())
        "#,
    )
    .bind(user_id)
    .bind(&unique_email)
    .bind(password_hash)
    .execute(executor)
    .await
    .unwrap();

    user_id
}

// Load .env.local automatically before tests run
// This allows setting DATABASE_URL in .env.local for local development
// Environment variables take precedence over .env.local
#[ctor::ctor]
fn init() {
    // Try .env.local first (for local development)
    let _ = dotenvy::from_filename(".env.local");
    // Fallback to .env if .env.local doesn't exist
    let _ = dotenvy::dotenv();
}

#[tokio::test]
async fn test_create_publisher() -> sqlx::Result<()> {
    if !common::has_database_url() {
        eprintln!("⚠️  DATABASE_URL not set - skipping test_create_publisher");
        return Ok(());
    }
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");
    let mut tx = pool.begin().await?;
    // Test creating a publisher with all required fields
    let instance_user_id = create_test_instance_user(&mut *tx).await;
    let instance_id = Uuid::new_v4();
    let publisher_name = format!("Test Publisher {}", Uuid::new_v4());
    let publisher_email = format!("test{}@example.com", Uuid::new_v4());

    // Create instance first (required foreign key)
    sqlx::query(
        "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
         VALUES ($1, 'Test Instance', 'active', $2, NOW(), NOW())",
    )
    .bind(instance_id)
    .bind(instance_user_id)
    .execute(&mut *tx)
    .await?;

    // Create publisher
    let publisher_id = Uuid::new_v4();
    let api_key_prefix = "pk_test_";
    let api_key_hash = format!("hash_{}", Uuid::new_v4());

    // Generate a test encryption key (32 bytes)
    let test_key = vec![0u8; 32];
    let encryption_service = leadsnebula_core::encryption::EncryptionService::new(&test_key)
        .expect("Failed to create encryption service");
    let test_api_key = format!("{}1234567890abcdef", api_key_prefix);
    let encrypted_key = encryption_service
        .encrypt(&test_api_key)
        .expect("Failed to encrypt API key");

    sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, 'active', $5, $6, $7, NOW(), NOW())
        "#,
    )
    .bind(publisher_id)
    .bind(instance_id)
    .bind(&publisher_name)
    .bind(&publisher_email)
    .bind(api_key_prefix)
    .bind(&api_key_hash)
    .bind(&encrypted_key)
    .execute(&mut *tx)
    .await?;

    // Verify publisher was created
    let result = sqlx::query_scalar::<_, String>("SELECT name FROM publishers WHERE id = $1")
        .bind(publisher_id)
        .fetch_one(&mut *tx)
        .await?;

    assert_eq!(result, publisher_name);

    // Rollback transaction to prevent test data from persisting
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
async fn test_list_publishers() -> sqlx::Result<()> {
    if !common::has_database_url() {
        eprintln!("⚠️  DATABASE_URL not set - skipping test_list_publishers");
        return Ok(());
    }
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");
    let mut tx = pool.begin().await?;
    // Test listing publishers
    let instance_user_id = create_test_instance_user(&mut *tx).await;
    let instance_id = Uuid::new_v4();

    // Create instance
    sqlx::query(
        "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
         VALUES ($1, 'Test Instance', 'active', $2, NOW(), NOW())",
    )
    .bind(instance_id)
    .bind(instance_user_id)
    .execute(&mut *tx)
    .await?;

    // Create multiple publishers
    let test_key = vec![0u8; 32];
    let encryption_service = leadsnebula_core::encryption::EncryptionService::new(&test_key)
        .expect("Failed to create encryption service");

    for i in 0..3 {
        let publisher_id = Uuid::new_v4();
        let api_key = format!("pk_test_{}", Uuid::new_v4());
        let encrypted_key = encryption_service
            .encrypt(&api_key)
            .expect("Failed to encrypt API key");
        let api_key_hash = format!("hash_{}_{}", i, Uuid::new_v4());
        let unique_email = format!("publisher{}_{}@example.com", i, Uuid::new_v4());

        sqlx::query(
            r#"
            INSERT INTO publishers (
                id, instance_id, name, email, status, api_key_prefix, api_key_hash,
                api_key_encrypted, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, 'active', 'pk_test_', $5, $6, NOW(), NOW())
            "#,
        )
        .bind(publisher_id)
        .bind(instance_id)
        .bind(format!("Publisher {}", i))
        .bind(&unique_email)
        .bind(&api_key_hash)
        .bind(&encrypted_key)
        .execute(&mut *tx)
        .await?;
    }

    // List all publishers
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM publishers WHERE instance_id = $1 AND deleted_at IS NULL",
    )
    .bind(instance_id)
    .fetch_one(&mut *tx)
    .await?;

    assert_eq!(count, 3);

    // Rollback transaction to prevent test data from persisting
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
async fn test_update_publisher() -> sqlx::Result<()> {
    if !common::has_database_url() {
        eprintln!("⚠️  DATABASE_URL not set - skipping test_update_publisher");
        return Ok(());
    }
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");
    let mut tx = pool.begin().await?;
    // Test updating a publisher
    let instance_user_id = create_test_instance_user(&mut *tx).await;
    let instance_id = Uuid::new_v4();
    let publisher_id = Uuid::new_v4();

    // Create instance
    sqlx::query(
        "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
         VALUES ($1, 'Test Instance', 'active', $2, NOW(), NOW())",
    )
    .bind(instance_id)
    .bind(instance_user_id)
    .execute(&mut *tx)
    .await?;

    // Create publisher
    let test_key = vec![0u8; 32];
    let encryption_service = leadsnebula_core::encryption::EncryptionService::new(&test_key)
        .expect("Failed to create encryption service");
    let api_key = format!("pk_test_{}", Uuid::new_v4());
    let encrypted_key = encryption_service
        .encrypt(&api_key)
        .expect("Failed to encrypt API key");
    let api_key_hash = format!("hash_{}", Uuid::new_v4());
    let unique_email = format!("original_{}@example.com", Uuid::new_v4());
    sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, 'Original Name', $3, 'active', 'pk_test_', $4, $5, NOW(), NOW())
        "#,
    )
    .bind(publisher_id)
    .bind(instance_id)
    .bind(&unique_email)
    .bind(&api_key_hash)
    .bind(&encrypted_key)
    .execute(&mut *tx)
    .await?;

    // Update publisher
    sqlx::query("UPDATE publishers SET name = $1, updated_at = NOW() WHERE id = $2")
        .bind("Updated Name")
        .bind(publisher_id)
        .execute(&mut *tx)
        .await?;

    // Verify update
    let name: String = sqlx::query_scalar("SELECT name FROM publishers WHERE id = $1")
        .bind(publisher_id)
        .fetch_one(&mut *tx)
        .await?;

    assert_eq!(name, "Updated Name");

    // Rollback transaction to prevent test data from persisting
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
async fn test_publisher_email_may_be_shared_by_active_publishers() -> sqlx::Result<()> {
    if !common::has_database_url() {
        eprintln!(
            "⚠️  DATABASE_URL not set - skipping test_publisher_email_may_be_shared_by_active_publishers"
        );
        return Ok(());
    }
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");
    let mut tx = pool.begin().await?;
    // After migration 20260218000003, multiple active publishers may share the same email
    // (e.g. Only Solar and Only Solar Dev). Verify the second insert succeeds.
    let instance_user_id = create_test_instance_user(&mut *tx).await;
    let instance_id = Uuid::new_v4();
    let email = format!("shared{}@example.com", Uuid::new_v4());

    // Create instance
    sqlx::query(
        "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
         VALUES ($1, 'Test Instance', 'active', $2, NOW(), NOW())",
    )
    .bind(instance_id)
    .bind(instance_user_id)
    .execute(&mut *tx)
    .await?;

    // Create first publisher
    let test_key = vec![0u8; 32];
    let encryption_service = leadsnebula_core::encryption::EncryptionService::new(&test_key)
        .expect("Failed to create encryption service");
    let api_key1 = format!("pk_test_{}", Uuid::new_v4());
    let encrypted_key1 = encryption_service
        .encrypt(&api_key1)
        .expect("Failed to encrypt API key");
    let api_key_hash1 = format!("hash_{}", Uuid::new_v4());

    sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, 'Publisher 1', $3, 'active', 'pk_test_', $4, $5, NOW(), NOW())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(instance_id)
    .bind(&email)
    .bind(&api_key_hash1)
    .bind(&encrypted_key1)
    .execute(&mut *tx)
    .await?;

    // Create second publisher with same email (allowed since publishers_email_unique_not_deleted was dropped)
    let api_key2 = format!("pk_test_{}", Uuid::new_v4());
    let encrypted_key2 = encryption_service
        .encrypt(&api_key2)
        .expect("Failed to encrypt API key");
    let api_key_hash2 = format!("hash_{}", Uuid::new_v4());

    let result = sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, 'Publisher 2', $3, 'active', 'pk_test_', $4, $5, NOW(), NOW())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(instance_id)
    .bind(&email)
    .bind(&api_key_hash2)
    .bind(&encrypted_key2)
    .execute(&mut *tx)
    .await;

    // Should succeed: duplicate email for active publishers is allowed
    result.expect("Second publisher with same email should be allowed");

    // Rollback transaction to prevent test data from persisting
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
async fn test_deleted_publisher_email_reuse() -> sqlx::Result<()> {
    if !common::has_database_url() {
        eprintln!("⚠️  DATABASE_URL not set - skipping test_deleted_publisher_email_reuse");
        return Ok(());
    }
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");

    // Retry transaction begin with exponential backoff to handle pool exhaustion
    // This matches the pattern used in test_otp_enable_and_disable
    let mut tx = {
        let mut retries = 0;
        let max_retries = 3;
        loop {
            match pool.begin().await {
                Ok(tx) => break Ok(tx),
                Err(sqlx::Error::PoolTimedOut) if retries < max_retries => {
                    retries += 1;
                    let delay_ms = 100 * (1 << retries); // 200ms, 400ms, 800ms
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    continue;
                }
                Err(e) => break Err(e),
            }
        }
    }?;
    // Test that deleted publishers' emails can be reused
    let instance_user_id = create_test_instance_user(&mut *tx).await;
    let instance_id = Uuid::new_v4();
    let email = format!("reusable{}@example.com", Uuid::new_v4());

    // Create instance
    sqlx::query(
        "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
         VALUES ($1, 'Test Instance', 'active', $2, NOW(), NOW())",
    )
    .bind(instance_id)
    .bind(instance_user_id)
    .execute(&mut *tx)
    .await?;

    // Create and delete first publisher
    let test_key = vec![0u8; 32];
    let encryption_service = leadsnebula_core::encryption::EncryptionService::new(&test_key)
        .expect("Failed to create encryption service");
    let api_key1 = format!("pk_test_{}", Uuid::new_v4());
    let encrypted_key1 = encryption_service
        .encrypt(&api_key1)
        .expect("Failed to encrypt API key");
    let api_key_hash1 = format!("hash_{}", Uuid::new_v4());

    let publisher_id1 = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, 'Publisher 1', $3, 'active', 'pk_test_', $4, $5, NOW(), NOW())
        "#,
    )
    .bind(publisher_id1)
    .bind(instance_id)
    .bind(&email)
    .bind(&api_key_hash1)
    .bind(&encrypted_key1)
    .execute(&mut *tx)
    .await?;

    // Delete publisher
    sqlx::query("UPDATE publishers SET deleted_at = NOW() WHERE id = $1")
        .bind(publisher_id1)
        .execute(&mut *tx)
        .await?;

    // Create new publisher with same email (should succeed)
    let api_key2 = format!("pk_test_{}", Uuid::new_v4());
    let encrypted_key2 = encryption_service
        .encrypt(&api_key2)
        .expect("Failed to encrypt API key");
    let api_key_hash2 = format!("hash_{}", Uuid::new_v4());

    let publisher_id2 = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, 'Publisher 2', $3, 'active', 'pk_test_', $4, $5, NOW(), NOW())
        "#,
    )
    .bind(publisher_id2)
    .bind(instance_id)
    .bind(&email)
    .bind(&api_key_hash2)
    .bind(&encrypted_key2)
    .execute(&mut *tx)
    .await?;

    // Verify second publisher was created
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM publishers WHERE email = $1 AND deleted_at IS NULL",
    )
    .bind(&email)
    .fetch_one(&mut *tx)
    .await?;

    assert_eq!(count, 1); // Only the new one should be active

    // Rollback transaction to prevent test data from persisting
    tx.rollback().await?;
    Ok(())
}
