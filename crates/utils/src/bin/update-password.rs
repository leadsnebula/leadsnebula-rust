use anyhow::Result;
use clap::Parser;
use leadsnebula_core::auth::hash_password;
use leadsnebula_core::services::database::create_pool;
use std::io::{self, Write};
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

    let args = Args::parse();

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL environment variable must be set");

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

