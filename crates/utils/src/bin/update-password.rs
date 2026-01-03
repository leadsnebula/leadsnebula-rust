use anyhow::Context;
use leadsnebula_core::{PasswordHelper, SsmClient};
use sqlx::{Connection, PgConnection};
use std::env;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::from_filename(".env.local").ok();

    let email = env::var("ADMIN_EMAIL").context("ADMIN_EMAIL environment variable required")?;
    let password =
        env::var("ADMIN_PASSWORD").context("ADMIN_PASSWORD environment variable required")?;

    // Load database URL from SSM or environment (same as main app)
    let environment = env::var("ENVIRONMENT")
        .unwrap_or_else(|_| env::var("ENV").unwrap_or_else(|_| "development".to_string()));

    let ssm_client = match SsmClient::new().await {
        Ok(client) => Arc::new(client),
        Err(e) => {
            tracing::warn!(
                "SSM client initialization failed: {}. Falling back to environment variables.",
                e
            );
            Arc::new(SsmClient::dummy())
        }
    };

    let param_path = format!("/leadsnebula/{}/rust/db/connection_url", environment);
    let database_url = if let Some(url) = ssm_client.get_parameter(&param_path).await? {
        url
    } else {
        env::var("DATABASE_URL").context(
            "DATABASE_URL not found in SSM or environment variables. \
             Set DATABASE_URL environment variable or configure SSM parameter.",
        )?
    };

    let mut conn = PgConnection::connect(&database_url).await?;
    let email_lower = email.trim().to_lowercase();

    // Hash password
    let password_hash =
        PasswordHelper::hash_password(&password).context("Failed to hash password")?;

    // Verify the hash works
    let verify_test = PasswordHelper::verify_password(&password, &password_hash).unwrap_or(false);
    if !verify_test {
        eprintln!("⚠️  Warning: Generated hash does not verify correctly!");
    }

    // Update password
    let result = sqlx::query(
        r#"
        UPDATE instance_users
        SET encrypted_password = $1,
            last_password_change_at = NOW(),
            updated_at = NOW()
        WHERE LOWER(email) = $2
        "#,
    )
    .bind(&password_hash)
    .bind(&email_lower)
    .execute(&mut conn)
    .await?;

    if result.rows_affected() > 0 {
        println!("✅ Password updated for: {}", email);
        println!(
            "   Hash verification test: {}",
            if verify_test {
                "✅ Passed"
            } else {
                "❌ Failed"
            }
        );
    } else {
        println!("⚠️  No user found with email: {}", email);
    }

    Ok(())
}
