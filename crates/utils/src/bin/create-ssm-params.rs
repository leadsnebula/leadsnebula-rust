// Temporary binary to create SSM parameters
// Usage: cargo run --bin create-ssm-params

use anyhow::Result;
use leadsnebula_core::SsmClient;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Creating AWS SSM Parameters for LeadsNebula Rust ===\n");

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

    // Neon Database Connection Strings - MUST be provided via environment variables
    // DO NOT hardcode credentials in source code
    let dev_db_url =
        std::env::var("DEV_DATABASE_URL").expect("DEV_DATABASE_URL environment variable required");
    let prod_db_url = std::env::var("PROD_DATABASE_URL")
        .expect("PROD_DATABASE_URL environment variable required");

    // Development environment
    println!("--- Development Environment ---");
    let dev_path = "/leadsnebula/development/rust/db/connection_url";
    match ssm_client
        .put_parameter(
            dev_path,
            &dev_db_url,
            Some("Neon PostgreSQL connection URL for development environment (Rust app)"),
        )
        .await
    {
        Ok(_) => println!("✅ Created: {}\n", dev_path),
        Err(e) => {
            eprintln!("❌ Failed to create {}: {}\n", dev_path, e);
            return Err(e);
        }
    }

    // Staging environment (using dev DB for now)
    println!("--- Staging Environment ---");
    let staging_path = "/leadsnebula/staging/rust/db/connection_url";
    match ssm_client
        .put_parameter(
            staging_path,
            &dev_db_url,
            Some(
                "Neon PostgreSQL connection URL for staging environment (Rust app) - using dev DB",
            ),
        )
        .await
    {
        Ok(_) => println!("✅ Created: {}\n", staging_path),
        Err(e) => {
            eprintln!("❌ Failed to create {}: {}\n", staging_path, e);
            return Err(e);
        }
    }

    // Production environment
    println!("--- Production Environment ---");
    let prod_path = "/leadsnebula/production/rust/db/connection_url";
    match ssm_client
        .put_parameter(
            prod_path,
            &prod_db_url,
            Some("Neon PostgreSQL connection URL for production environment (Rust app)"),
        )
        .await
    {
        Ok(_) => println!("✅ Created: {}\n", prod_path),
        Err(e) => {
            eprintln!("❌ Failed to create {}: {}\n", prod_path, e);
            return Err(e);
        }
    }

    // Create JWT secrets for all environments
    println!("--- Creating JWT Secrets ---");
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let jwt_secret: String = (0..64)
        .map(|_| rng.sample(rand::distributions::Alphanumeric) as char)
        .collect();

    let jwt_paths = vec![
        (
            "development",
            "/leadsnebula/development/rust/jwt/secret_key",
        ),
        ("staging", "/leadsnebula/staging/rust/jwt/secret_key"),
        ("production", "/leadsnebula/production/rust/jwt/secret_key"),
    ];

    for (env_name, jwt_path) in jwt_paths {
        match ssm_client
            .put_parameter(
                jwt_path,
                &jwt_secret,
                Some(&format!(
                    "JWT signing secret for {} environment (Rust app)",
                    env_name
                )),
            )
            .await
        {
            Ok(_) => println!("✅ Created: {}", jwt_path),
            Err(e) => {
                eprintln!("❌ Failed to create {}: {}", jwt_path, e);
                return Err(e);
            }
        }
    }
    println!();

    println!("=== Setup Complete ===\n");
    println!("Verifying parameters...\n");

    // Verify database parameters were created
    let dev_check = ssm_client.get_parameter(dev_path).await?;
    let staging_check = ssm_client.get_parameter(staging_path).await?;
    let prod_check = ssm_client.get_parameter(prod_path).await?;

    println!("Database URL Verification:");
    println!(
        "  - Development: {}",
        if dev_check.is_some() { "✅" } else { "❌" }
    );
    println!(
        "  - Staging: {}",
        if staging_check.is_some() {
            "✅"
        } else {
            "❌"
        }
    );
    println!(
        "  - Production: {}",
        if prod_check.is_some() { "✅" } else { "❌" }
    );

    // Verify JWT secrets
    let jwt_dev = ssm_client
        .get_parameter("/leadsnebula/development/rust/jwt/secret_key")
        .await?;
    let jwt_staging = ssm_client
        .get_parameter("/leadsnebula/staging/rust/jwt/secret_key")
        .await?;
    let jwt_prod = ssm_client
        .get_parameter("/leadsnebula/production/rust/jwt/secret_key")
        .await?;

    println!("JWT Secret Verification:");
    println!(
        "  - Development: {}",
        if jwt_dev.is_some() { "✅" } else { "❌" }
    );
    println!(
        "  - Staging: {}",
        if jwt_staging.is_some() { "✅" } else { "❌" }
    );
    println!(
        "  - Production: {}",
        if jwt_prod.is_some() { "✅" } else { "❌" }
    );

    if dev_check.is_some()
        && staging_check.is_some()
        && prod_check.is_some()
        && jwt_dev.is_some()
        && jwt_staging.is_some()
        && jwt_prod.is_some()
    {
        println!("\n✅ All parameters verified successfully!");
    } else {
        println!("\n⚠️  Warning: Some parameters may not have been created correctly");
    }

    Ok(())
}
