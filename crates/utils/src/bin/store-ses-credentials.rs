use anyhow::Result;
use clap::Parser;
use leadsnebula_core::email::store_ses_credentials_in_ssm;
use leadsnebula_core::ssm::SsmClient;
use std::env;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Access key ID (or set SES_ACCESS_KEY_ID env var)
    #[arg(short, long)]
    access_key_id: Option<String>,

    /// Secret access key (or set SES_SECRET_ACCESS_KEY env var)
    #[arg(short, long)]
    secret_access_key: Option<String>,

    /// AWS region (defaults to us-east-1, or set SES_REGION env var)
    #[arg(short, long)]
    region: Option<String>,

    /// Environment (prod, staging, dev) - defaults to prod
    #[arg(short, long, default_value = "prod")]
    environment: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Get credentials from args or environment variables
    let access_key_id = args
        .access_key_id
        .or_else(|| env::var("SES_ACCESS_KEY_ID").ok())
        .or_else(|| env::var("AWS_ACCESS_KEY_ID").ok());

    let secret_access_key = args
        .secret_access_key
        .or_else(|| env::var("SES_SECRET_ACCESS_KEY").ok())
        .or_else(|| env::var("AWS_SECRET_ACCESS_KEY").ok());

    let region = args
        .region
        .or_else(|| env::var("SES_REGION").ok())
        .or_else(|| env::var("AWS_REGION").ok())
        .unwrap_or_else(|| "us-east-1".to_string());

    if access_key_id.is_none() || secret_access_key.is_none() {
        eprintln!("Error: Missing credentials");
        eprintln!();
        eprintln!("Provide credentials via:");
        eprintln!("  - Command line: --access-key-id <key> --secret-access-key <secret>");
        eprintln!("  - Environment variables: SES_ACCESS_KEY_ID and SES_SECRET_ACCESS_KEY");
        eprintln!("  - Or fallback: AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY");
        eprintln!();
        eprintln!("Example:");
        eprintln!("  cargo run --bin store-ses-credentials -- --access-key-id AKIA... --secret-access-key ...");
        eprintln!("  Or:");
        eprintln!("  SES_ACCESS_KEY_ID=AKIA... SES_SECRET_ACCESS_KEY=... cargo run --bin store-ses-credentials");
        std::process::exit(1);
    }

    println!("Storing SES credentials in SSM...");
    println!("Environment: {}", args.environment);
    println!("Region: {}", region);
    println!("Path: /leadsnebula/{}/shared/aws/ses/", args.environment);
    println!();

    // Initialize SSM client
    let ssm_client = SsmClient::new().await?;

    // Store credentials
    store_ses_credentials_in_ssm(
        &ssm_client,
        &args.environment,
        &access_key_id.unwrap(),
        &secret_access_key.unwrap(),
        Some(&region),
    )
    .await?;

    println!("✅ Successfully stored SES credentials in SSM!");
    println!();
    println!("The Rust API will now automatically use these credentials from SSM.");

    Ok(())
}
