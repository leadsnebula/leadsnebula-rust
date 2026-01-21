mod config;
mod middleware;
mod routes;

use config::AppState;
use middleware::{api_auth::api_key_auth_middleware, jwt_auth::jwt_auth_middleware};
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
    // Check for .env.local existence OR ENVIRONMENT being development/dev
    // This allows local dev to work even if ENVIRONMENT isn't explicitly set
    let env_local_exists = std::path::Path::new(".env.local").exists();
    let is_local_dev = std::env::var("ENVIRONMENT")
        .map(|e| e == "development" || e == "dev")
        .unwrap_or(false)
        || env_local_exists;

    // Load environment variables from .env.local if it exists (development only)
    // This allows local development without setting env vars manually
    // Production should use SSM Parameter Store or system environment variables
    // Priority: .env.local (highest) > .env > system environment variables
    // This ensures local development doesn't interfere with production
    // Use a tolerant parser that skips invalid lines instead of failing completely
    if is_local_dev && env_local_exists {
        match load_env_local_tolerant(".env.local") {
            Ok(_loaded_count) => {
                // Variables loaded successfully
            }
            Err(_e) => {
                // .env.local exists but couldn't be read, try .env as fallback
                let _ = dotenvy::dotenv();
            }
        }
    } else {
        // .env.local doesn't exist or not in dev, try .env as fallback
        let _ = dotenvy::dotenv();
    }

    // Initialize tracing with comprehensive logging
    // Default to INFO level for all crates to ensure logs work in Fly.io/Grafana
    // RUST_LOG can override this if set (e.g., RUST_LOG=debug for verbose)
    let default_filter = "leadsnebula_api=info,leadsnebula_core=info,leadsnebula_utils=info,tower_http=info,sqlx=warn,redis=info";

    // Use JSON format by default for production (works better with Grafana/Fly.io)
    // Set RUST_LOG_JSON=0 to disable JSON and use pretty format
    let use_json = std::env::var("RUST_LOG_JSON")
        .map(|v| v != "0" && v.to_lowercase() != "false")
        .unwrap_or(true); // Default to JSON

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_filter.into()),
        )
        .with_target(true) // Include module path
        .with_file(true) // Include file name
        .with_line_number(true); // Include line numbers

    if use_json {
        subscriber.json().init();
    } else {
        subscriber.init();
    }

    info!("Starting LeadsNebula API server...");

    // Bind listener first - this allows the server to start accepting connections immediately
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 8080))).await?;
    info!("Server listening on 0.0.0.0:8080");

    // Initialize AppState - we need it for full functionality
    // But if it fails, we'll serve a minimal app with just /live endpoint for health checks
    info!("Initializing application state...");
    let app_state_result =
        tokio::time::timeout(std::time::Duration::from_secs(30), AppState::new()).await;

    match app_state_result {
        Ok(Ok(state)) => {
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

            // Build full application router with all routes
            // Following the pattern from commit b80b48b: merge routes first, then set state once
            info!("Building application router with all routes...");
            let app = axum::Router::new()
                .route("/live", axum::routing::get(routes::health::liveness_check))
                .merge(health_routes())
                .merge(auth_routes())
                .merge(
                    dashboard_routes().layer(axum::middleware::from_fn_with_state(
                        state.clone(),
                        jwt_auth_middleware,
                    )),
                )
                .merge(carina_routes().layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    api_key_auth_middleware,
                )))
                .merge(pulsar_routes()) // No authentication middleware - Pulsar is internal
                .with_state(state)
                .layer(
                    ServiceBuilder::new()
                        .layer(TraceLayer::new_for_http())
                        .layer(
                            CorsLayer::new()
                                .allow_origin(tower_http::cors::Any)
                                .allow_methods(tower_http::cors::Any)
                                .allow_headers([
                                    axum::http::header::AUTHORIZATION,
                                    axum::http::header::CONTENT_TYPE,
                                    axum::http::HeaderName::from_static("x-api-key"),
                                    axum::http::HeaderName::from_static("x-hmac-signature"),
                                ])
                                .expose_headers(tower_http::cors::Any),
                        ),
                );

            info!("All routes are now available, including /api/auth/login");
            // In axum 0.7, Router<AppState> supports into_make_service() when state is set
            axum::serve(listener, app.into_make_service()).await?;
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to initialize application state: {}", e);
            tracing::warn!("Application starting in minimal mode - only /live endpoint available");
            tracing::warn!("Full functionality will be unavailable until configuration is fixed");
            // Serve minimal app with just /live endpoint for health checks
            // This ensures health checks pass even if AppState initialization fails
            let minimal_app = axum::Router::new()
                .route("/live", axum::routing::get(routes::health::liveness_check))
                .layer(
                    ServiceBuilder::new()
                        .layer(TraceLayer::new_for_http())
                        .layer(
                            CorsLayer::new()
                                .allow_origin(tower_http::cors::Any)
                                .allow_methods(tower_http::cors::Any)
                                .allow_headers([
                                    axum::http::header::AUTHORIZATION,
                                    axum::http::header::CONTENT_TYPE,
                                    axum::http::HeaderName::from_static("x-api-key"),
                                    axum::http::HeaderName::from_static("x-hmac-signature"),
                                ])
                                .expose_headers(tower_http::cors::Any),
                        ),
                );
            // In axum 0.7, Router<()> supports into_make_service()
            axum::serve(listener, minimal_app.into_make_service()).await?;
        }
        Err(_) => {
            tracing::error!("AppState initialization timed out after 30 seconds");
            tracing::warn!("Application starting in minimal mode - only /live endpoint available");
            tracing::warn!("Full functionality will be unavailable until configuration is fixed");
            // Serve minimal app with just /live endpoint for health checks
            let minimal_app = axum::Router::new()
                .route("/live", axum::routing::get(routes::health::liveness_check))
                .layer(
                    ServiceBuilder::new()
                        .layer(TraceLayer::new_for_http())
                        .layer(
                            CorsLayer::new()
                                .allow_origin(tower_http::cors::Any)
                                .allow_methods(tower_http::cors::Any)
                                .allow_headers([
                                    axum::http::header::AUTHORIZATION,
                                    axum::http::header::CONTENT_TYPE,
                                    axum::http::HeaderName::from_static("x-api-key"),
                                    axum::http::HeaderName::from_static("x-hmac-signature"),
                                ])
                                .expose_headers(tower_http::cors::Any),
                        ),
                );
            // In axum 0.7, Router<()> supports into_make_service()
            axum::serve(listener, minimal_app.into_make_service()).await?;
        }
    }

    Ok(())
}
