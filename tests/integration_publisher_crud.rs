// Integration tests for Publisher CRUD operations
// These tests require a test database connection
// 
// To run these tests locally:
// 1. Add DATABASE_URL to .env.local (recommended for local development)
//    Example: DATABASE_URL=postgresql://user:password@localhost:5432/test_db
// 2. Or export DATABASE_URL in your shell before running tests
//    export DATABASE_URL="postgresql://user:password@localhost:5432/test_db"
//
// Run with: cargo test --test integration_publisher_crud

mod common;

use sqlx::PgPool;
use uuid::Uuid;
use common::create_test_pool;

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
    let pool = create_test_pool().await
        .expect("Failed to create test pool");
    // Test creating a publisher with all required fields
    let instance_id = Uuid::new_v4();
    let publisher_name = format!("Test Publisher {}", Uuid::new_v4());
    let publisher_email = format!("test{}@example.com", Uuid::new_v4());
    
    // Create instance first (required foreign key)
    sqlx::query(
        "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
         VALUES ($1, 'Test Instance', 'active', $1, NOW(), NOW())"
    )
    .bind(instance_id)
    .execute(&pool)
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
    let encrypted_key = encryption_service.encrypt(&test_api_key)
        .expect("Failed to encrypt API key");

    sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, 'active', $5, $6, $7, NOW(), NOW())
        "#
    )
    .bind(publisher_id)
    .bind(instance_id)
    .bind(&publisher_name)
    .bind(&publisher_email)
    .bind(api_key_prefix)
    .bind(&api_key_hash)
    .bind(&encrypted_key)
    .execute(&pool)
    .await?;

    // Verify publisher was created
    let result = sqlx::query_scalar::<_, String>(
        "SELECT name FROM publishers WHERE id = $1"
    )
    .bind(publisher_id)
    .fetch_one(&pool)
    .await?;

    assert_eq!(result, publisher_name);
    Ok(())
}

#[tokio::test]
async fn test_list_publishers() -> sqlx::Result<()> {
    let pool = create_test_pool().await
        .expect("Failed to create test pool");
    // Test listing publishers
    let instance_id = Uuid::new_v4();
    
    // Create instance
    sqlx::query(
        "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
         VALUES ($1, 'Test Instance', 'active', $1, NOW(), NOW())"
    )
    .bind(instance_id)
    .execute(&pool)
    .await?;

    // Create multiple publishers
    let test_key = vec![0u8; 32];
    let encryption_service = leadsnebula_core::encryption::EncryptionService::new(&test_key)
        .expect("Failed to create encryption service");

    for i in 0..3 {
        let publisher_id = Uuid::new_v4();
        let api_key = format!("pk_test_{}", Uuid::new_v4());
        let encrypted_key = encryption_service.encrypt(&api_key)
            .expect("Failed to encrypt API key");

        sqlx::query(
            r#"
            INSERT INTO publishers (
                id, instance_id, name, email, status, api_key_prefix, api_key_hash,
                api_key_encrypted, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, 'active', 'pk_test_', $5, $6, NOW(), NOW())
            "#
        )
        .bind(publisher_id)
        .bind(instance_id)
        .bind(format!("Publisher {}", i))
        .bind(format!("publisher{}@example.com", i))
        .bind(format!("hash_{}", i))
        .bind(&encrypted_key)
        .execute(&pool)
        .await?;
    }

    // List all publishers
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM publishers WHERE instance_id = $1 AND deleted_at IS NULL"
    )
    .bind(instance_id)
    .fetch_one(&pool)
    .await?;

    assert_eq!(count, 3);
    Ok(())
}

#[tokio::test]
async fn test_update_publisher() -> sqlx::Result<()> {
    let pool = create_test_pool().await
        .expect("Failed to create test pool");
    // Test updating a publisher
    let instance_id = Uuid::new_v4();
    let publisher_id = Uuid::new_v4();
    
    // Create instance
    sqlx::query(
        "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
         VALUES ($1, 'Test Instance', 'active', $1, NOW(), NOW())"
    )
    .bind(instance_id)
    .execute(&pool)
    .await?;

    // Create publisher
    let test_key = vec![0u8; 32];
    let encryption_service = leadsnebula_core::encryption::EncryptionService::new(&test_key)
        .expect("Failed to create encryption service");
    let api_key = format!("pk_test_{}", Uuid::new_v4());
    let encrypted_key = encryption_service.encrypt(&api_key)
        .expect("Failed to encrypt API key");

    sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, 'Original Name', 'original@example.com', 'active', 'pk_test_', 'hash1', $3, NOW(), NOW())
        "#
    )
    .bind(publisher_id)
    .bind(instance_id)
    .bind(&encrypted_key)
    .execute(&pool)
    .await?;

    // Update publisher
    sqlx::query(
        "UPDATE publishers SET name = $1, updated_at = NOW() WHERE id = $2"
    )
    .bind("Updated Name")
    .bind(publisher_id)
    .execute(&pool)
    .await?;

    // Verify update
    let name: String = sqlx::query_scalar(
        "SELECT name FROM publishers WHERE id = $1"
    )
    .bind(publisher_id)
    .fetch_one(&pool)
    .await?;

    assert_eq!(name, "Updated Name");
    Ok(())
}

#[tokio::test]
async fn test_publisher_email_uniqueness_for_active() -> sqlx::Result<()> {
    let pool = create_test_pool().await
        .expect("Failed to create test pool");
    // Test that active publishers cannot have duplicate emails
    let instance_id = Uuid::new_v4();
    let email = format!("unique{}@example.com", Uuid::new_v4());
    
    // Create instance
    sqlx::query(
        "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
         VALUES ($1, 'Test Instance', 'active', $1, NOW(), NOW())"
    )
    .bind(instance_id)
    .execute(&pool)
    .await?;

    // Create first publisher
    let test_key = vec![0u8; 32];
    let encryption_service = leadsnebula_core::encryption::EncryptionService::new(&test_key)
        .expect("Failed to create encryption service");
    let api_key1 = format!("pk_test_{}", Uuid::new_v4());
    let encrypted_key1 = encryption_service.encrypt(&api_key1)
        .expect("Failed to encrypt API key");

    sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, 'Publisher 1', $3, 'active', 'pk_test_', 'hash1', $4, NOW(), NOW())
        "#
    )
    .bind(Uuid::new_v4())
    .bind(instance_id)
    .bind(&email)
    .bind(&encrypted_key1)
    .execute(&pool)
    .await?;

    // Try to create second publisher with same email (should fail)
    let api_key2 = format!("pk_test_{}", Uuid::new_v4());
    let encrypted_key2 = encryption_service.encrypt(&api_key2)
        .expect("Failed to encrypt API key");

    let result = sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, 'Publisher 2', $3, 'active', 'pk_test_', 'hash2', $4, NOW(), NOW())
        "#
    )
    .bind(Uuid::new_v4())
    .bind(instance_id)
    .bind(&email)
    .bind(&encrypted_key2)
    .execute(&pool)
    .await;

    // Should fail due to unique constraint
    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn test_deleted_publisher_email_reuse() -> sqlx::Result<()> {
    let pool = create_test_pool().await
        .expect("Failed to create test pool");
    // Test that deleted publishers' emails can be reused
    let instance_id = Uuid::new_v4();
    let email = format!("reusable{}@example.com", Uuid::new_v4());
    
    // Create instance
    sqlx::query(
        "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
         VALUES ($1, 'Test Instance', 'active', $1, NOW(), NOW())"
    )
    .bind(instance_id)
    .execute(&pool)
    .await?;

    // Create and delete first publisher
    let test_key = vec![0u8; 32];
    let encryption_service = leadsnebula_core::encryption::EncryptionService::new(&test_key)
        .expect("Failed to create encryption service");
    let api_key1 = format!("pk_test_{}", Uuid::new_v4());
    let encrypted_key1 = encryption_service.encrypt(&api_key1)
        .expect("Failed to encrypt API key");

    let publisher_id1 = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, 'Publisher 1', $3, 'active', 'pk_test_', 'hash1', $4, NOW(), NOW())
        "#
    )
    .bind(publisher_id1)
    .bind(instance_id)
    .bind(&email)
    .bind(&encrypted_key1)
    .execute(&pool)
    .await?;

    // Delete publisher
    sqlx::query(
        "UPDATE publishers SET deleted_at = NOW() WHERE id = $1"
    )
    .bind(publisher_id1)
    .execute(&pool)
    .await?;

    // Create new publisher with same email (should succeed)
    let api_key2 = format!("pk_test_{}", Uuid::new_v4());
    let encrypted_key2 = encryption_service.encrypt(&api_key2)
        .expect("Failed to encrypt API key");

    let publisher_id2 = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO publishers (
            id, instance_id, name, email, status, api_key_prefix, api_key_hash,
            api_key_encrypted, created_at, updated_at
        )
        VALUES ($1, $2, 'Publisher 2', $3, 'active', 'pk_test_', 'hash2', $4, NOW(), NOW())
        "#
    )
    .bind(publisher_id2)
    .bind(instance_id)
    .bind(&email)
    .bind(&encrypted_key2)
    .execute(&pool)
    .await?;

    // Verify second publisher was created
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM publishers WHERE email = $1 AND deleted_at IS NULL"
    )
    .bind(&email)
    .fetch_one(&pool)
    .await?;

    assert_eq!(count, 1); // Only the new one should be active
    Ok(())
}
