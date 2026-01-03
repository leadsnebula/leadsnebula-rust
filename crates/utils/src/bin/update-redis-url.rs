use anyhow::Result;
use clap::Parser;
use leadsnebula_core::ssm::SsmService;
use tracing::info;

#[derive(Parser)]
#[command(name = "update-redis-url")]
#[command(about = "Update Redis URL in SSM Parameter Store")]
struct Args {
    #[arg(short, long)]
    environment: String,
    #[arg(short, long)]
    redis_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let ssm = SsmService::new(args.environment.clone(), None).await?;
    let path = ssm.build_path("rust", "redis", Some("connection_url"));

    // Note: This would require a put_parameter method in SsmService
    // For now, just print the path that should be updated
    info!("Redis URL should be updated in SSM at: {}", path);
    info!(
        "Use AWS CLI: aws ssm put-parameter --name {} --value {} --type SecureString --overwrite",
        path, args.redis_url
    );

    Ok(())
}
