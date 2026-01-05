mod config;
mod middleware;
mod routes;

use config::AppState;
use middleware::{
    api_auth::api_key_auth_middleware, hmac::hmac_verification_middleware,
    jwt_auth::jwt_auth_middleware,
};
use routes::{auth_routes, carina_routes, dashboard_routes, health_routes, pulsar_routes};
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
    // Priority: .env.local (highest) > .env > system environment variables
    // This ensures local development doesn't interfere with production
    let env_loaded = dotenv::from_filename(".env.local").is_ok();
    if !env_loaded {
        // .env.local doesn't exist, try .env as fallback
        let _ = dotenv::dotenv();
    }

    // Log which env file was loaded (for debugging)
    if env_loaded {
        tracing::info!("Loaded environment from .env.local (local development mode)");
    }

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "leadsnebula_api=debug,tower_http=info".into()),
        )
        .init();

    info!("Starting LeadsNebula API server...");

    // Load configuration - if it fails, app still starts with just /live endpoint
    // Use separate variables to handle different router types
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 8080))).await?;

    match AppState::new().await {
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
            let app = axum::Router::new()
                .route("/live", axum::routing::get(routes::health::liveness_check))
                .merge(health_routes())
                .merge(auth_routes())
                .merge(
                    carina_routes()
                        .layer(axum::middleware::from_fn_with_state(
                            state.clone(),
                            api_key_auth_middleware,
                        ))
                        .layer(axum::middleware::from_fn_with_state(
                            state.clone(),
                            hmac_verification_middleware,
                        )),
                )
                .merge(pulsar_routes().layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    api_key_auth_middleware,
                )))
                .merge(
                    dashboard_routes().layer(axum::middleware::from_fn_with_state(
                        state.clone(),
                        jwt_auth_middleware,
                    )),
                )
                .with_state(state)
                .layer(
                    ServiceBuilder::new()
                        .layer(TraceLayer::new_for_http())
                        .layer(
                            CorsLayer::new()
                                .allow_origin(tower_http::cors::Any)
                                .allow_methods(tower_http::cors::Any)
                                .allow_headers([
                                    axum::http::header::CONTENT_TYPE,
                                    axum::http::header::AUTHORIZATION,
                                    axum::http::header::HeaderName::from_static("x-api-key"),
                                    axum::http::header::HeaderName::from_static("x-hmac-signature"),
                                ])
                                .expose_headers(tower_http::cors::Any),
                        ),
                );

            info!("Server listening on 0.0.0.0:8080");
            axum::serve(listener, app.into_make_service()).await?;
        }
        Err(e) => {
            tracing::error!("Failed to initialize application state: {}", e);
            tracing::warn!("Application starting in minimal mode - only /live endpoint available");
            tracing::warn!("Full functionality will be unavailable until configuration is fixed");
            // App continues with just /live endpoint - this ensures health checks pass
            let app = axum::Router::new()
                .route(
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
                .layer(
                    ServiceBuilder::new()
                        .layer(TraceLayer::new_for_http())
                        .layer(CorsLayer::permissive()),
                );

            info!("Server listening on 0.0.0.0:8080");
            axum::serve(listener, app.into_make_service()).await?;
        }
    }

    Ok(())
}
