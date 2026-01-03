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

    // Load configuration
    let state = AppState::new().await?;

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

    // Build application
    let app = axum::Router::new()
        .merge(health_routes())
        .merge(auth_routes())
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(CorsLayer::permissive()),
        );

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
