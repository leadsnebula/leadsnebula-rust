use anyhow::Result;
use clap::Parser;
use leadsnebula_core::ssm::SsmService;
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, Client};
use std::error::Error;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "test-redis-connection")]
#[command(about = "Test direct Redis connection without pooling")]
struct Args {
    #[arg(short, long, default_value = "development")]
    environment: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    info!("Testing direct Redis connection (no pooling)...");

    // Load Redis URL from SSM
    let ssm = Arc::new(SsmService::new(args.environment.clone(), None).await?);

    // Fetch from prod path (as per config.rs logic - Redis URL is always in prod path)
    let config_path = "/leadsnebula/prod/rust/";
    let params = ssm.get_parameters_by_path(config_path).await?;
    let redis_url = params
        .get("/leadsnebula/prod/rust/redis/connection_url")
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Redis URL not found in SSM at /leadsnebula/prod/rust/redis/connection_url"
            )
        })?;

    let display_url = redis_url
        .split_once("://")
        .and_then(|(scheme, rest)| {
            rest.split_once('@')
                .map(|(_, host_port)| format!("{}://{}", scheme, host_port))
        })
        .unwrap_or_else(|| "(hidden)".to_string());

    info!("Redis URL: {}...", display_url);
    info!(
        "Scheme: {}",
        if redis_url.starts_with("rediss://") {
            "TLS (rediss://)"
        } else {
            "Plain (redis://)"
        }
    );

    // Test 1: Client::open()
    info!("🔵 Test 1: Creating Redis client with Client::open()...");
    let start = Instant::now();
    let client = match Client::open(redis_url.as_str()) {
        Ok(c) => {
            info!("✅ Client::open() succeeded in {:?}", start.elapsed());
            c
        }
        Err(e) => {
            error!("❌ Client::open() failed: {} (kind: {:?})", e, e.kind());
            return Err(anyhow::anyhow!("Client::open() failed: {}", e));
        }
    };

    // Test 2: client.get_connection_manager() - direct connection (using recommended method)
    info!("🔵 Test 2: Getting ConnectionManager via client.get_connection_manager() (recommended method)...");
    let start = Instant::now();
    let mut conn: ConnectionManager = match client.get_connection_manager().await {
        Ok(c) => {
            info!(
                "✅ client.get_connection_manager() succeeded in {:?}",
                start.elapsed()
            );
            c
        }
        Err(e) => {
            error!(
                "❌ client.get_connection_manager() failed after {:?}: {} (kind: {:?}, is_connection_refusal: {}, is_timeout: {}, is_io_error: {})",
                start.elapsed(),
                e,
                e.kind(),
                e.is_connection_refusal(),
                e.is_timeout(),
                e.is_io_error()
            );
            if let Some(source) = e.source() {
                error!("Error source: {}", source);
            }
            return Err(anyhow::anyhow!(
                "client.get_connection_manager() failed: {}",
                e
            ));
        }
    };

    // Test 3: PING command
    info!("🔵 Test 3: Sending PING command...");
    let start = Instant::now();
    match conn.ping::<String>().await {
        Ok(result) => {
            info!("✅ PING succeeded in {:?}: {}", start.elapsed(), result);
        }
        Err(e) => {
            error!("❌ PING failed: {}", e);
            return Err(anyhow::anyhow!("PING failed: {}", e));
        }
    }

    // Test 4: SET/GET
    info!("🔵 Test 4: Testing SET/GET operations...");
    let test_key = format!("test:direct:{}", chrono::Utc::now().timestamp());
    let test_value = "test_value";

    let start = Instant::now();
    match conn.set::<_, _, ()>(&test_key, test_value).await {
        Ok(_) => {
            info!("✅ SET succeeded in {:?}", start.elapsed());
        }
        Err(e) => {
            error!("❌ SET failed: {}", e);
            return Err(anyhow::anyhow!("SET failed: {}", e));
        }
    }

    let start = Instant::now();
    match conn.get::<_, Option<String>>(&test_key).await {
        Ok(Some(value)) => {
            info!("✅ GET succeeded in {:?}: {}", start.elapsed(), value);
            if value != test_value {
                error!(
                    "❌ Value mismatch: expected '{}', got '{}'",
                    test_value, value
                );
                return Err(anyhow::anyhow!("Value mismatch"));
            }
        }
        Ok(None) => {
            error!("❌ GET returned None");
            return Err(anyhow::anyhow!("GET returned None"));
        }
        Err(e) => {
            error!("❌ GET failed: {}", e);
            return Err(anyhow::anyhow!("GET failed: {}", e));
        }
    }

    // Cleanup
    let _: () = conn.del(&test_key).await?;
    info!("✅ Cleanup: deleted test key");

    info!("✅✅✅ All Redis connection tests passed!");
    Ok(())
}
