use anyhow::Result;
use clap::Parser;
use leadsnebula_core::auth::hash_password;
use leadsnebula_core::services::database::create_pool;
use std::io::{self, Write};
use tracing::info;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "create-user")]
#[command(about = "Create a new user in the database")]
struct Args {
    #[arg(short, long)]
    email: String,
    #[arg(short, long)]
    password: Option<String>,
    #[arg(short, long)]
    first_name: Option<String>,
    #[arg(short, long)]
    last_name: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");

    let pool = create_pool(&database_url).await?;

    let password = if let Some(pwd) = args.password {
        pwd
    } else {
        print!("Enter password: ");
        io::stdout().flush()?;
        let mut pwd = String::new();
        io::stdin().read_line(&mut pwd)?;
        pwd.trim().to_string()
    };

    let encrypted_password = hash_password(&password)?;

    let user_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        INSERT INTO instance_users (id, email, encrypted_password, first_name, last_name, status, confirmed_at, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, 'active', $6, $7, $8)
        "#,
    )
    .bind(user_id)
    .bind(&args.email)
    .bind(&encrypted_password)
    .bind(&args.first_name)
    .bind(&args.last_name)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await?;

    info!("User created successfully: {}", args.email);

    Ok(())
}
