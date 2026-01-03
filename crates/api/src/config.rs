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
    pub jwt_secret: String,
    #[allow(dead_code)] // Used by Sentry initialization in main.rs
    pub sentry_dsn: Option<String>,
    pub environment: String,
    #[allow(dead_code)] // Used by EmailService when implemented
    pub from_email: String,
}

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub db_pool: Arc<PgPool>,
    #[allow(dead_code)] // Used by services that need Redis
    pub redis: Option<Arc<RedisClient>>,
    #[allow(dead_code)] // Used by services that need SSM
    pub ssm: Arc<SsmService>,
}

impl AppConfig {
    pub async fn load() -> anyhow::Result<Self> {
        let environment = std::env::var("ENVIRONMENT")
            .or_else(|_| std::env::var("ENV"))
            .unwrap_or_else(|_| "development".to_string());

        let env_normalized = leadsnebula_core::normalize_env_for_ssm(&environment);

        // Create SSM service (without Redis initially for bootstrapping)
        let ssm = Arc::new(SsmService::new(environment.clone(), None).await?);

        // Fetch all configs in one batched call
        let config_path = format!("/leadsnebula/{}/rust/", env_normalized);
        let params = ssm.get_parameters_by_path(&config_path).await?;

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

        let redis_url = params
            .get(&format!(
                "/leadsnebula/{}/rust/redis/connection_url",
                env_normalized
            ))
            .cloned()
            .or_else(|| std::env::var("REDIS_URL").ok());

        let jwt_secret = params
            .get(&format!(
                "/leadsnebula/{}/rust/auth/jwt_secret",
                env_normalized
            ))
            .cloned()
            .or_else(|| std::env::var("JWT_SECRET").ok())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "JWT_SECRET not found in SSM at /leadsnebula/{}/rust/auth/jwt_secret",
                    env_normalized
                )
            })?;

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

        info!(
            "Configuration loaded successfully for environment: {}",
            environment
        );

        Ok(Self {
            database_url,
            redis_url,
            jwt_secret,
            sentry_dsn,
            environment,
            from_email,
        })
    }
}

impl AppState {
    pub async fn new() -> anyhow::Result<Self> {
        let config = AppConfig::load().await?;

        // Create database pool
        info!("Connecting to database...");
        let db_pool = Arc::new(create_pool(&config.database_url).await?);
        info!("Database connection pool created successfully");

        // Create Redis client if URL is provided
        let redis = if let Some(redis_url) = &config.redis_url {
            info!(
                "Connecting to Redis at {}...",
                redis_url.split('@').last().unwrap_or("(hidden)")
            );
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                RedisClient::new(redis_url, config.environment.clone()),
            )
            .await
            {
                Ok(Ok(client)) => {
                    info!("Redis connection created successfully");
                    Some(Arc::new(client))
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        "Failed to connect to Redis: {}. Continuing without Redis cache.",
                        e
                    );
                    None
                }
                Err(_) => {
                    tracing::warn!("Redis connection timed out after 5 seconds. Continuing without Redis cache.");
                    None
                }
            }
        } else {
            info!("No Redis URL provided, skipping Redis connection");
            None
        };

        // Create SSM service with Redis for caching
        let ssm = Arc::new(SsmService::new(config.environment.clone(), redis.clone()).await?);

        Ok(Self {
            config,
            db_pool,
            redis,
            ssm,
        })
    }
}
