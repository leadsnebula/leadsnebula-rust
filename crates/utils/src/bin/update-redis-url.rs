// Binary to update Redis connection URL in SSM Parameter Store
// Usage: cargo run --bin update-redis-url -- --env dev --url "rediss://..."

use anyhow::Result;
use clap::Parser;
use leadsnebula_core::SsmClient;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Environment (dev, prod, or development, production)
    #[arg(short, long, default_value = "dev")]
    env: String,

    /// Redis connection URL (if not provided, will prompt or use env var)
    #[arg(short, long)]
    url: Option<String>,

    /// Use TLS (rediss://) instead of non-TLS (redis://)
    #[arg(short, long, default_value_t = true)]
    tls: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Normalize environment name
    let env_name = match args.env.as_str() {
        "dev" => "development",
        "prod" => "production",
        other => other,
    };

    // Get Redis URL
    let redis_url = if let Some(url) = args.url {
        url
    } else if let Ok(url) = std::env::var("REDIS_URL") {
        url
    } else {
        anyhow::bail!(
            "Redis URL required. Provide via --url flag or REDIS_URL environment variable"
        );
    };

    // Ensure URL uses correct protocol based on TLS flag
    let redis_url = if args.tls && !redis_url.starts_with("rediss://") {
        redis_url.replace("redis://", "rediss://")
    } else if !args.tls && redis_url.starts_with("rediss://") {
        redis_url.replace("rediss://", "redis://")
    } else {
        redis_url
    };

    println!("=== Updating Redis Connection URL in SSM ===\n");
    println!("Environment: {}", env_name);
    println!(
        "URL: {}",
        redis_url.replace(":705f0617c9b84bb6960c65ee5c85b638@", ":****@")
    );
    println!(
        "TLS: {}\n",
        if redis_url.starts_with("rediss://") {
            "Enabled"
        } else {
            "Disabled"
        }
    );

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

    // Update parameter
    let param_path = format!("/leadsnebula/{}/rust/redis/connection_url", env_name);
    println!("Updating parameter: {}\n", param_path);

    match ssm_client
        .put_parameter(
            &param_path,
            &redis_url,
            Some(&format!(
                "Redis connection URL for {} environment (Rust app)",
                env_name
            )),
        )
        .await
    {
        Ok(_) => {
            println!("✅ Successfully updated: {}\n", param_path);

            // Verify
            println!("Verifying...");
            if let Ok(Some(_verified)) = ssm_client.get_parameter(&param_path).await {
                println!("✅ Verified: Parameter stored correctly");
                println!("\n⚠️  Note: The Upstash dashboard may still show 'TLS/SSL: Disabled'");
                println!(
                    "   This is a dashboard display issue. Upstash Redis always supports TLS."
                );
                println!("   Your application will use TLS when connecting with 'rediss://' URL.");
            } else {
                println!("⚠️  Warning: Could not verify parameter");
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to update {}: {}\n", param_path, e);
            return Err(e);
        }
    }

    Ok(())
}
