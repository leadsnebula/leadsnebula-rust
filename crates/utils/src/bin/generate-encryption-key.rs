use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use clap::Parser;
use rand::RngCore;

#[derive(Parser)]
#[command(name = "generate-encryption-key")]
#[command(about = "Generate a secure 32-byte encryption key for API key encryption")]
struct Args {
    #[arg(short, long, default_value = "dev")]
    environment: String,

    #[arg(long, help = "Print AWS CLI command to store the key in SSM")]
    print_aws_command: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Generate a secure 32-byte (256-bit) key for AES-256-GCM
    let mut key_bytes = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut key_bytes);

    // Encode as base64 for storage in SSM
    let key_base64 = general_purpose::STANDARD.encode(&key_bytes);

    println!(
        "Generated 32-byte encryption key for environment: {}",
        args.environment
    );
    println!();
    println!("Base64-encoded key (for SSM):");
    println!("{}", key_base64);
    println!();

    if args.print_aws_command {
        let env_normalized = if args.environment == "development" || args.environment == "dev" {
            "dev"
        } else if args.environment == "production" || args.environment == "prod" {
            "prod"
        } else {
            &args.environment
        };

        let ssm_path = format!(
            "/leadsnebula/{}/rust/encryption/api_key_key",
            env_normalized
        );

        println!("To store this key in AWS SSM Parameter Store, run:");
        println!();
        println!("aws ssm put-parameter \\");
        println!("  --name \"{}\" \\", ssm_path);
        println!("  --value \"{}\" \\", key_base64);
        println!("  --type \"SecureString\" \\");
        println!("  --overwrite");
        println!();
        println!("Or for both dev and prod:");
        println!();
        println!("# Dev environment:");
        println!("aws ssm put-parameter \\");
        println!("  --name \"/leadsnebula/dev/rust/encryption/api_key_key\" \\");
        println!("  --value \"{}\" \\", key_base64);
        println!("  --type \"SecureString\" \\");
        println!("  --overwrite");
        println!();
        println!("# Prod environment:");
        println!("aws ssm put-parameter \\");
        println!("  --name \"/leadsnebula/prod/rust/encryption/api_key_key\" \\");
        println!("  --value \"{}\" \\", key_base64);
        println!("  --type \"SecureString\" \\");
        println!("  --overwrite");
    }

    println!();
    println!("⚠️  IMPORTANT: Keep this key secure! It's used to encrypt API keys in the database.");
    println!("   Store it in SSM Parameter Store and never commit it to version control.");

    Ok(())
}
