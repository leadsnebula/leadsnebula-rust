use anyhow::Result;
use clap::Parser;
use leadsnebula_core::ssm::SsmService;

#[derive(Parser)]
#[command(name = "fetch-ssm-values")]
#[command(about = "Fetch values from SSM Parameter Store for .env.local")]
struct Args {
    #[arg(short, long, default_value = "production")]
    environment: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let ssm = SsmService::new(args.environment.clone(), None).await?;
    let env_normalized = leadsnebula_core::normalize_env_for_ssm(&args.environment);
    let config_path = format!("/leadsnebula/{}/rust/", env_normalized);
    
    let params = ssm.get_parameters_by_path(&config_path).await?;
    
    // Map SSM paths to env var names
    let mappings = vec![
        (format!("/leadsnebula/{}/rust/db/connection_url", env_normalized), "DATABASE_URL"),
        (format!("/leadsnebula/{}/rust/redis/connection_url", env_normalized), "REDIS_URL"),
        (format!("/leadsnebula/{}/rust/auth/jwt_secret", env_normalized), "JWT_SECRET"),
        (format!("/leadsnebula/{}/rust/monitoring/sentry_dsn", env_normalized), "SENTRY_DSN"),
        (format!("/leadsnebula/{}/rust/email/from_address", env_normalized), "FROM_EMAIL"),
    ];
    
    println!("# Values fetched from SSM Parameter Store");
    println!("# Environment: {} (normalized: {})", args.environment, env_normalized);
    println!();
    
    for (ssm_path, env_var) in mappings {
        if let Some(value) = params.get(&ssm_path) {
            println!("{}={}", env_var, value);
        } else {
            println!("# {} not found in SSM", env_var);
        }
    }
    
    Ok(())
}

