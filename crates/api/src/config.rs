use base64::{engine::general_purpose, Engine as _};
use leadsnebula_core::cache::CacheService;
use leadsnebula_core::email::EmailService;
use leadsnebula_core::redis::RedisClient;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::services::write_behind_queue::WriteBehindQueue;
use leadsnebula_core::ssm::SsmService;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{info, warn};

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
    #[allow(dead_code)] // Used by EmailService
    pub from_email: String,
    /// Base URL for password reset links (e.g. https://app.leadsnebula.com). No trailing slash.
    pub password_reset_base_url: String,
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
    #[allow(dead_code)] // Used by routes for caching
    pub cache: Option<Arc<CacheService>>,
    #[allow(dead_code)] // Used for batching background database writes
    pub write_behind_queue: Arc<WriteBehindQueue>,
    #[allow(dead_code)] // Used by password-reset-email route
    pub email_service: Arc<EmailService>,
}

impl AppConfig {
    /// Strip connection parameters that sqlx doesn't recognize (e.g. channel_binding=require from Neon).
    /// Prevents "ignoring unrecognized connect parameter" warnings in logs.
    fn sanitize_database_url(url: &str) -> String {
        if let Some((base, query)) = url.split_once('?') {
            let params: Vec<&str> = query
                .split('&')
                .filter(|p| {
                    let name = p.split('=').next().unwrap_or("");
                    name != "channel_binding"
                })
                .collect();
            if params.is_empty() {
                base.to_string()
            } else {
                format!("{}?{}", base, params.join("&"))
            }
        } else {
            url.to_string()
        }
    }

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
                tracing::debug!(
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

        // In local development, try to load all config from environment variables first
        // Only use SSM as fallback if environment variables aren't available
        let mut params = std::collections::HashMap::new();
        if is_local_dev {
            // Try to load all required config from environment variables
            let has_all_env_vars = std::env::var("DATABASE_URL").is_ok()
                && std::env::var("JWT_SECRET").is_ok()
                && std::env::var("ENCRYPTION_KEY").is_ok();

            if !has_all_env_vars {
                // Some env vars missing, try SSM as fallback
                tracing::debug!("Local development: some environment variables missing, attempting SSM fallback");
                if let Ok(ssm_service) = SsmService::new(environment.clone(), None).await {
                    let ssm_arc = Arc::new(ssm_service);
                    let config_path = format!("/leadsnebula/{}/rust/", env_normalized);
                    if let Ok(mut ssm_params) = ssm_arc.get_parameters_by_path(&config_path).await {
                        // For dev environment, also fetch from prod path as fallback
                        if env_normalized == "dev" {
                            if let Ok(prod_params) = ssm_arc
                                .get_parameters_by_path("/leadsnebula/prod/rust/")
                                .await
                            {
                                ssm_params.extend(prod_params);
                            }
                        }
                        params = ssm_params;
                    } else {
                        tracing::warn!(
                            "Failed to fetch SSM parameters. Will use environment variables only."
                        );
                    }
                } else {
                    tracing::warn!(
                        "Failed to create SSM service. Will use environment variables only."
                    );
                }
            } else {
                tracing::info!(
                    "Local development detected: using environment variables instead of SSM"
                );
            }
        } else {
            // Production/deployed: always use SSM
            let ssm_service = Arc::new(SsmService::new(environment.clone(), None).await?);
            let config_path = format!("/leadsnebula/{}/rust/", env_normalized);
            let mut ssm_params = ssm_service.get_parameters_by_path(&config_path).await?;

            // For dev environment, also fetch from prod path as fallback
            if env_normalized == "dev" {
                let prod_params = ssm_service
                    .get_parameters_by_path("/leadsnebula/prod/rust/")
                    .await?;
                ssm_params.extend(prod_params);
            }
            params = ssm_params;
        }

        // Extract values from batched parameters
        // In local dev, prioritize environment variables; in production, prioritize SSM
        let expected_path = format!("/leadsnebula/{}/rust/db/connection_url", env_normalized);
        let mut database_url = if is_local_dev {
            // Local dev: try environment variable first, then SSM
            std::env::var("DATABASE_URL")
                .ok()
                .or_else(|| params.get(&expected_path).cloned())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "DATABASE_URL not found in environment or SSM. For local development, set DATABASE_URL in .env.local"
                    )
                })?
        } else {
            // Production: try SSM first, then environment variable as fallback
            params
                .get(&expected_path)
                .cloned()
                .or_else(|| std::env::var("DATABASE_URL").ok())
                .ok_or_else(|| {
                    // Provide detailed error message with both original and normalized environment
                    let available_paths: Vec<String> = params.keys().cloned().collect();
                    anyhow::anyhow!(
                        "DATABASE_URL not found in SSM at {}. Environment: '{}' (normalized: '{}'). Production environments must use SSM Parameter Store. Available SSM paths: {:?}",
                        expected_path,
                        environment,
                        env_normalized,
                        available_paths
                    )
                })?
        };
        database_url = Self::sanitize_database_url(&database_url);

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
                    tracing::debug!(
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
        let jwt_secret = if is_local_dev {
            // Local dev: try environment variable first, then SSM
            std::env::var("JWT_SECRET")
                .ok()
                .or_else(|| {
                    params
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
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "JWT_SECRET not found in environment or SSM. For local development, set JWT_SECRET in .env.local"
                    )
                })?
        } else {
            // Production: try SSM first, then environment variable as fallback
            params
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
                })?
        };

        // Load encryption key for API key encryption from SSM (REQUIRED)
        // This is a separate key from Rails encryption keys - it's specifically for encrypting publisher API keys
        // Path: /leadsnebula/{env}/rust/encryption/api_key_key
        // IMPORTANT: Dev environment uses dev key only (no prod fallback) for local development
        // Production uses prod key only
        let encryption_key_path = format!(
            "/leadsnebula/{}/rust/encryption/api_key_key",
            env_normalized
        );

        let encryption_key_str = if is_local_dev {
            // Local dev: try environment variable first, then SSM
            std::env::var("ENCRYPTION_KEY")
                .ok()
                .or_else(|| params.get(&encryption_key_path).cloned())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Encryption key not found in environment or SSM at {}. For local development, set ENCRYPTION_KEY (base64 encoded, 32 bytes) in .env.local",
                        encryption_key_path
                    )
                })?
        } else {
            // Production: try SSM first
            params
                .get(&encryption_key_path)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Encryption key not found in SSM at {}. This key is required for encrypting publisher API keys. Please create it in SSM Parameter Store.",
                        encryption_key_path
                    )
                })?
        };

        // Decode encryption key - support both base64 (SSM format) and hex (local dev format)
        // Base64: 44 chars = 32 bytes, Hex: 64 chars = 32 bytes
        let encryption_key = if encryption_key_str.len() == 64
            && encryption_key_str.chars().all(|c| c.is_ascii_hexdigit())
        {
            // Looks like hex (64 hex characters = 32 bytes)
            hex::decode(&encryption_key_str)
                .map_err(|e| anyhow::anyhow!("Failed to decode encryption key from hex: {}", e))?
        } else {
            // Try base64 (SSM format)
            general_purpose::STANDARD
                .decode(&encryption_key_str)
                .map_err(|e| {
                    anyhow::anyhow!("Failed to decode encryption key from base64: {}", e)
                })?
        };

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

        let password_reset_base_url = params
            .get(&format!(
                "/leadsnebula/{}/rust/email/password_reset_base_url",
                env_normalized
            ))
            .cloned()
            .or_else(|| std::env::var("PASSWORD_RESET_BASE_URL").ok())
            .unwrap_or_else(|| "https://app.leadsnebula.com".to_string());

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
            .unwrap_or(25); // Increased from 15 to 25 for better concurrency

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
            .unwrap_or(5); // Increased from 2 to 5 for better connection readiness

        info!("Config loaded (environment={})", environment);

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
            password_reset_base_url,
        })
    }
}

impl AppState {
    pub async fn new() -> anyhow::Result<Self> {
        let config = AppConfig::load().await?;

        info!("Connecting to database and Redis...");

        let db_future = async {
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
                let pool_arc = Arc::new(pool);

                // CRITICAL: Pre-warm database SYNCHRONOUSLY (prevents first-request cold start)
                let db_warmup_start = std::time::Instant::now();
                match sqlx::query("SELECT 1").execute(pool_arc.as_ref()).await {
                    Ok(_) => {
                        let duration_ms = db_warmup_start.elapsed().as_millis();
                        info!(
                            db_warmup_ms = duration_ms,
                            "DB pre-warm done ({}ms)", duration_ms
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Database pre-warm failed; first request may be slower. Error: {}",
                            e
                        );
                    }
                }

                pool_arc
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
            Some(Ok(Some(client))) => {
                // CRITICAL: Pre-warm Redis connection SYNCHRONOUSLY (prevents first-request cold start)
                let redis_warmup_start = std::time::Instant::now();
                match client.ping().await {
                    Ok(_) => {
                        let duration_ms = redis_warmup_start.elapsed().as_millis();
                        info!(
                            redis_connect_ms = duration_ms,
                            "Redis connected ({}ms)", duration_ms
                        );
                    }
                    Err(e) => {
                        warn!("Redis pre-warm failed; cache may be slower. Error: {}", e);
                    }
                }
                Some(client)
            }
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
        // In local dev, SSM might not be available (no AWS credentials)
        // The AWS SDK should still create the service (it will just fail when used)
        let is_local_dev = std::path::Path::new(".env.local").exists();
        let ssm = match SsmService::new(config.environment.clone(), redis.clone()).await {
            Ok(ssm_service) => Arc::new(ssm_service),
            Err(e) => {
                if is_local_dev {
                    tracing::warn!(
                        "Failed to create SSM service in local dev: {}. This is OK if using environment variables for all config.",
                        e
                    );
                    // In local dev, if SSM creation fails, we can't create AppState with a working SSM
                    // But since we're using env vars, SSM might not be needed at runtime
                    // Try one more time - if it still fails, we'll return an error
                    SsmService::new(config.environment.clone(), redis.clone()).await
                        .map(Arc::new)
                        .map_err(|e2| {
                            anyhow::anyhow!(
                                "Failed to create SSM service (required by AppState): {} and {}. For local dev with env vars, SSM service creation should still succeed (it will just fail when used). Check AWS SDK configuration.",
                                e, e2
                            )
                        })?
                } else {
                    // Production: SSM is required, fail hard
                    return Err(anyhow::anyhow!(
                        "Failed to create SSM service: {}. SSM is required in production.",
                        e
                    ));
                }
            }
        };

        // SAMURAI PERFECTION: Pre-fetch SSM encryption keys SYNCHRONOUSLY during startup
        // This "pays" the 500-1000ms SSM cost at deploy time, not on first request
        // CRITICAL: Wait for SSM keys to be cached before proceeding (ensures first request is fast)
        let env_normalized = leadsnebula_core::normalize_env_for_ssm(&config.environment);
        let det_path = format!(
            "/leadsnebula/{}/carina/encryption/deterministic_key_v1",
            env_normalized
        );
        let salt_path = format!(
            "/leadsnebula/{}/carina/encryption/key_derivation_salt_v1",
            env_normalized
        );

        let ssm_prefetch_start = std::time::Instant::now();
        let prefetch_result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let (det_result, salt_result) = tokio::join!(
                ssm.get_parameter(&det_path, true),
                ssm.get_parameter(&salt_path, true)
            );
            (det_result, salt_result)
        })
        .await;

        let ssm_prefetch_ms = ssm_prefetch_start.elapsed().as_millis();
        match prefetch_result {
            Ok((Ok(Some(_)), Ok(Some(_)))) => {
                info!(
                    ssm_encryption_ms = ssm_prefetch_ms,
                    "Encryption keys loaded from SSM ({}ms)", ssm_prefetch_ms
                );
            }
            Ok((Ok(Some(_)), Ok(None))) => {
                tracing::debug!(
                    "SSM pre-fetch: deterministic_key found but salt not found (will fetch on first request)"
                );
            }
            Ok((Ok(Some(_)), Err(e))) => {
                tracing::debug!(
                    "SSM pre-fetch: deterministic_key found but salt fetch failed (will fetch on first request): {}",
                    e
                );
            }
            Ok((Ok(None), _)) => {
                tracing::debug!(
                    "SSM pre-fetch: deterministic_key not found (will fetch on first request)"
                );
            }
            Ok((Err(e), _)) => {
                warn!(
                    "SSM pre-fetch failed (non-critical, will fetch on first request): {}",
                    e
                );
            }
            Err(_) => {
                warn!(
                    "SSM pre-fetch timed out after 5s (non-critical, will fetch on first request)"
                );
            }
        }

        // Create CacheService from Redis (if available)
        let cache = redis.as_ref().map(|r| {
            Arc::new(CacheService::new(
                Some(r.clone()),
                config.environment.clone(),
            ))
        });

        // Create write-behind queue for batching background database writes
        let write_behind_queue = Arc::new(WriteBehindQueue::new(db_pool.clone()));

        // SES email service (uses default AWS config; From address must be verified in SES)
        let email_service = Arc::new(
            EmailService::new(config.from_email.clone())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create EmailService: {}", e))?,
        );

        Ok(Self {
            config,
            db_pool,
            redis,
            ssm,
            cache,
            write_behind_queue,
            email_service,
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

    tracing::debug!("Connecting to Redis at {}...", display_url);

    let connect_result =
        tokio::time::timeout(std::time::Duration::from_secs(timeout_seconds), async {
            RedisClient::new(redis_url, environment.to_string(), pool_size, min_idle).await
        })
        .await;

    match connect_result {
        Ok(Ok(client)) => match client.ping().await {
            Ok(_) => Ok(Some(Arc::new(client))),
            Err(e) => {
                tracing::warn!("Redis ping failed: {}. Continuing without Redis cache.", e);
                Ok(None)
            }
        },
        Ok(Err(e)) => {
            tracing::warn!("Redis connection failed: {}. Retrying in 2s...", e);
            if is_local_dev {
                tracing::warn!("Redis unavailable in local dev; continuing without cache.");
                return Ok(None);
            }

            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            tracing::debug!("Retrying Redis connection...");
            match tokio::time::timeout(
                std::time::Duration::from_secs(timeout_seconds),
                RedisClient::new(redis_url, environment.to_string(), pool_size, min_idle),
            )
            .await
            {
                Ok(Ok(client)) => match client.ping().await {
                    Ok(_) => Ok(Some(Arc::new(client))),
                    Err(e2) => {
                        tracing::warn!(
                            "Redis ping failed on retry: {}. Continuing without Redis cache.",
                            e2
                        );
                        Ok(None)
                    }
                },
                Ok(Err(e2)) => {
                    tracing::warn!(
                        "Redis connection failed after retry: {}. Continuing without Redis cache.",
                        e2
                    );
                    Ok(None)
                }
                Err(_) => {
                    tracing::warn!(
                        "Redis connection timed out ({}s). Continuing without Redis cache.",
                        timeout_seconds
                    );
                    Ok(None)
                }
            }
        }
        Err(_) => {
            if is_local_dev {
                tracing::warn!(
                    "Redis connection timed out ({}s). Skipping Redis in local dev.",
                    timeout_seconds
                );
                return Ok(None);
            }

            tracing::warn!(
                "Redis connection timed out ({}s). Retrying in 2s...",
                timeout_seconds
            );
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            tracing::debug!("Retrying Redis connection...");
            match tokio::time::timeout(
                std::time::Duration::from_secs(timeout_seconds),
                RedisClient::new(redis_url, environment.to_string(), pool_size, min_idle),
            )
            .await
            {
                Ok(Ok(client)) => match client.ping().await {
                    Ok(_) => Ok(Some(Arc::new(client))),
                    Err(e2) => {
                        tracing::warn!(
                            "Redis ping failed on retry: {}. Continuing without Redis cache.",
                            e2
                        );
                        Ok(None)
                    }
                },
                Ok(Err(e2)) => {
                    tracing::warn!(
                        "Redis connection failed after retry: {}. Continuing without Redis cache.",
                        e2
                    );
                    Ok(None)
                }
                Err(_) => {
                    tracing::warn!(
                        "Redis connection timed out ({}s). Continuing without Redis cache.",
                        timeout_seconds
                    );
                    Ok(None)
                }
            }
        }
    }
}
