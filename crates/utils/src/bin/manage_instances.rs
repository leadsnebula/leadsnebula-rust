use anyhow::Result;
use clap::Parser;
use leadsnebula_core::services::database::create_pool;
use serde_json::json;
use sqlx::Row;
use std::fs::File;
use std::io::Write;
use std::process::Command;
use uuid::Uuid;

#[derive(Parser)]
struct Args {
    /// Instance ID to operate on
    #[arg(long)]
    instance_id: String,

    /// New name for the existing instance
    #[arg(long)]
    rename: Option<String>,

    /// Create test instance and user with this email
    #[arg(long, default_value = "test@leadsnebula.com")]
    test_user_email: String,

    /// Name for the new test instance
    #[arg(long, default_value = "Test Instance")]
    test_instance_name: String,

    /// Run validate.sh after operations
    #[arg(long)]
    run_tests: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();

    let args = Args::parse();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env.local or env");
    let pool = create_pool(&database_url).await?;

    let instance_id = Uuid::parse_str(&args.instance_id)?;

    // Backup publishers for instance
    let rows = sqlx::query("SELECT id, name, created_at FROM publishers WHERE instance_id = $1")
        .bind(instance_id)
        .fetch_all(&pool)
        .await?;

    let backup_path = format!("publishers_backup_{}.jsonl", instance_id);
    let mut f = File::create(&backup_path)?;
    for row in rows.iter() {
        let id: Uuid = row.get("id");
        let name: Option<String> = row.get("name");
        let created_at: Option<chrono::DateTime<chrono::Utc>> = row.get("created_at");
        let obj = json!({"id": id.to_string(), "name": name, "created_at": created_at});
        writeln!(f, "{}", obj)?;
    }
    println!("Backed up {} publisher rows to {}", rows.len(), backup_path);

    // Begin transaction for destructive ops
    let mut tx = pool.begin().await?;

    // Delete dependent objects referencing this instance
    // Delete publishers and cascade-related data
    println!(
        "Deleting publishers and dependents for instance {}...",
        instance_id
    );

    // Delete api_keys if exists
    let exists_api_keys: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'api_keys')",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(false);
    if exists_api_keys {
        sqlx::query("DELETE FROM api_keys WHERE publisher_id IN (SELECT id FROM publishers WHERE instance_id = $1)")
            .bind(instance_id)
            .execute(&mut *tx)
            .await?;
    }

    // Delete publishers
    let res = sqlx::query("DELETE FROM publishers WHERE instance_id = $1")
        .bind(instance_id)
        .execute(&mut *tx)
        .await?;
    println!("Deleted {} publishers", res.rows_affected());

    // Delete buyers, campaigns, ping_trees that reference the instance
    let _ = sqlx::query("DELETE FROM campaigns WHERE instance_id = $1")
        .bind(instance_id)
        .execute(&mut *tx)
        .await?;
    let _ = sqlx::query("DELETE FROM ping_trees WHERE instance_id = $1")
        .bind(instance_id)
        .execute(&mut *tx)
        .await?;
    let _ = sqlx::query("DELETE FROM buyers WHERE instance_id = $1")
        .bind(instance_id)
        .execute(&mut *tx)
        .await?;

    // Commit transaction
    tx.commit().await?;
    println!("Deletion transaction committed.");

    // Rename instance if requested
    if let Some(new_name) = args.rename {
        let result = sqlx::query("UPDATE instances SET name = $1 WHERE id = $2")
            .bind(new_name)
            .bind(instance_id)
            .execute(&pool)
            .await?;
        println!(
            "Renamed instance; rows affected: {}",
            result.rows_affected()
        );
    }

    // Create test instance_user and instance
    let test_user_id = Uuid::new_v4();
    let test_instance_id = Uuid::new_v4();

    let exists_instance_users: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'instance_users')",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if exists_instance_users {
        sqlx::query("INSERT INTO instance_users (id, email, status, created_at, updated_at) VALUES ($1, $2, 'active', NOW(), NOW()) ON CONFLICT (email) DO NOTHING")
            .bind(test_user_id)
            .bind(&args.test_user_email)
            .execute(&pool)
            .await?;
        println!(
            "Ensured instance_user {} exists (id={})",
            args.test_user_email, test_user_id
        );

        sqlx::query("INSERT INTO instances (id, name, instance_user_id, created_at, updated_at) VALUES ($1, $2, $3, NOW(), NOW())")
            .bind(test_instance_id)
            .bind(&args.test_instance_name)
            .bind(test_user_id)
            .execute(&pool)
            .await?;
        println!(
            "Created test instance '{}' id={} for user {}",
            args.test_instance_name, test_instance_id, args.test_user_email
        );
    } else {
        // Fallback: try generic users table
        let user_id = Uuid::new_v4();
        sqlx::query("INSERT INTO users (id, email, name, status, created_at, updated_at) VALUES ($1, $2, $3, 'active', NOW(), NOW()) ON CONFLICT (email) DO NOTHING")
            .bind(user_id)
            .bind(&args.test_user_email)
            .bind("Test User")
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO instances (id, name, owner_id, created_at, updated_at) VALUES ($1, $2, $3, NOW(), NOW())")
            .bind(test_instance_id)
            .bind(&args.test_instance_name)
            .bind(user_id)
            .execute(&pool)
            .await?;
        println!(
            "Created test instance '{}' id={} for user {} (users table path)",
            args.test_instance_name, test_instance_id, args.test_user_email
        );
    }

    // Optionally run tests with INSTANCE_ID env var set
    if args.run_tests {
        println!(
            "Running ./validate.sh with INSTANCE_ID={} ...",
            test_instance_id
        );
        let status = Command::new("bash")
            .arg("-lc")
            .arg(format!("INSTANCE_ID={} ./validate.sh", test_instance_id))
            .status()?;
        println!("validate.sh exited with: {}", status);
    }

    Ok(())
}
