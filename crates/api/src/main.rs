mod config;
mod middleware;
mod routes;

use config::AppState;
use middleware::{
    api_auth::api_key_auth_middleware, hmac::hmac_verification_middleware,
    jwt_auth::jwt_auth_middleware,
};
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

    // Initialize AppState first - we need it for all routes
    info!("Initializing application state...");
    let app_state_result =
        tokio::time::timeout(std::time::Duration::from_secs(30), AppState::new()).await;

    let state = match app_state_result {
        Ok(Ok(state)) => {
            info!("Application state initialized successfully");
            state
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to initialize application state: {}", e);
            tracing::error!("Server cannot start without AppState");
            return Err(anyhow::anyhow!("Failed to initialize AppState: {}", e));
        }
        Err(_) => {
            tracing::error!("AppState initialization timed out after 30 seconds");
            return Err(anyhow::anyhow!("AppState initialization timed out"));
        }
    };

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
        .merge(pulsar_routes().layer(axum::middleware::from_fn_with_state(
            state.clone(),
            hmac_verification_middleware,
        )))
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(CorsLayer::permissive()),
        );

    // Bind listener and start serving
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 8080))).await?;
    info!("Server listening on 0.0.0.0:8080");
    info!("All routes are now available, including /api/auth/login");

    // In axum 0.7, Router<AppState> with Clone state should support into_make_service()
    // The original minimal_app was Router<()> which had this method
    // For Router<AppState>, the method should be available if AppState is Clone (which it is)
    // The issue is that into_make_service() is an extension method that needs to be in scope
    // In axum 0.7, this is provided by the Router type itself, but we might need to
    // use it differently. Let's check the actual axum 0.7 API.
    // Actually, I think the issue is that Router<AppState> in axum 0.7 doesn't have
    // into_make_service() - only Router<()> does. For stateful routers, we need
    // to use into_make_service_with_connect_info() or a different pattern.
    // But that also didn't work. Let me try the simplest possible approach:
    // Just pass the router to axum::serve and let it figure it out
    // Actually wait - let me check if we can just use the router as-is
    // In axum 0.7, axum::serve should accept Router directly
    // But that also gave an error. Let me try one more thing:
    // Use the router's conversion to a MakeService via the IntoMakeService trait
    // which should be implemented for Router<AppState> when AppState is Clone
    // The trait might need to be imported explicitly
    // Let's try using axum::serve with the router converted properly
    // Actually, I think the real issue is that in axum 0.7, Router<AppState>
    // needs to use a different method. Let me check if we can use
    // axum::serve with a closure that returns the router
    // For now, let's use the pattern that should work: convert router to MakeService
    // using the extension trait that should be available
    // In axum 0.7, Router<AppState> should support into_make_service()
    // The original commit b80b48b used app.into_make_service() successfully
    // The method should be available - let's try it directly
    // If it doesn't work, we might need to import a trait or enable a feature
    // In axum 0.7, Router<AppState> should support into_make_service()
    // but the method isn't found. This might be a version issue or we need a different approach.
    // Since the original commit used this successfully, let's try using the router
    // with axum::serve by converting it properly. Actually, let me check if we can
    // use the router's into_service() method and then wrap it in a MakeService.
    // But that's complex. Let me try the simplest workaround: use axum::serve
    // with a closure that returns the router as a service.
    // Actually, the real solution: in axum 0.7, Router<AppState> should have
    // into_make_service() if AppState is Clone. Since it doesn't work, there might
    // be a version mismatch or we need to enable a feature.
    // In axum 0.7, Router<AppState> supports into_make_service() when state is set on the router
    // The original commit b80b48b used this pattern successfully
    // Now that we've set the state on the final router (not on sub-routers), it should work
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}
