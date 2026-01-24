use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use sqlx::migrate::Migrator;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let env_loaded = dotenvy::from_filename(".env.local").is_ok();
    if !env_loaded {
        let _ = dotenvy::dotenv();
    }

    let environment = std::env::var("ENVIRONMENT")
        .or_else(|_| std::env::var("ENV"))
        .unwrap_or_else(|_| "development".to_string());

    let env_normalized = leadsnebula_core::normalize_env_for_ssm(&environment);
    let ssm = Arc::new(SsmService::new(environment.clone(), None).await?);
    let config_path = format!("/leadsnebula/{}/rust/", env_normalized);
    let params = ssm.get_parameters_by_path(&config_path).await?;

    let database_url = params
        .get(&format!(
            "/leadsnebula/{}/rust/db/connection_url",
            env_normalized
        ))
        .cloned()
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| anyhow::anyhow!("DATABASE_URL not found"))?;

    let pool = create_pool(&database_url).await?;

    // Find migrations directory
    let migrations_dir = Path::new("migrations");
    let migrations_path = if !migrations_dir.exists() {
        let crate_root = env!("CARGO_MANIFEST_DIR");
        Path::new(crate_root)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("migrations")
    } else {
        migrations_dir.to_path_buf()
    };

    if !migrations_path.exists() {
        eprintln!(
            "❌ Migrations directory not found: {}",
            migrations_path.display()
        );
        std::process::exit(1);
    }

    // Create migrator to get all migrations
    let migrator = Migrator::new(migrations_path.as_path()).await?;
    let all_migrations: Vec<_> = migrator.iter().collect();

    println!("📁 Found {} migration files", all_migrations.len());

    // Get applied migrations from database
    let applied_versions: HashSet<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success = true")
            .fetch_all(&pool)
            .await?
            .into_iter()
            .collect();

    println!(
        "✅ Found {} applied migrations in database",
        applied_versions.len()
    );

    // Find pending migrations
    let mut pending = Vec::new();
    for migration in &all_migrations {
        let version = migration.version;
        let name = migration.description.to_string();
        if !applied_versions.contains(&version) {
            pending.push((version, name));
        }
    }

    pending.sort_by_key(|(v, _)| *v);

    if pending.is_empty() {
        println!("\n✅ All migrations are applied!");
        println!("   Database is up to date with migration files");
        std::process::exit(0);
    } else {
        println!("\n❌ PENDING MIGRATIONS DETECTED:");
        for (version, name) in &pending {
            println!("   - {} (version: {})", name, version);
        }
        println!("\n⚠️  Run migrations to apply:");
        println!("   cargo run --bin run-migrations");
        std::process::exit(1);
    }
}
