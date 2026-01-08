use anyhow::Result;
use clap::Parser;
use leadsnebula_core::auth::hash_password;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use sqlx::Row;
use std::io::{self, Write};
use std::sync::Arc;
use tracing::info;

#[derive(Parser)]
#[command(name = "update-password")]
#[command(about = "Update a user's password")]
struct Args {
    #[arg(short, long)]
    email: String,
    #[arg(short, long)]
    password: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Load .env.local first for local development (highest priority)
    let env_loaded = dotenv::from_filename(".env.local").is_ok();
    if !env_loaded {
        let _ = dotenv::dotenv();
    }

    if env_loaded {
        tracing::info!("Loaded environment from .env.local (local development mode)");
    }

    let args = Args::parse();

    // Load DATABASE_URL from SSM (like main app) or fall back to env var
    let environment = std::env::var("ENVIRONMENT")
        .or_else(|_| std::env::var("ENV"))
        .unwrap_or_else(|_| "development".to_string());

    let env_normalized = leadsnebula_core::normalize_env_for_ssm(&environment);
    let ssm = Arc::new(SsmService::new(environment.clone(), None).await?);
    let config_path = format!("/leadsnebula/{}/rust/", env_normalized);
    let mut params = ssm.get_parameters_by_path(&config_path).await?;

    // For dev environment, also fetch from prod path as fallback
    if env_normalized == "dev" {
        let prod_params = ssm
            .get_parameters_by_path("/leadsnebula/prod/rust/")
            .await?;
        params.extend(prod_params);
    }

    let database_url = params
        .get(&format!(
            "/leadsnebula/{}/rust/db/connection_url",
            env_normalized
        ))
        .cloned()
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "DATABASE_URL not found in SSM at /leadsnebula/{}/rust/db/connection_url and not in environment variables",
                env_normalized
            )
        })?;

    let pool = create_pool(&database_url).await?;

    let password = if let Some(pwd) = args.password {
        pwd
    } else {
        print!("Enter new password: ");
        io::stdout().flush()?;
        let mut pwd = String::new();
        io::stdin().read_line(&mut pwd)?;
        pwd.trim().to_string()
    };

    let encrypted_password = hash_password(&password)?;

    // First, check which user(s) exist
    let users: Vec<(uuid::Uuid, String)> =
        sqlx::query("SELECT id, email FROM instance_users WHERE LOWER(email) = LOWER($1)")
            .bind(&args.email)
            .map(|row: sqlx::postgres::PgRow| (row.get(0), row.get(1)))
            .fetch_all(&pool)
            .await?;

    if users.is_empty() {
        return Err(anyhow::anyhow!("User not found: {}", args.email));
    }

    if users.len() > 1 {
        info!(
            "Warning: Multiple users found with email {}. Updating all of them:",
            args.email
        );
        for (id, email) in &users {
            info!("  - ID: {}, Email: {}", id, email);
        }
    }

    let rows_affected = sqlx::query(
        "UPDATE instance_users SET encrypted_password = $1, updated_at = $2 WHERE LOWER(email) = LOWER($3)",
    )
    .bind(&encrypted_password)
    .bind(chrono::Utc::now())
    .bind(&args.email)
    .execute(&pool)
    .await?
    .rows_affected();

    if rows_affected > 0 {
        info!("Password updated successfully for: {}", args.email);
    } else {
        return Err(anyhow::anyhow!("User not found: {}", args.email));
    }

    Ok(())
}
