use base64::{engine::general_purpose, Engine as _};
use leadsnebula_core::redis::RedisClient;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::info;

#[derive(Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub redis_url: Option<String>,
    pub redis_pool_size: u32,
    pub redis_min_idle: u32,
    #[allow(dead_code)] // Used by routes that require authentication
    pub jwt_secret: String,
    #[allow(dead_code)] // Used by routes that encrypt/decrypt API keys
    pub encryption_key: Vec<u8>,
    #[allow(dead_code)] // Used by Sentry initialization in main.rs
    pub sentry_dsn: Option<String>,
    pub environment: String,
    #[allow(dead_code)] // Used by EmailService when implemented
    pub from_email: String,
}

#[derive(Clone)]
pub struct AppState {
    #[allow(dead_code)] // Used by routes that need config
    pub config: AppConfig,
    #[allow(dead_code)] // Used by routes that need database access
    pub db_pool: Arc<PgPool>,
    #[allow(dead_code)] // Used by services that need Redis
    pub redis: Option<Arc<RedisClient>>,
    #[allow(dead_code)] // Used by services that need SSM
    pub ssm: Arc<SsmService>,
}

impl AppConfig {
    pub async fn load() -> anyhow::Result<Self> {
        // Check if .env.local exists to detect local development
        // This ensures we only use .env.local REDIS_URL in local dev, not in Fly.io deployments
        let is_local_dev = std::path::Path::new(".env.local").exists();

        // Load .env.local first for local development (highest priority)
        // This ensures local development doesn't interfere with production
        // Note: main.rs also loads .env.local, but we verify it's available here
        // We don't reload it here since main.rs already loaded it with tolerant parsing
        if is_local_dev {
            // Verify REDIS_URL was loaded (main.rs already loaded .env.local)
            if let Ok(redis_url) = std::env::var("REDIS_URL") {
                tracing::info!(
                    "REDIS_URL found in environment: {}...",
                    if redis_url.len() > 30 {
                        format!("{}...", &redis_url[..30])
                    } else {
                        redis_url
                    }
                );
            } else {
                tracing::warn!("REDIS_URL not found in environment after .env.local was loaded");
            }
        }

        let environment = std::env::var("ENVIRONMENT")
            .or_else(|_| std::env::var("ENV"))
            .unwrap_or_else(|_| "development".to_string());

        let env_normalized = leadsnebula_core::normalize_env_for_ssm(&environment);

        // Create SSM service (without Redis initially for bootstrapping)
        let ssm = Arc::new(SsmService::new(environment.clone(), None).await?);

        // Fetch all configs in one batched call
        let config_path = format!("/leadsnebula/{}/rust/", env_normalized);
        let mut params = ssm.get_parameters_by_path(&config_path).await?;

        // For dev environment, also fetch from prod path as fallback
        if env_normalized == "dev" {
            let prod_params = ssm
                .get_parameters_by_path("/leadsnebula/prod/rust/")
                .await?;
            params.extend(prod_params);
        }

        // Extract values from batched parameters
        let database_url = params
            .get(&format!("/leadsnebula/{}/rust/db/connection_url", env_normalized))
            .cloned()
            .or_else(|| std::env::var("DATABASE_URL").ok())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "DATABASE_URL not found in SSM at /leadsnebula/{}/rust/db/connection_url. Production environments must use SSM Parameter Store.",
                    env_normalized
                )
            })?;

        // Redis URL: Always use prod path for both environments (shared Redis instance)
        // In local development (when .env.local exists), use REDIS_URL from .env.local
        // In deployed environments (Fly.io), always use SSM (no REDIS_URL secrets in Fly.io)
        let redis_url = if std::env::var("SKIP_REDIS").is_ok() {
            info!("SKIP_REDIS environment variable set - skipping Redis connection");
            None
        } else if is_local_dev {
            // Local development: prioritize REDIS_URL from .env.local
            match std::env::var("REDIS_URL") {
                Ok(redis_url) => {
                    info!(
                        "Using REDIS_URL from .env.local for local development: {}",
                        if redis_url.len() > 20 {
                            format!("{}...", &redis_url[..20])
                        } else {
                            redis_url.clone()
                        }
                    );
                    Some(redis_url)
                }
                Err(e) => {
                    tracing::warn!(
                        "REDIS_URL not found in environment (error: {}), falling back to SSM",
                        e
                    );
                    params
                        .get("/leadsnebula/prod/rust/redis/connection_url")
                        .cloned()
                }
            }
        } else {
            // Deployed environments (Fly.io): always use SSM (sole source of truth)
            // No REDIS_URL secrets in Fly.io - SSM is the only source
            params
                .get("/leadsnebula/prod/rust/redis/connection_url")
                .cloned()
        };

        // Try multiple possible SSM paths for JWT_SECRET (supporting different path structures)
        // Also try prod path as fallback for dev environment
        let jwt_secret = params
            .get(&format!("/leadsnebula/{}/rust/auth/jwt_secret", env_normalized))
            .or_else(|| params.get(&format!("/leadsnebula/{}/rust/jwt/secret_key", env_normalized)))
            .or_else(|| params.get(&format!("/leadsnebula/{}/rust/jwt_secret", env_normalized)))
            .or_else(|| {
                // Fallback to prod path if dev environment
                if env_normalized == "dev" {
                    params.get("/leadsnebula/prod/rust/jwt/secret_key")
                        .or_else(|| params.get("/leadsnebula/prod/rust/auth/jwt_secret"))
                        .or_else(|| params.get("/leadsnebula/prod/rust/jwt_secret"))
                } else {
                    None
                }
            })
            .cloned()
            .or_else(|| std::env::var("JWT_SECRET").ok())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "JWT_SECRET not found in SSM. Tried paths: /leadsnebula/{}/rust/auth/jwt_secret, /leadsnebula/{}/rust/jwt/secret_key, /leadsnebula/{}/rust/jwt_secret{}",
                    env_normalized, env_normalized, env_normalized,
                    if env_normalized == "dev" { ", /leadsnebula/prod/rust/jwt/secret_key, /leadsnebula/prod/rust/auth/jwt_secret" } else { "" }
                )
            })?;

        // Load encryption key for API key encryption from SSM (REQUIRED)
        // This is a separate key from Rails encryption keys - it's specifically for encrypting publisher API keys
        // Path: /leadsnebula/{env}/rust/encryption/api_key_key
        // IMPORTANT: Dev environment uses dev key only (no prod fallback) for local development
        // Production uses prod key only
        let encryption_key_path = format!(
            "/leadsnebula/{}/rust/encryption/api_key_key",
            env_normalized
        );

        let encryption_key_str = params
            .get(&encryption_key_path)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Encryption key not found in SSM at {}. This key is required for encrypting publisher API keys. Please create it in SSM Parameter Store.",
                    encryption_key_path
                )
            })?;

        // Decode base64 encryption key (SSM stores it as base64)
        let encryption_key = general_purpose::STANDARD
            .decode(encryption_key_str)
            .map_err(|e| anyhow::anyhow!("Failed to decode encryption key from base64: {}", e))?;

        if encryption_key.len() != 32 {
            return Err(anyhow::anyhow!(
                "Encryption key must be 32 bytes (256 bits) for AES-256-GCM, got {} bytes",
                encryption_key.len()
            ));
        }

        let sentry_dsn = params
            .get(&format!(
                "/leadsnebula/{}/rust/monitoring/sentry_dsn",
                env_normalized
            ))
            .cloned()
            .or_else(|| std::env::var("SENTRY_DSN").ok());

        let from_email = params
            .get(&format!(
                "/leadsnebula/{}/rust/email/from_address",
                env_normalized
            ))
            .cloned()
            .or_else(|| std::env::var("FROM_EMAIL").ok())
            .unwrap_or_else(|| "noreply@leadsnebula.com".to_string());

        // Redis pool size (default: 15, configurable via SSM)
        let redis_pool_size = params
            .get(&format!(
                "/leadsnebula/{}/rust/redis/pool_size",
                env_normalized
            ))
            .and_then(|s| s.parse::<u32>().ok())
            .or_else(|| {
                std::env::var("REDIS_POOL_SIZE")
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
            })
            .unwrap_or(15);

        // Redis min_idle (default: 2, configurable via SSM)
        let redis_min_idle = params
            .get(&format!(
                "/leadsnebula/{}/rust/redis/min_idle",
                env_normalized
            ))
            .and_then(|s| s.parse::<u32>().ok())
            .or_else(|| {
                std::env::var("REDIS_MIN_IDLE")
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
            })
            .unwrap_or(2);

        info!(
            "Configuration loaded successfully for environment: {}",
            environment
        );

        Ok(Self {
            database_url,
            redis_url,
            redis_pool_size,
            redis_min_idle,
            jwt_secret,
            encryption_key,
            sentry_dsn,
            environment,
            from_email,
        })
    }
}

impl AppState {
    pub async fn new() -> anyhow::Result<Self> {
        let config = AppConfig::load().await?;

        // Parallelize database and Redis initialization for faster startup
        info!("Initializing database and Redis connections in parallel...");

        let db_future = async {
            info!("Connecting to database...");
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                create_pool(&config.database_url),
            )
            .await
        };

        let redis_future = async {
            if let Some(redis_url) = &config.redis_url {
                Some(
                    self::init_redis(
                        redis_url,
                        &config.environment,
                        config.redis_pool_size,
                        config.redis_min_idle,
                    )
                    .await,
                )
            } else {
                None
            }
        };

        // Initialize both in parallel
        let (db_result, redis_result) = tokio::join!(db_future, redis_future);

        // Handle database result
        let db_pool = match db_result {
            Ok(Ok(pool)) => {
                info!("Database connection pool created successfully");
                Arc::new(pool)
            }
            Ok(Err(e)) => {
                return Err(anyhow::anyhow!(
                    "Failed to create database pool: {}. Check DATABASE_URL and network connectivity.",
                    e
                ));
            }
            Err(_) => {
                return Err(anyhow::anyhow!(
                    "Database connection timed out after 10 seconds. Check DATABASE_URL and network connectivity."
                ));
            }
        };

        // Handle Redis result
        let redis = match redis_result {
            Some(Ok(Some(client))) => Some(client),
            Some(Ok(None)) => {
                tracing::warn!(
                    "Redis initialization returned None. Continuing without Redis cache."
                );
                None
            }
            Some(Err(e)) => {
                tracing::warn!(
                    "Redis initialization failed: {}. Continuing without Redis cache.",
                    e
                );
                None
            }
            None => None,
        };

        // Create SSM service with Redis for caching (if available)
        let ssm = Arc::new(SsmService::new(config.environment.clone(), redis.clone()).await?);

        Ok(Self {
            config,
            db_pool,
            redis,
            ssm,
        })
    }
}

// Helper function to initialize Redis (extracted for parallelization)
async fn init_redis(
    redis_url: &str,
    environment: &str,
    pool_size: u32,
    min_idle: u32,
) -> anyhow::Result<Option<Arc<RedisClient>>> {
    // Check if we should skip Redis in local development
    let is_local_dev = environment == "development"
        || environment == "dev"
        || std::env::var("ENVIRONMENT").unwrap_or_default() == "development";

    // Use shorter timeout for local development to fail fast
    let timeout_seconds = if is_local_dev { 3 } else { 30 };

    // Extract host/port for logging (hide credentials)
    let display_url = redis_url
        .split_once("://")
        .and_then(|(scheme, rest)| {
            rest.split_once('@')
                .map(|(_, host_port)| format!("{}://{}", scheme, host_port))
                .or_else(|| {
                    Some(format!(
                        "{}://{}",
                        scheme,
                        rest.split('/').next().unwrap_or("(hidden)")
                    ))
                })
        })
        .unwrap_or_else(|| "(hidden)".to_string());

    info!("Connecting to Redis at {}...", display_url);

    // First connection attempt with configurable timeout (shorter for local dev)
    info!(
        "🔵 Starting Redis connection attempt ({}s timeout)...",
        timeout_seconds
    );
    let start_time = std::time::Instant::now();
    let connect_result =
        tokio::time::timeout(std::time::Duration::from_secs(timeout_seconds), async {
            info!("🔵 Inside timeout wrapper - calling RedisClient::new()...");
            RedisClient::new(redis_url, environment.to_string(), pool_size, min_idle).await
        })
        .await;
    let elapsed = start_time.elapsed();
    info!("🔵 Redis connection attempt completed in {:?}", elapsed);

    match connect_result {
        Ok(Ok(client)) => {
            info!("✅ Redis connection created successfully");
            // Verify connection with a ping
            match client.ping().await {
                Ok(pong) => {
                    info!("Redis ping successful: {}", pong);
                    Ok(Some(Arc::new(client)))
                }
                Err(e) => {
                    tracing::warn!(
                        "Redis connection created but ping failed: {}. Continuing without Redis cache.",
                        e
                    );
                    Ok(None)
                }
            }
        }
        Ok(Err(e)) => {
            let error_source = e
                .source()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            tracing::warn!(
                "❌ Failed to connect to Redis: {} (source: {}). Retrying in 2 seconds...",
                e,
                error_source
            );
            // In local development, don't retry - fail fast
            if is_local_dev {
                tracing::warn!("❌ Redis connection failed. Skipping Redis in local development.");
                return Ok(None);
            }

            // Retry once after 2 seconds (only in non-local environments)
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            info!("Retrying Redis connection...");
            match tokio::time::timeout(
                std::time::Duration::from_secs(timeout_seconds),
                RedisClient::new(redis_url, environment.to_string(), pool_size, min_idle),
            )
            .await
            {
                Ok(Ok(client)) => {
                    info!("✅ Redis connection created successfully on retry");
                    match client.ping().await {
                        Ok(pong) => {
                            info!("Redis ping successful on retry: {}", pong);
                            Ok(Some(Arc::new(client)))
                        }
                        Err(e2) => {
                            tracing::warn!(
                                "Redis connection created on retry but ping failed: {}. Continuing without Redis cache.",
                                e2
                            );
                            Ok(None)
                        }
                    }
                }
                Ok(Err(e2)) => {
                    tracing::warn!(
                        "❌ Redis connection failed after retry: {}. Continuing without Redis cache.",
                        e2
                    );
                    Ok(None)
                }
                Err(_) => {
                    tracing::warn!(
                        "❌ Redis connection timed out after retry ({} seconds). Continuing without Redis cache.",
                        timeout_seconds
                    );
                    Ok(None)
                }
            }
        }
        Err(_) => {
            // In local development, don't retry - fail fast
            if is_local_dev {
                tracing::warn!(
                    "❌ Redis connection timed out after {} seconds. Skipping Redis in local development.",
                    timeout_seconds
                );
                return Ok(None);
            }

            tracing::warn!(
                "❌ Redis connection timed out after {} seconds. Retrying in 2 seconds...",
                timeout_seconds
            );
            // Retry once after 2 seconds (only in non-local environments)
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            info!("Retrying Redis connection after timeout...");
            match tokio::time::timeout(
                std::time::Duration::from_secs(timeout_seconds),
                RedisClient::new(redis_url, environment.to_string(), pool_size, min_idle),
            )
            .await
            {
                Ok(Ok(client)) => {
                    info!("✅ Redis connection created successfully on retry");
                    match client.ping().await {
                        Ok(pong) => {
                            info!("Redis ping successful on retry: {}", pong);
                            Ok(Some(Arc::new(client)))
                        }
                        Err(e2) => {
                            tracing::warn!(
                                "Redis connection created on retry but ping failed: {}. Continuing without Redis cache.",
                                e2
                            );
                            Ok(None)
                        }
                    }
                }
                Ok(Err(e2)) => {
                    tracing::warn!(
                        "❌ Redis connection failed after retry: {}. Continuing without Redis cache.",
                        e2
                    );
                    Ok(None)
                }
                Err(_) => {
                    tracing::warn!(
                        "❌ Redis connection timed out after retry ({} seconds). Continuing without Redis cache.",
                        timeout_seconds
                    );
                    Ok(None)
                }
            }
        }
    }
}
