mod cache_warmup;
mod config;
mod middleware;
mod routes;

use config::AppState;
use middleware::{api_auth::api_key_auth_middleware, jwt_auth::jwt_auth_middleware};
use routes::{auth_routes, carina_routes, dashboard_routes, health_routes, pulsar_routes};
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

// Use mimalloc for faster allocations
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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

    #[cfg(feature = "profiling")]
    {
        use tracing_flame::FlameLayer;
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        let (flame_layer, _guard) = FlameLayer::with_file("./traces.folded").unwrap();

        // Note: For production, set RUST_LOG=warn in Fly.io environment to disable
        // feature-gated tracing and eliminate logging overhead (zero tracing overhead in production).
        // Keep RUST_LOG=debug or RUST_LOG=info for development/staging.
        let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| default_filter.into());

        let registry = tracing_subscriber::registry()
            .with(flame_layer)
            .with(env_filter);

        if use_json {
            registry
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_target(true)
                        .with_file(true)
                        .with_line_number(true),
                )
                .init();
        } else {
            registry
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_target(true)
                        .with_file(true)
                        .with_line_number(true),
                )
                .init();
        }

        info!("Tracing-flame profiling enabled - flamegraph will be written to ./traces.folded");
    }

    #[cfg(not(feature = "profiling"))]
    {
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

            // Clone state for background tasks before moving it into the router
            let state_for_warmup = state.clone();
            let state_for_periodic = Arc::new(state.clone());
            let write_behind_queue_for_shutdown = state.write_behind_queue.clone();

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

            // Pre-warm cache on startup (non-blocking, runs in background)
            tokio::spawn(async move {
                cache_warmup::pre_warm_cache(&state_for_warmup).await;
            });

            // Start periodic cache warm-up task (runs every 30 minutes)
            tokio::spawn(async move {
                cache_warmup::start_periodic_warmup(state_for_periodic).await;
            });

            // Setup shutdown signal handler to flush write-behind queue
            // Create shutdown signal once
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

            // Spawn signal handler task
            tokio::spawn(async move {
                let ctrl_c = async {
                    tokio::signal::ctrl_c()
                        .await
                        .expect("failed to install Ctrl+C handler");
                };

                #[cfg(unix)]
                let terminate = async {
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("failed to install signal handler")
                        .recv()
                        .await;
                };

                #[cfg(not(unix))]
                let terminate = std::future::pending::<()>();

                tokio::select! {
                    _ = ctrl_c => {},
                    _ = terminate => {},
                }

                #[cfg(feature = "tracing")]
                tracing::info!("Shutdown signal received, flushing write-behind queue...");
                // Flush with retry logic (3 attempts) to ensure no data loss on shutdown
                let mut _flush_success = false;
                for attempt in 1..=3 {
                    match write_behind_queue_for_shutdown.flush().await {
                        Ok(_) => {
                            #[cfg(feature = "tracing")]
                            tracing::info!(
                                "Write-behind queue flushed successfully on attempt {}",
                                attempt
                            );
                            _flush_success = true;
                            break;
                        }
                        Err(e) => {
                            #[cfg(feature = "tracing")]
                            tracing::warn!(
                                "Write-behind queue flush failed on attempt {}: {}",
                                attempt,
                                e
                            );
                            if attempt < 3 {
                                // Wait 1 second before retry
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                            } else {
                                // Final attempt failed - log error for monitoring
                                #[cfg(feature = "tracing")]
                                tracing::error!("Write-behind queue flush failed after 3 attempts - potential data loss");
                            }
                        }
                    }
                }

                // Signal shutdown complete
                let _ = shutdown_tx.send(());
            });

            // In axum 0.7, Router<AppState> supports into_make_service() when state is set
            axum::serve(listener, app.into_make_service())
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await?;
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to initialize application state: {}", e);
            tracing::warn!("Application starting in minimal mode - only /live endpoint available");
            tracing::warn!("Full functionality will be unavailable until configuration is fixed");

            // Alert to Sentry if configured
            #[cfg(feature = "sentry")]
            {
                sentry::capture_message(
                    &format!("DB pool init failed - running in minimal mode: {}", e),
                    sentry::Level::Fatal,
                );
            }
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
