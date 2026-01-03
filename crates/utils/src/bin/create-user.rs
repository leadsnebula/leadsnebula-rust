use anyhow::Context;
use chrono::Utc;
use leadsnebula_core::{PasswordHelper, SsmClient};
use sqlx::{Connection, PgConnection};
use std::env;
use std::sync::Arc;
use uuid::Uuid;

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

    let first_name = env::var("ADMIN_FIRST_NAME").unwrap_or_else(|_| "Admin".to_string());
    let last_name = env::var("ADMIN_LAST_NAME").unwrap_or_else(|_| "User".to_string());
    let make_admin = env::var("MAKE_ADMIN")
        .unwrap_or_else(|_| "true".to_string())
        .parse::<bool>()
        .unwrap_or(true);

    let mut conn = PgConnection::connect(&database_url).await?;
    let email_lower = email.trim().to_lowercase();

    // Check if user already exists
    let existing = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM instance_users WHERE LOWER(email) = $1
        "#,
    )
    .bind(&email_lower)
    .fetch_one(&mut conn)
    .await?;

    if existing > 0 {
        println!("⚠️  User with email {} already exists", email);
        println!("   Use update-password utility to change password");
        return Ok(());
    }

    // Hash password
    let password_hash =
        PasswordHelper::hash_password(&password).context("Failed to hash password")?;

    // Verify the hash works
    let verify_test = PasswordHelper::verify_password(&password, &password_hash).unwrap_or(false);
    if !verify_test {
        eprintln!("⚠️  Warning: Generated hash does not verify correctly!");
    }

    let user_id = Uuid::new_v4();
    let now = Utc::now();

    // Create user with active status (skip email verification)
    sqlx::query(
        r#"
        INSERT INTO instance_users (
            id, email, encrypted_password, first_name, last_name,
            status, created_at, updated_at, confirmed_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(user_id)
    .bind(&email_lower)
    .bind(&password_hash)
    .bind(&first_name)
    .bind(&last_name)
    .bind("active") // Active status, skip email verification
    .bind(now)
    .bind(now)
    .bind(now) // confirmed_at - auto-confirm
    .execute(&mut conn)
    .await
    .context("Failed to create user")?;

    println!("✅ User created: {}", email);
    println!("   User ID: {}", user_id);
    println!(
        "   Hash verification test: {}",
        if verify_test {
            "✅ Passed"
        } else {
            "❌ Failed"
        }
    );

    // Optionally create admin role if instances table exists
    if make_admin {
        // Check if instances table exists and create a default instance
        let instance_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'instances')",
        )
        .fetch_one(&mut conn)
        .await?;

        if instance_exists {
            // Check if user already has an instance
            let instance_id: Option<Uuid> =
                sqlx::query_scalar("SELECT id FROM instances WHERE instance_user_id = $1 LIMIT 1")
                    .bind(user_id)
                    .fetch_optional(&mut conn)
                    .await?;

            let instance_id = if let Some(id) = instance_id {
                id
            } else {
                // Create a default instance
                let new_instance_id = Uuid::new_v4();
                sqlx::query(
                    r#"
                    INSERT INTO instances (id, name, instance_user_id, payment_status, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, $5, $6)
                    "#
                )
                .bind(new_instance_id)
                .bind("Default Instance")
                .bind(user_id)
                .bind("active")
                .bind(now)
                .bind(now)
                .execute(&mut conn)
                .await
                .context("Failed to create instance")?;

                println!("✅ Default instance created: {}", new_instance_id);
                new_instance_id
            };

            // Check if instance_user_roles table exists
            let roles_table_exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'instance_user_roles')"
            )
            .fetch_one(&mut conn)
            .await?;

            if roles_table_exists {
                // Check if admin role already exists
                let role_exists = sqlx::query_scalar::<_, i64>(
                    r#"
                    SELECT COUNT(*) FROM instance_user_roles 
                    WHERE instance_user_id = $1 AND instance_id = $2 AND role = 'admin'
                    "#,
                )
                .bind(user_id)
                .bind(instance_id)
                .fetch_one(&mut conn)
                .await?;

                if role_exists == 0 {
                    sqlx::query(
                        r#"
                        INSERT INTO instance_user_roles (instance_user_id, instance_id, role, created_at, updated_at)
                        VALUES ($1, $2, $3, $4, $5)
                        "#
                    )
                    .bind(user_id)
                    .bind(instance_id)
                    .bind("admin")
                    .bind(now)
                    .bind(now)
                    .execute(&mut conn)
                    .await
                    .context("Failed to create admin role")?;

                    println!("✅ Admin role added");
                } else {
                    println!("✅ Admin role already exists");
                }
            }
        }
    }

    println!("\n🎉 User setup complete!");
    println!("   Email: {}", email);
    println!("   You can now log in with these credentials");

    Ok(())
}
