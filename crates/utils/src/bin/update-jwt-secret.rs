use anyhow::Result;
use clap::Parser;
use leadsnebula_core::SsmClient;
use rand::Rng;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Environment (production, staging, development) - defaults to production
    #[arg(short, long, default_value = "production")]
    environment: String,

    /// JWT secret key (if not provided, will generate a new one)
    #[arg(long)]
    jwt_secret: Option<String>,

    /// Generate new JWT secret (ignored if --jwt-secret is provided)
    #[arg(long, default_value = "false")]
    generate: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env.local if it exists (for local development)
    let _ = dotenvy::from_filename(".env.local").ok();

    let args = Args::parse();

    // Normalize environment name
    let env_name = match args.environment.as_str() {
        "prod" | "production" => "production",
        "staging" => "staging",
        "dev" | "development" => "development",
        _ => {
            eprintln!("Error: Invalid environment. Must be: production, staging, or development");
            std::process::exit(1);
        }
    };

    println!("=== Updating JWT Secret in SSM ===\n");
    println!("Environment: {}\n", env_name);

    // Check for AWS credentials first
    let has_aws_creds = std::env::var("AWS_ACCESS_KEY_ID").is_ok()
        && std::env::var("AWS_SECRET_ACCESS_KEY").is_ok();
    if !has_aws_creds {
        eprintln!("❌ AWS credentials not found!");
        eprintln!("\nPlease set AWS credentials before running this script:");
        eprintln!("  export AWS_ACCESS_KEY_ID=<your-access-key>");
        eprintln!("  export AWS_SECRET_ACCESS_KEY=<your-secret-key>");
        eprintln!("  export AWS_REGION=us-east-1  # optional, defaults to us-east-1");
        eprintln!("\nOr add them to .env.local and load with: dotenvy");
        std::process::exit(1);
    }

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
    } else if args.generate {
        println!("Generating new JWT secret...");
        let mut rng = rand::thread_rng();
        let secret: String = (0..64)
            .map(|_| rng.sample(rand::distributions::Alphanumeric) as char)
            .collect();
        println!("✅ Generated new JWT secret\n");
        secret
    } else {
        eprintln!("Error: Must provide either --jwt-secret or --generate");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  1. Provide existing JWT secret: --jwt-secret <secret>");
        eprintln!("  2. Generate new JWT secret: --generate");
        std::process::exit(1);
    };

    let jwt_path = format!("/leadsnebula/{}/rust/jwt/secret_key", env_name);

    println!("⚠️  WARNING: Updating JWT secret will invalidate all existing tokens!");
    println!("   All users will need to log in again after this change.\n");
    println!("Storing JWT secret at: {}\n", jwt_path);

    // Store JWT secret
    match ssm_client
        .put_parameter(
            &jwt_path,
            &jwt_secret,
            Some(&format!(
                "JWT signing secret for {} environment (Rust app)",
                env_name
            )),
        )
        .await
    {
        Ok(_) => println!("✅ Stored: {}\n", jwt_path),
        Err(e) => {
            eprintln!("❌ Failed to store {}: {}\n", jwt_path, e);
            return Err(e);
        }
    }

    // Verify
    println!("--- Verifying ---");
    let jwt_check = ssm_client.get_parameter(&jwt_path).await?;

    if jwt_check.is_some() {
        println!("JWT Secret: ✅");
        println!("\n✅ JWT secret stored successfully!");
        println!("\n⚠️  Remember: All existing JWT tokens are now invalid.");
        println!("   Users will need to log in again to get new tokens.");
    } else {
        println!("JWT Secret: ❌");
        println!("\n⚠️  Warning: JWT secret may not have been stored correctly");
    }

    Ok(())
}
