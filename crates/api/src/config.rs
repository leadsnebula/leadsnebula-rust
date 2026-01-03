use anyhow::Result;
use leadsnebula_core::SsmClient;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub environment: String,
    pub port: u16,
    pub database_url: String,
    #[allow(dead_code)] // Loaded but not directly used (passed to Redis client)
    pub redis_url: Option<String>,
    #[allow(dead_code)] // Loaded but not directly used (passed to Sentry)
    pub sentry_dsn: Option<String>,
    pub webauthn_rp_id: String,
    pub webauthn_rp_name: String,
    pub webauthn_origin: String,
}

impl AppConfig {
    pub async fn load() -> Result<Self> {
        // Load from environment or SSM
        let environment = std::env::var("ENVIRONMENT")
            .unwrap_or_else(|_| std::env::var("ENV").unwrap_or_else(|_| "development".to_string()));

        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse::<u16>()?;

        // Initialize SSM client (optional - will fall back to env vars if unavailable)
        let ssm_client = match SsmClient::new().await {
            Ok(client) => Arc::new(client),
            Err(e) => {
                tracing::warn!(
                    "SSM client initialization failed: {}. Falling back to environment variables.",
                    e
                );
                // Create a dummy client that will always return None, forcing env var fallback
                Arc::new(SsmClient::dummy())
            }
        };

        // Load secrets from SSM
        let database_url = load_database_url(&ssm_client, &environment).await?;
        let redis_url = load_redis_url(&ssm_client, &environment).await;
        let sentry_dsn = load_sentry_dsn(&ssm_client, &environment).await;

        // WebAuthn configuration
        let webauthn_rp_id = std::env::var("WEBAUTHN_RP_ID").unwrap_or_else(|_| {
            if environment == "development" {
                "localhost".to_string()
            } else {
                "dashboard.leadsnebula.com".to_string()
            }
        });
        let webauthn_rp_name =
            std::env::var("WEBAUTHN_RP_NAME").unwrap_or_else(|_| "LeadsNebula".to_string());
        let webauthn_origin = std::env::var("WEBAUTHN_ORIGIN").unwrap_or_else(|_| {
            if environment == "development" {
                "http://localhost:3000".to_string()
            } else {
                "https://dashboard.leadsnebula.com".to_string()
            }
        });

        Ok(Self {
            environment,
            port,
            database_url,
            redis_url,
            sentry_dsn,
            webauthn_rp_id,
            webauthn_rp_name,
            webauthn_origin,
        })
    }
}

async fn load_database_url(ssm: &SsmClient, env: &str) -> Result<String> {
    // Try SSM first (Rust-specific path)
    let path = format!("/leadsnebula/{}/rust/db/connection_url", env);
    if let Some(url) = ssm.get_parameter(&path).await? {
        return Ok(url);
    }

    // Only allow env var fallback in development
    if env == "development" {
        if let Ok(url) = std::env::var("DATABASE_URL") {
            tracing::warn!("Using DATABASE_URL from environment variable (development mode only)");
            return Ok(url);
        }
    }

    Err(anyhow::anyhow!("DATABASE_URL not found in SSM at {}. Production environments must use SSM Parameter Store.", path))
}

async fn load_redis_url(ssm: &SsmClient, env: &str) -> Option<String> {
    let path = format!("/leadsnebula/{}/rust/redis/connection_url", env);
    ssm.get_parameter(&path)
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("REDIS_URL").ok())
}

async fn load_sentry_dsn(ssm: &SsmClient, env: &str) -> Option<String> {
    let path = format!("/leadsnebula/{}/rust/sentry/dsn", env);
    ssm.get_parameter(&path)
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("SENTRY_DSN").ok())
}
