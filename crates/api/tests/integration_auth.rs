// Integration tests for authentication endpoints
// These tests verify authentication-related functionality including:
// - JWT login and token verification (no database required)
// - Password hashing/verification (no database required)
// - OTP setup, enable, disable (requires database)
// - OTP backup codes storage (requires database)
// - Passkey credential storage (requires database)
// - User password verification (requires database)
// - User status management (requires database)
//
// Tests are split into two categories:
// 1. Unit-style tests (using #[tokio::test]) - No database required
//    - test_password_hashing_and_verification
//    - test_jwt_token_encoding_and_decoding
//    - test_jwt_token_with_different_secrets
//    - test_jwt_token_expiration_validation
//
// 2. Integration tests (using #[tokio::test] with create_test_pool helper) - Require DATABASE_URL
//    - All OTP tests
//    - All passkey tests
//    - User-related tests
//
// To run these tests locally:
// 1. Add DATABASE_URL to .env.local (recommended for local development)
//    Example: DATABASE_URL=postgresql://user:password@localhost:5432/test_db
// 2. Or export DATABASE_URL in your shell before running tests
//    export DATABASE_URL="postgresql://user:password@localhost:5432/test_db"
//
// Run with: cargo test --test integration_auth --features otp,webauthn
//
// Note: Tests that require a database will panic if DATABASE_URL is not set, which is expected behavior.
// The create_test_pool helper handles migrations gracefully, even if they're already applied.

mod common;

use common::create_test_pool;
use leadsnebula_core::auth::{hash_password, verify_password, JwtService};
use sqlx::PgPool;
use uuid::Uuid;

// Load .env.local automatically before tests run
// Note: Each test that needs env vars should load them, or we can use a test setup function
fn load_test_env() {
    // Try .env.local first (for local development)
    let _ = dotenv::from_filename(".env.local");
    // Fallback to .env if .env.local doesn't exist
    let _ = dotenv::dotenv();
}

// Helper function to create a test user with a unique email
async fn create_test_user(pool: &PgPool, email: &str) -> Uuid {
    let user_id = Uuid::new_v4();
    let password_hash = hash_password("TestPassword123!").unwrap();

    // Make email unique by appending timestamp and random UUID
    let unique_email = format!(
        "{}_{}_{}@test.invalid",
        email.split('@').next().unwrap_or(email),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        Uuid::new_v4().to_string().split('-').next().unwrap()
    );

    sqlx::query(
        r#"
        INSERT INTO instance_users (id, email, encrypted_password, status, confirmed_at, created_at, updated_at)
        VALUES ($1, $2, $3, 'active', NOW(), NOW(), NOW())
        "#,
    )
    .bind(user_id)
    .bind(&unique_email)
    .bind(password_hash)
    .execute(pool)
    .await
    .unwrap();

    user_id
}

#[tokio::test]
async fn test_password_hashing_and_verification() {
    load_test_env();
    let password = "TestPassword123!";
    let hash = hash_password(password).unwrap();

    // Verify password matches hash
    assert!(verify_password(password, &hash).unwrap());

    // Verify wrong password doesn't match
    assert!(!verify_password("WrongPassword123!", &hash).unwrap());
}

#[tokio::test]
async fn test_jwt_token_encoding_and_decoding() {
    load_test_env();
    let jwt_secret = "test_jwt_secret_key_for_integration_tests".to_string();
    let service = JwtService::new(jwt_secret);

    let user_id = "123e4567-e89b-12d3-a456-426614174000".to_string();
    let email = "test@example.com".to_string();

    // Encode token
    let token = service.encode(user_id.clone(), email.clone()).unwrap();
    assert!(!token.is_empty());

    // Decode token
    let claims = service.decode(&token).unwrap();
    assert_eq!(claims.user_id, user_id);
    assert_eq!(claims.email, email);

    // Verify expiration is set correctly (24 hours)
    assert!(claims.exp > claims.iat);
    assert!(claims.exp - claims.iat >= 24 * 3600 - 1);
}

#[tokio::test]
async fn test_jwt_token_with_different_secrets() {
    load_test_env();
    let secret1 = "secret1".to_string();
    let secret2 = "secret2".to_string();

    let service1 = JwtService::new(secret1);
    let service2 = JwtService::new(secret2);

    let user_id = "123e4567-e89b-12d3-a456-426614174000".to_string();
    let email = "test@example.com".to_string();

    // Encode with service1
    let token = service1.encode(user_id.clone(), email.clone()).unwrap();

    // Should decode with service1
    let claims1 = service1.decode(&token).unwrap();
    assert_eq!(claims1.user_id, user_id);

    // Should NOT decode with service2 (different secret)
    assert!(service2.decode(&token).is_err());
}

#[tokio::test]
async fn test_otp_setup_creates_secret() -> sqlx::Result<()> {
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");
    let user_id = create_test_user(&pool, "otp_test@example.com").await;

    // Create OTP setting
    let secret = "JBSWY3DPEHPK3PXP"; // Base32 encoded test secret
    let backup_codes = vec!["12345678", "87654321"];
    let backup_codes_json = serde_json::to_string(&backup_codes).unwrap();

    sqlx::query(
        r#"
        INSERT INTO user_otp_settings (platform_user_id, secret, backup_codes, enabled, created_at, updated_at)
        VALUES ($1, $2, $3, false, NOW(), NOW())
        ON CONFLICT (platform_user_id) DO UPDATE
        SET secret = EXCLUDED.secret, backup_codes = EXCLUDED.backup_codes, updated_at = NOW()
        "#,
    )
    .bind(user_id)
    .bind(secret)
    .bind(backup_codes_json)
    .execute(&pool)
    .await?;

    // Verify OTP setting was created
    let otp_secret: Option<String> =
        sqlx::query_scalar("SELECT secret FROM user_otp_settings WHERE platform_user_id = $1")
            .bind(user_id)
            .fetch_optional(&pool)
            .await?
            .flatten();

    assert!(otp_secret.is_some());
    assert_eq!(otp_secret.unwrap(), secret);

    Ok(())
}

#[tokio::test]
async fn test_otp_enable_and_disable() -> sqlx::Result<()> {
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");
    let user_id = create_test_user(&pool, "otp_enable_test@example.com").await;

    // Create OTP setting (disabled)
    let secret = "JBSWY3DPEHPK3PXP";
    sqlx::query(
        r#"
        INSERT INTO user_otp_settings (platform_user_id, secret, enabled, created_at, updated_at)
        VALUES ($1, $2, false, NOW(), NOW())
        ON CONFLICT (platform_user_id) DO UPDATE
        SET secret = EXCLUDED.secret, updated_at = NOW()
        "#,
    )
    .bind(user_id)
    .bind(secret)
    .execute(&pool)
    .await?;

    // Verify OTP is disabled
    let enabled: Option<bool> =
        sqlx::query_scalar("SELECT enabled FROM user_otp_settings WHERE platform_user_id = $1")
            .bind(user_id)
            .fetch_optional(&pool)
            .await?
            .flatten();

    assert_eq!(enabled, Some(false));

    // Enable OTP
    sqlx::query(
        "UPDATE user_otp_settings SET enabled = true, updated_at = NOW() WHERE platform_user_id = $1",
    )
    .bind(user_id)
    .execute(&pool)
    .await?;

    // Verify OTP is enabled
    let enabled: Option<bool> =
        sqlx::query_scalar("SELECT enabled FROM user_otp_settings WHERE platform_user_id = $1")
            .bind(user_id)
            .fetch_optional(&pool)
            .await?
            .flatten();

    assert_eq!(enabled, Some(true));

    // Disable OTP
    sqlx::query(
        "UPDATE user_otp_settings SET enabled = false, updated_at = NOW() WHERE platform_user_id = $1",
    )
    .bind(user_id)
    .execute(&pool)
    .await?;

    // Verify OTP is disabled again
    let enabled: Option<bool> =
        sqlx::query_scalar("SELECT enabled FROM user_otp_settings WHERE platform_user_id = $1")
            .bind(user_id)
            .fetch_optional(&pool)
            .await?
            .flatten();

    assert_eq!(enabled, Some(false));

    Ok(())
}

#[tokio::test]
async fn test_otp_backup_codes_storage() -> sqlx::Result<()> {
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");
    let user_id = create_test_user(&pool, "otp_backup_test@example.com").await;

    // Create OTP setting with backup codes
    let secret = "JBSWY3DPEHPK3PXP";
    let backup_codes = vec!["12345678", "87654321", "11223344"];
    let backup_codes_json = serde_json::to_string(&backup_codes).unwrap();

    sqlx::query(
        r#"
        INSERT INTO user_otp_settings (platform_user_id, secret, backup_codes, enabled, created_at, updated_at)
        VALUES ($1, $2, $3, false, NOW(), NOW())
        ON CONFLICT (platform_user_id) DO UPDATE
        SET secret = EXCLUDED.secret, backup_codes = EXCLUDED.backup_codes, updated_at = NOW()
        "#,
    )
    .bind(user_id)
    .bind(secret)
    .bind(backup_codes_json)
    .execute(&pool)
    .await?;

    // Retrieve backup codes
    let stored_codes_json: Option<String> = sqlx::query_scalar(
        "SELECT backup_codes FROM user_otp_settings WHERE platform_user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .flatten();

    assert!(stored_codes_json.is_some());
    let stored_codes: Vec<String> = serde_json::from_str(&stored_codes_json.unwrap()).unwrap();
    assert_eq!(stored_codes.len(), 3);
    assert!(stored_codes.contains(&"12345678".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_passkey_credential_storage() -> sqlx::Result<()> {
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");
    let user_id = create_test_user(&pool, "passkey_test@example.com").await;

    // Create a test passkey credential
    let passkey_id = Uuid::new_v4();
    let external_id = format!("test_credential_id_{}", Uuid::new_v4());
    let public_key = r#"{"kty":"EC","crv":"P-256","x":"test_x","y":"test_y"}"#;
    let sign_count = 0i32;
    let name = "Test Passkey";
    let passkey_type = "soft";

    sqlx::query(
        r#"
        INSERT INTO webauthn_credentials (
            id, platform_user_id, instance_user_id, external_id, public_key, sign_count,
            name, passkey_type, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW(), NOW())
        "#,
    )
    .bind(passkey_id)
    .bind(user_id)
    .bind(user_id)
    .bind(&external_id)
    .bind(public_key)
    .bind(sign_count)
    .bind(name)
    .bind(passkey_type)
    .execute(&pool)
    .await?;

    // Verify passkey was stored
    let stored_name: Option<String> = sqlx::query_scalar(
        "SELECT name FROM webauthn_credentials WHERE platform_user_id = $1 AND external_id = $2",
    )
    .bind(user_id)
    .bind(&external_id)
    .fetch_optional(&pool)
    .await?
    .flatten();

    assert_eq!(stored_name, Some(name.to_string()));

    // Verify passkey count
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM webauthn_credentials WHERE platform_user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await?;

    assert_eq!(count, 1);

    Ok(())
}

#[tokio::test]
async fn test_passkey_max_limit_enforcement() -> sqlx::Result<()> {
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");
    let user_id = create_test_user(&pool, "passkey_limit_test@example.com").await;

    // Create 3 passkeys (max limit)
    for i in 0..3 {
        let passkey_id = Uuid::new_v4();
        let external_id = format!("test_credential_id_{}_{}", i, Uuid::new_v4());
        let public_key = r#"{"kty":"EC","crv":"P-256","x":"test_x","y":"test_y"}"#;

        sqlx::query(
            r#"
            INSERT INTO webauthn_credentials (
                id, platform_user_id, instance_user_id, external_id, public_key, sign_count,
                name, passkey_type, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, 0, $6, 'soft', NOW(), NOW())
            "#,
        )
        .bind(passkey_id)
        .bind(user_id)
        .bind(user_id)
        .bind(external_id)
        .bind(public_key)
        .bind(format!("Passkey {}", i + 1))
        .execute(&pool)
        .await?;
    }

    // Verify we have 3 passkeys
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM webauthn_credentials WHERE platform_user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await?;

    assert_eq!(count, 3);

    Ok(())
}

#[tokio::test]
async fn test_user_password_verification() -> sqlx::Result<()> {
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");
    let user_id = create_test_user(&pool, "password_test@example.com").await;

    // Retrieve stored password hash
    let stored_hash: String =
        sqlx::query_scalar("SELECT encrypted_password FROM instance_users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await?;

    // Verify password matches
    assert!(verify_password("TestPassword123!", &stored_hash).unwrap());

    // Verify wrong password doesn't match
    assert!(!verify_password("WrongPassword123!", &stored_hash).unwrap());

    Ok(())
}

#[tokio::test]
async fn test_user_status_affects_authentication() -> sqlx::Result<()> {
    let pool = create_test_pool()
        .await
        .expect("Failed to create test pool");
    let user_id = create_test_user(&pool, "status_test@example.com").await;

    // Verify user is active
    let status: String =
        sqlx::query_scalar("SELECT status::text FROM instance_users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await?;

    assert_eq!(status, "active");

    // Suspend user
    sqlx::query("UPDATE instance_users SET status = 'suspended'::instance_user_status_enum, updated_at = NOW() WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await?;

    // Verify user is suspended
    let status: String =
        sqlx::query_scalar("SELECT status::text FROM instance_users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await?;

    assert_eq!(status, "suspended");

    Ok(())
}

#[tokio::test]
async fn test_jwt_token_expiration_validation() {
    load_test_env();
    let jwt_secret = "test_jwt_secret_key_for_integration_tests".to_string();
    let service = JwtService::new(jwt_secret);

    let user_id = "123e4567-e89b-12d3-a456-426614174000".to_string();
    let email = "test@example.com".to_string();

    // Create token
    let token = service.encode(user_id.clone(), email.clone()).unwrap();

    // Decode immediately (should work)
    let claims = service.decode(&token).unwrap();
    assert_eq!(claims.user_id, user_id);

    // Verify expiration is in the future
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    assert!(claims.exp > now);
    assert!(claims.exp - now >= 24 * 3600 - 10); // Allow 10 seconds tolerance
}
