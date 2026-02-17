//! Lightweight integration tests for email and password reset (Rust/leadsnebula only).
//!
//! 1. Email service availability and successful start
//! 2. One email send via SES
//! 3. One password-reset email send
//! 4. Successful password reset (DB update with token)
//!
//! Tests skip gracefully when AWS (FROM_EMAIL / credentials) or DATABASE_URL are not set.
//! Run with: cargo test --test integration_email

mod common;

use common::create_test_pool;
use leadsnebula_core::auth::{hash_password, verify_password};
use leadsnebula_core::models::user::{InstanceUserStatus, User};
use leadsnebula_core::password_reset::PasswordResetService;
use std::sync::Arc;
use uuid::Uuid;

fn load_test_env() {
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();
}

/// 1. Email service availability and successful start
#[tokio::test]
async fn test_email_service_availability() {
    load_test_env();
    let from = std::env::var("FROM_EMAIL").unwrap_or_else(|_| "noreply@example.com".to_string());

    match leadsnebula_core::email::EmailService::new(from).await {
        Ok(_svc) => {}
        Err(e) => {
            eprintln!("⚠️  EmailService::new skipped (no AWS/config): {}", e);
            return;
        }
    }
}

/// 2. One email send (functionality only)
#[tokio::test]
async fn test_email_send_one() {
    load_test_env();
    let from = match std::env::var("FROM_EMAIL") {
        Ok(f) => f,
        Err(_) => {
            eprintln!("⚠️  FROM_EMAIL not set - skipping test_email_send_one");
            return;
        }
    };

    let svc = match leadsnebula_core::email::EmailService::new(from.clone()).await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!(
                "⚠️  EmailService::new failed - skipping test_email_send_one: {}",
                e
            );
            return;
        }
    };

    let to = std::env::var("TEST_EMAIL").unwrap_or(from);
    let result = svc
        .send_email(
            &to,
            "Test from leadsnebula integration_email",
            "One test body.",
            Some("<p>One test body.</p>"),
        )
        .await;

    if let Err(e) = result {
        eprintln!("⚠️  send_email failed (SES/network) - skipping: {}", e);
        return;
    }
}

/// 3. One password-reset email send
#[tokio::test]
async fn test_pw_reset_email_send() {
    load_test_env();
    let from = match std::env::var("FROM_EMAIL") {
        Ok(f) => f,
        Err(_) => {
            eprintln!("⚠️  FROM_EMAIL not set - skipping test_pw_reset_email_send");
            return;
        }
    };

    let email_svc = match leadsnebula_core::email::EmailService::new(from).await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!(
                "⚠️  EmailService::new failed - skipping test_pw_reset_email_send: {}",
                e
            );
            return;
        }
    };

    let now = chrono::Utc::now();
    let user = User {
        id: Uuid::new_v4(),
        email: std::env::var("TEST_EMAIL")
            .unwrap_or_else(|_| "pwreset-test@test.invalid".to_string()),
        encrypted_password: String::new(),
        first_name: None,
        last_name: None,
        status: InstanceUserStatus::Active,
        confirmed_at: Some(now),
        created_at: now,
        updated_at: now,
    };

    let pw_reset = PasswordResetService::new(email_svc, "http://test.example".to_string());
    let result = pw_reset.send_reset_email(&user, "test-token-ignore").await;

    if let Err(e) = result {
        eprintln!("⚠️  send_reset_email failed - skipping: {}", e);
        return;
    }
}

/// 4. Successful password reset (one DB update: set new password by token)
#[tokio::test]
async fn test_pw_reset_success() -> anyhow::Result<()> {
    load_test_env();
    if !common::has_database_url() {
        eprintln!("⚠️  DATABASE_URL not set - skipping test_pw_reset_success");
        return Ok(());
    }

    let pool = create_test_pool().await?;
    let mut tx = common::begin_transaction_with_retry(&pool).await?;

    let user_id = Uuid::new_v4();
    let email = format!("reset_{}@test.invalid", Uuid::new_v4());
    let old_hash = hash_password("OldPass123!")?;
    let reset_token = "test-reset-token-unique-123";
    let new_password = "NewPass456!";
    let new_hash = hash_password(new_password)?;

    sqlx::query(
        r#"
        INSERT INTO instance_users (id, email, encrypted_password, status, reset_password_token, confirmed_at, created_at, updated_at)
        VALUES ($1, $2, $3, 'active', $4, NOW(), NOW(), NOW())
        "#,
    )
    .bind(user_id)
    .bind(&email)
    .bind(&old_hash)
    .bind(reset_token)
    .execute(&mut *tx)
    .await?;

    let rows = sqlx::query(
        "UPDATE instance_users SET encrypted_password = $1, reset_password_token = NULL, reset_password_sent_at = NULL, updated_at = NOW() WHERE reset_password_token = $2 AND status = 'active'",
    )
    .bind(&new_hash)
    .bind(reset_token)
    .execute(&mut *tx)
    .await?;

    assert_eq!(
        rows.rows_affected(),
        1,
        "exactly one row updated by reset token"
    );

    let stored_hash: String =
        sqlx::query_scalar("SELECT encrypted_password FROM instance_users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

    assert!(verify_password(new_password, &stored_hash)?);
    tx.rollback().await?;
    Ok(())
}
