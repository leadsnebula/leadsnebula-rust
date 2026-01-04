mod config;
mod middleware;
mod routes;

use config::AppState;
use routes::{auth_routes, health_routes};
use std::net::SocketAddr;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables from .env.local if it exists (development only)
    // This allows local development without setting env vars manually
    // Production should use SSM Parameter Store or system environment variables
    // Try .env.local first, then fall back to .env if it exists
    if dotenv::from_filename(".env.local").is_err() {
        // .env.local doesn't exist, try .env as fallback
        let _ = dotenv::dotenv();
    }

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "leadsnebula_api=info,tower_http=info".into()),
        )
        .init();

    info!("Starting LeadsNebula API server...");

    // Load configuration - if it fails, app still starts with just /live endpoint
    let app = match AppState::new().await {
        Ok(state) => {
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

            // Build full application with all routes
            // Note: /live is included in health_routes for consistency
            axum::Router::new()
                .route("/live", axum::routing::get(routes::health::liveness_check))
                .merge(health_routes())
                .merge(auth_routes())
                .with_state(state)
                .layer(
                    ServiceBuilder::new()
                        .layer(TraceLayer::new_for_http())
                        .layer(CorsLayer::permissive()),
                )
        }
        Err(e) => {
            tracing::error!("Failed to initialize application state: {}", e);
            tracing::warn!("Application starting in minimal mode - only /live endpoint available");
            tracing::warn!("Full functionality will be unavailable until configuration is fixed");
            // App continues with just /live endpoint - this ensures health checks pass
            axum::Router::new().route(
                "/live",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "status": "alive",
                        "mode": "degraded",
                        "message": "Application state initialization failed - check logs",
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    }))
                }),
            )
        }
    };

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
