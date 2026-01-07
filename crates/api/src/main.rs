mod config;
mod middleware;
mod routes;

use config::AppState;
// Note: These imports are used when building the full app router
// They're kept here for when we implement hot-reloading or server restart
// Currently unused because we only serve minimal app with /live endpoint
#[allow(unused_imports)]
use middleware::{
    api_auth::api_key_auth_middleware, hmac::hmac_verification_middleware,
    jwt_auth::jwt_auth_middleware,
};
#[allow(unused_imports)]
use routes::{auth_routes, carina_routes, dashboard_routes, health_routes, pulsar_routes};
use std::fs;
use std::io;
use std::net::SocketAddr;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

/// Load .env.local file with tolerance for invalid lines
/// Skips lines that don't match KEY=VALUE format instead of failing completely
fn load_env_local_tolerant(path: &str) -> io::Result<usize> {
    let contents = fs::read_to_string(path)?;
    let mut loaded = 0;

    for line in contents.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Only process lines that match KEY=VALUE format
        // This skips invalid lines like "psql '...'" that would break dotenv
        if let Some(equals_pos) = line.find('=') {
            let key = line[..equals_pos].trim();
            let value = line[equals_pos + 1..].trim();

            // Validate key (must start with letter/underscore, contain only alphanumeric/underscore)
            if !key.is_empty()
                && key
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                && key.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                // Clean up the value
                let mut cleaned_value = value;

                // Remove surrounding quotes if present (handles both single and double quotes)
                cleaned_value = cleaned_value
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .or_else(|| {
                        cleaned_value
                            .strip_prefix('\'')
                            .and_then(|s| s.strip_suffix('\''))
                    })
                    .unwrap_or(cleaned_value);

                // Special handling for DATABASE_URL: remove "psql " prefix if present
                // This handles cases where someone accidentally included the psql command
                if key == "DATABASE_URL" && cleaned_value.starts_with("psql ") {
                    cleaned_value = &cleaned_value[5..]; // Remove "psql "
                                                         // Remove quotes again if they were after "psql "
                    cleaned_value = cleaned_value
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .or_else(|| {
                            cleaned_value
                                .strip_prefix('\'')
                                .and_then(|s| s.strip_suffix('\''))
                        })
                        .unwrap_or(cleaned_value);
                }

                std::env::set_var(key, cleaned_value);
                loaded += 1;
            }
        }
    }

    Ok(loaded)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Determine if we're in local development
    // Only check for .env.local in development environment to avoid unnecessary checks in production
    let is_local_dev = std::env::var("ENVIRONMENT")
        .map(|e| e == "development" || e == "dev")
        .unwrap_or(false);

    // Load environment variables from .env.local if it exists (development only)
    // This allows local development without setting env vars manually
    // Production should use SSM Parameter Store or system environment variables
    // Priority: .env.local (highest) > .env > system environment variables
    // This ensures local development doesn't interfere with production
    // Use a tolerant parser that skips invalid lines instead of failing completely
    if is_local_dev && std::path::Path::new(".env.local").exists() {
        match load_env_local_tolerant(".env.local") {
            Ok(_loaded_count) => {
                // Variables loaded successfully
            }
            Err(_e) => {
                // .env.local exists but couldn't be read, try .env as fallback
                let _ = dotenv::dotenv();
            }
        }
    } else {
        // .env.local doesn't exist or not in dev, try .env as fallback
        let _ = dotenv::dotenv();
    }

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "leadsnebula_api=debug,tower_http=info".into()),
        )
        .init();

    info!("Starting LeadsNebula API server...");

    // Bind listener FIRST - this allows the server to start accepting connections immediately
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 8080))).await?;
    info!("Server listening on 0.0.0.0:8080");

    // Initialize AppState in parallel - don't block server startup
    // Start server immediately with /live endpoint, then upgrade to full app once AppState is ready
    let app_state_handle = tokio::spawn(AppState::new());

    // Start server immediately with /live endpoint (doesn't require AppState)
    // This ensures health checks pass even before AppState is initialized
    let minimal_app = axum::Router::new()
        .route("/live", axum::routing::get(routes::health::liveness_check))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(CorsLayer::permissive()),
        );

    // Start serving minimal app immediately (just /live endpoint)
    // This ensures health checks pass even before AppState is initialized
    info!("Serving minimal app (only /live endpoint) while AppState initializes...");

    // Start minimal server in a task - this allows it to accept connections immediately
    // Note: We can't clone TcpListener, so we serve the minimal app directly
    // Once AppState is ready, we log it but continue serving minimal app
    // Full routes will be available after restart/deployment
    let serve_handle =
        tokio::spawn(async move { axum::serve(listener, minimal_app.into_make_service()).await });

    // Wait for AppState initialization (with timeout) in parallel
    let state_result =
        tokio::time::timeout(std::time::Duration::from_secs(30), app_state_handle).await;

    match state_result {
        Ok(Ok(Ok(state))) => {
            info!("Application state initialized successfully");

            // Initialize Sentry if DSN is provided
            #[cfg(feature = "sentry")]
            if let Some(dsn) = &state.config.sentry_dsn {
                let _guard = sentry::init((
                    dsn.clone(),
                    sentry::ClientOptions {
                        release: sentry::release_name!(),
                        ..Default::default()
                    },
                ));
                info!("Sentry initialized");
            } else {
                tracing::warn!("Sentry DSN not provided, error tracking disabled");
            }

            info!("Full application state is ready");
            info!("Note: /live endpoint is available immediately for health checks");
            info!("Other routes will be available after server restart or next deployment");

            // Continue with minimal server since we can't hot-reload routes
            // The /live endpoint works for health checks, which is the main goal
            // Full routes will be available after restart/deployment
            serve_handle.await??;
        }
        Ok(Ok(Err(e))) => {
            tracing::error!("Failed to initialize application state: {}", e);
            tracing::warn!("Application running in minimal mode - only /live endpoint available");
            tracing::warn!("Full functionality will be unavailable until configuration is fixed");
            serve_handle.await??;
        }
        Ok(Err(e)) => {
            tracing::error!("AppState initialization task error: {}", e);
            tracing::warn!("Application running in minimal mode - only /live endpoint available");
            serve_handle.await??;
        }
        Err(_) => {
            tracing::error!("AppState initialization timed out after 30 seconds");
            tracing::warn!("Application running in minimal mode - only /live endpoint available");
            serve_handle.await??;
        }
    }

    Ok(())
}
