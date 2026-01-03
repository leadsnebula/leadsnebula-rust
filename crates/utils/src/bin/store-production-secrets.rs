use anyhow::Result;
use clap::Parser;
use leadsnebula_core::SsmClient;
use rand::Rng;
use std::env;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Database URL (or set PROD_DATABASE_URL env var)
    #[arg(long)]
    database_url: Option<String>,

    /// JWT secret key (if not provided, will generate a new one)
    #[arg(long)]
    jwt_secret: Option<String>,

    /// Generate new JWT secret (ignored if --jwt-secret is provided)
    #[arg(long, default_value = "false")]
    generate_jwt: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    println!("=== Storing Production Secrets in SSM ===\n");

    // Initialize SSM client
    let ssm_client = match SsmClient::new().await {
        Ok(client) => {
            println!("✅ Connected to AWS SSM\n");
            client
        }
        Err(e) => {
            eprintln!("❌ Failed to connect to AWS SSM: {}", e);
            eprintln!("\nPlease ensure AWS credentials are configured:");
            eprintln!("  - Set AWS_ACCESS_KEY_ID");
            eprintln!("  - Set AWS_SECRET_ACCESS_KEY");
            eprintln!("  - Set AWS_REGION (default: us-east-1)");
            return Err(e);
        }
    };

    // Get or generate JWT secret
    let jwt_secret = if let Some(secret) = args.jwt_secret {
        println!("Using provided JWT secret\n");
        secret
    } else if args.generate_jwt {
        println!("Generating new JWT secret...");
        let mut rng = rand::thread_rng();
        let secret: String = (0..64)
            .map(|_| rng.sample(rand::distributions::Alphanumeric) as char)
            .collect();
        println!("✅ Generated new JWT secret\n");
        secret
    } else {
        eprintln!("Error: Must provide either --jwt-secret or --generate-jwt");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  1. Provide existing JWT secret: --jwt-secret <secret>");
        eprintln!("  2. Generate new JWT secret: --generate-jwt");
        std::process::exit(1);
    };

    // Get database URL
    let database_url = args
        .database_url
        .or_else(|| env::var("PROD_DATABASE_URL").ok())
        .expect("Database URL required. Provide via --database-url or PROD_DATABASE_URL env var");

    // Production paths
    let jwt_path = "/leadsnebula/production/rust/jwt/secret_key";
    let db_path = "/leadsnebula/production/rust/db/connection_url";

    // Store JWT secret
    println!("--- Storing JWT Secret ---");
    match ssm_client
        .put_parameter(
            jwt_path,
            &jwt_secret,
            Some("JWT signing secret for production environment (Rust app)"),
        )
        .await
    {
        Ok(_) => println!("✅ Stored: {}\n", jwt_path),
        Err(e) => {
            eprintln!("❌ Failed to store {}: {}\n", jwt_path, e);
            return Err(e);
        }
    }

    // Store database URL
    println!("--- Storing Database URL ---");
    match ssm_client
        .put_parameter(
            db_path,
            &database_url,
            Some("Neon PostgreSQL connection URL for production environment (Rust app)"),
        )
        .await
    {
        Ok(_) => println!("✅ Stored: {}\n", db_path),
        Err(e) => {
            eprintln!("❌ Failed to store {}: {}\n", db_path, e);
            return Err(e);
        }
    }

    // Verify
    println!("--- Verifying ---");
    let jwt_check = ssm_client.get_parameter(jwt_path).await?;
    let db_check = ssm_client.get_parameter(db_path).await?;

    println!(
        "JWT Secret: {}",
        if jwt_check.is_some() { "✅" } else { "❌" }
    );
    println!(
        "Database URL: {}",
        if db_check.is_some() { "✅" } else { "❌" }
    );

    if jwt_check.is_some() && db_check.is_some() {
        println!("\n✅ All production secrets stored successfully!");
    } else {
        println!("\n⚠️  Warning: Some secrets may not have been stored correctly");
    }

    Ok(())
}
