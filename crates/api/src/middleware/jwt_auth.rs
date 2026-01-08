// Note: This middleware is currently unused because we only serve minimal app with /live endpoint
// It will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

use crate::AppState;
use axum::{
    extract::{Request, State},
    http::{header::HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use leadsnebula_core::auth::JwtService;
use leadsnebula_core::models::user::User;
use uuid::Uuid;

pub async fn jwt_auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // #region agent log
    let path = request.uri().path().to_string();
    let method = request.method().to_string();
    let log_data = serde_json::json!({
        "sessionId": "debug-session",
        "runId": "run1",
        "hypothesisId": "A",
        "location": "jwt_auth.rs:16",
        "message": "JWT middleware entry",
        "data": {"path": path, "method": method},
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/home/badinoff/projects/leadsNebula/ruby/.cursor/debug.log")
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(
                f,
                "{}",
                serde_json::to_string(&log_data).unwrap_or_default()
            )
        });
    // #endregion

    // Extract token from Authorization header
    let token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    let token = match token {
        Some(t) => t,
        None => {
            // #region agent log
            let log_data = serde_json::json!({
                "sessionId": "debug-session",
                "runId": "run1",
                "hypothesisId": "B",
                "location": "jwt_auth.rs:35",
                "message": "Missing Authorization header",
                "data": {"path": path},
                "timestamp": chrono::Utc::now().timestamp_millis()
            });
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("/home/badinoff/projects/leadsNebula/ruby/.cursor/debug.log")
                .and_then(|mut f| {
                    use std::io::Write;
                    writeln!(
                        f,
                        "{}",
                        serde_json::to_string(&log_data).unwrap_or_default()
                    )
                });
            // #endregion
            tracing::warn!("Missing Authorization header");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // Decode JWT
    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let claims = jwt_service.decode(&token).map_err(|e| {
        // #region agent log
        let log_data = serde_json::json!({
            "sessionId": "debug-session",
            "runId": "run1",
            "hypothesisId": "B",
            "location": "jwt_auth.rs:42",
            "message": "JWT decode error",
            "data": {"error": e.to_string(), "path": path},
            "timestamp": chrono::Utc::now().timestamp_millis()
        });
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/home/badinoff/projects/leadsNebula/ruby/.cursor/debug.log")
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(
                    f,
                    "{}",
                    serde_json::to_string(&log_data).unwrap_or_default()
                )
            });
        // #endregion
        tracing::warn!("JWT decode error: {}", e);
        StatusCode::UNAUTHORIZED
    })?;

    // Load user from database
    // Note: instance_users table uses 'status' column, not 'deleted_at'
    // Filter by status = 'active' instead of deleted_at IS NULL
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM instance_users WHERE id = $1 AND status = 'active'",
    )
    .bind(Uuid::parse_str(&claims.user_id).map_err(|_| StatusCode::UNAUTHORIZED)?)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        // #region agent log
        let log_data = serde_json::json!({
            "sessionId": "debug-session",
            "runId": "run1",
            "hypothesisId": "E",
            "location": "jwt_auth.rs:55",
            "message": "Database error during user lookup",
            "data": {"error": e.to_string(), "path": path},
            "timestamp": chrono::Utc::now().timestamp_millis()
        });
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/home/badinoff/projects/leadsNebula/ruby/.cursor/debug.log")
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(
                    f,
                    "{}",
                    serde_json::to_string(&log_data).unwrap_or_default()
                )
            });
        // #endregion
        tracing::error!("Database error during user lookup: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    // Check if user is active
    if !user.is_active() {
        tracing::warn!("User {} is not active", user.id);
        return Err(StatusCode::UNAUTHORIZED);
    }

    // #region agent log
    let user_id = user.id.to_string();
    let log_data = serde_json::json!({
        "sessionId": "debug-session",
        "runId": "run1",
        "hypothesisId": "A",
        "location": "jwt_auth.rs:68",
        "message": "JWT middleware exit, calling next",
        "data": {"path": path, "method": method, "user_id": user_id},
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/home/badinoff/projects/leadsNebula/ruby/.cursor/debug.log")
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(
                f,
                "{}",
                serde_json::to_string(&log_data).unwrap_or_default()
            )
        });
    // #endregion

    // Attach user to request extensions
    request.extensions_mut().insert(user);

    let response = next.run(request).await;

    // #region agent log
    let status = response.status();
    let log_data = serde_json::json!({
        "sessionId": "debug-session",
        "runId": "run1",
        "hypothesisId": "A",
        "location": "jwt_auth.rs:72",
        "message": "JWT middleware after next.run",
        "data": {"path": path, "method": method, "status": status.as_u16()},
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/home/badinoff/projects/leadsNebula/ruby/.cursor/debug.log")
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(
                f,
                "{}",
                serde_json::to_string(&log_data).unwrap_or_default()
            )
        });
    // #endregion

    Ok(response)
}
