use axum::{routing::get, Router};
use sentry_tower::NewSentryLayer;
use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auth;
mod config;
mod middleware;
mod routes;
mod webauthn_challenge;

use auth::{auth_middleware, AuthState};
use config::AppConfig;
use leadsnebula_core::{create_redis_client, JwtSecret, SsmClient, WebauthnService};
use leadsnebula_services::database::create_pool;
use middleware::rate_limit::{rate_limit_middleware, RateLimitConfig, RateLimitState};
use middleware::rls::{rls_middleware, RlsState};
use routes::{auth as auth_routes, health, security as security_routes};
use std::sync::Arc;

// Test handler to verify Sentry is working (remove after testing)
async fn test_sentry() -> &'static str {
    // Explicitly capture the error in Sentry before panicking
    sentry::capture_message(
        "Sentry test error - this should appear in Sentry dashboard",
        sentry::Level::Error,
    );
    panic!("Sentry test error - this should appear in Sentry dashboard");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env.local for local development (gitignored, not committed)
    // This allows local dev without AWS SSM, but production always uses SSM
    let _ = dotenvy::from_filename(".env.local").ok();

    // Map SES_* env vars to AWS_* for AWS SDK (if SES_* are set and AWS_* are not)
    // This ensures the AWS SDK can find credentials for email sending
    if std::env::var("AWS_ACCESS_KEY_ID").is_err() {
        if let Ok(ses_key) = std::env::var("SES_ACCESS_KEY_ID") {
            std::env::set_var("AWS_ACCESS_KEY_ID", ses_key);
        }
    }
    if std::env::var("AWS_SECRET_ACCESS_KEY").is_err() {
        if let Ok(ses_secret) = std::env::var("SES_SECRET_ACCESS_KEY") {
            std::env::set_var("AWS_SECRET_ACCESS_KEY", ses_secret);
        }
    }
    if std::env::var("AWS_REGION").is_err() {
        if let Ok(ses_region) = std::env::var("SES_REGION") {
            std::env::set_var("AWS_REGION", ses_region);
        }
    }

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "leadsnebula_api=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Initialize Sentry
    // Use SENTRY_DSN environment variable if set, otherwise use default DSN
    let sentry_dsn = std::env::var("SENTRY_DSN")
        .unwrap_or_else(|_| "https://a27d171d9549d28e48fb2a492519a33b@o4510625270857728.ingest.us.sentry.io/4510625315553280".to_string());

    info!("Initializing Sentry with DSN");
    let _sentry_guard = sentry::init((
        sentry_dsn,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            // Capture user IPs and potentially sensitive headers when using HTTP server integrations
            // see https://docs.sentry.io/platforms/rust/data-management/data-collected for more info
            send_default_pii: true,
            ..Default::default()
        },
    ));

    // Panic hook is automatically installed by sentry::init()

    // Load configuration
    let config = AppConfig::load().await?;
    info!("Configuration loaded: environment={}", config.environment);

    // Create database connection pool
    let pool = create_pool(&config.database_url).await?;
    info!("Database connection pool created");

    // Load JWT secret from SSM or environment
    // Handle SSM client creation failure gracefully (for local dev)
    let ssm_client_temp = match SsmClient::new().await {
        Ok(client) => client,
        Err(e) => {
            tracing::warn!("SSM client initialization failed in main: {}. Using dummy client for local development.", e);
            SsmClient::dummy()
        }
    };

    // Initialize Redis client (optional - for rate limiting and caching)
    let redis_client = create_redis_client(Some(&ssm_client_temp), &config.environment).await?;
    let redis_arc = redis_client.as_ref().map(|r| Arc::new(r.clone()));

    // Enhance SSM client with Redis caching if available
    let ssm_client = Arc::new(if let Some(redis) = &redis_arc {
        ssm_client_temp.with_redis(Some(redis.clone()))
    } else {
        ssm_client_temp
    });

    if redis_arc.is_some() {
        info!("SSM client enhanced with Redis caching");
    }
    let jwt_secret_path = format!("/leadsnebula/{}/rust/jwt/secret_key", config.environment);
    let jwt_secret = match ssm_client.get_parameter(&jwt_secret_path).await {
        Ok(Some(secret)) => JwtSecret::new(secret),
        Ok(None) => {
            // Only allow env var fallback in development
            if config.environment == "development" {
                if let Ok(secret) = std::env::var("JWT_SECRET_KEY") {
                    tracing::warn!(
                        "Using JWT_SECRET_KEY from environment variable (development mode only)"
                    );
                    JwtSecret::new(secret)
                } else {
                    return Err(anyhow::anyhow!(
                        "JWT_SECRET_KEY not found in SSM at {} or environment",
                        jwt_secret_path
                    ));
                }
            } else {
                return Err(anyhow::anyhow!("JWT_SECRET_KEY not found in SSM at {}. Production environments must use SSM Parameter Store.", jwt_secret_path));
            }
        }
        Err(e) => {
            // Only allow fallback in development if SSM client failed
            if config.environment == "development" {
                if let Ok(secret) = std::env::var("JWT_SECRET_KEY") {
                    tracing::warn!("SSM unavailable, using JWT_SECRET_KEY from environment variable (development mode only): {}", e);
                    JwtSecret::new(secret)
                } else {
                    return Err(anyhow::anyhow!(
                        "JWT_SECRET_KEY not found: SSM error ({}) and no environment variable",
                        e
                    ));
                }
            } else {
                return Err(anyhow::anyhow!("Failed to load JWT_SECRET_KEY from SSM at {}: {}. Production environments must use SSM Parameter Store.", jwt_secret_path, e));
            }
        }
    };
    info!("JWT secret loaded");

    // Initialize WebAuthn service
    let webauthn_service = Arc::new(
        WebauthnService::new(
            &config.webauthn_rp_id,
            &config.webauthn_rp_name,
            &config.webauthn_origin,
        )
        .map_err(|e| anyhow::anyhow!("Failed to initialize WebAuthn: {}", e))?,
    );
    info!(
        "WebAuthn service initialized: rp_id={}, origin={}",
        config.webauthn_rp_id, config.webauthn_origin
    );

    // Create RLS state
    let pool_arc = Arc::new(pool.clone());
    let rls_state = RlsState {
        pool: pool_arc.clone(),
    };

    // Create challenge store for WebAuthn
    let challenge_store = Arc::new(webauthn_challenge::ChallengeStore::new());

    // Create auth state
    let auth_state = AuthState {
        pool: pool_arc.clone(),
        jwt_secret: Arc::new(jwt_secret),
        webauthn: webauthn_service,
        challenge_store,
    };

    // Create rate limiting state
    let max_requests = 100; // 100 requests
    let window_seconds = 60; // per minute
    let use_redis = redis_arc.is_some();
    let rate_limit_config = RateLimitConfig {
        max_requests,
        window_seconds,
        use_redis,
    };
    let rate_limit_state = RateLimitState::new(redis_arc.clone(), rate_limit_config);
    info!(
        "Rate limiting configured: {} requests per {} seconds (Redis: {})",
        max_requests, window_seconds, use_redis
    );

    // Build application router
    // Order matters: auth middleware must run before RLS middleware
    let app = Router::new()
        .route("/health", get(health::health_check))
        .route("/api/health", get(health::health_check))
        // Test endpoint to verify Sentry is working (remove after testing)
        .route("/api/test-sentry", get(test_sentry))
        .route("/api/auth/login", axum::routing::post(auth_routes::login))
        .route(
            "/api/auth/verify-otp-login",
            axum::routing::post(auth_routes::verify_otp_login),
        )
        .route(
            "/api/auth/register",
            axum::routing::post(auth_routes::register),
        )
        .route(
            "/api/auth/change-password",
            axum::routing::post(auth_routes::change_password),
        )
        .route(
            "/api/auth/forgot-password",
            axum::routing::post(security_routes::forgot_password),
        )
        .route(
            "/api/security",
            axum::routing::get(security_routes::get_security_info),
        )
        .route(
            "/api/security/password-reset-email",
            axum::routing::post(security_routes::request_password_reset_email),
        )
        .route(
            "/api/security/otp/setup",
            axum::routing::post(security_routes::setup_otp),
        )
        .route(
            "/api/security/otp/verify",
            axum::routing::post(security_routes::verify_otp),
        )
        .route(
            "/api/security/otp/disable",
            axum::routing::post(security_routes::disable_otp),
        )
        .route(
            "/api/security/passkeys/registration_options",
            axum::routing::post(security_routes::passkey_registration_options),
        )
        .route(
            "/api/security/passkeys/register",
            axum::routing::post(security_routes::register_passkey),
        )
        .route(
            "/api/security/passkeys/:id",
            axum::routing::delete(security_routes::delete_passkey),
        )
        .layer(axum::middleware::from_fn_with_state(
            pool_arc.clone(),
            auth::api_key::api_key_auth_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            auth_state.clone(),
            auth_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            rls_state.clone(),
            rls_middleware,
        ))
        // Rate limiting middleware (applied to all routes)
        .layer(axum::middleware::from_fn_with_state(
            rate_limit_state.clone(),
            rate_limit_middleware,
        ))
        .layer(NewSentryLayer::new_from_top())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers([
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::AUTHORIZATION,
                    axum::http::HeaderName::from_static("x-api-key"),
                ])
                .expose_headers(tower_http::cors::Any),
        )
        .with_state(auth_state);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
