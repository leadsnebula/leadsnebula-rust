// Note: This module is currently unused because we only serve minimal app with /live endpoint
// It will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

use axum::{extract::State, http::StatusCode, response::Json, routing::post, Router};
use leadsnebula_core::auth::{verify_password, JwtService};
use leadsnebula_core::models::user::User;
use serde::{Deserialize, Serialize};

use crate::AppState;

#[derive(Deserialize)]
pub struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    success: bool,
    token: Option<String>,
    user: Option<UserResponse>,
    error: Option<String>,
}

#[derive(Serialize)]
pub struct UserResponse {
    id: String,
    email: String,
    first_name: Option<String>,
    last_name: Option<String>,
}

pub fn auth_routes() -> Router<AppState> {
    Router::new().route("/api/auth/login", post(login))
}

async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    use tracing::{error, info, warn};

    info!("Login attempt for email: {}", payload.email);

    // Find user by email (case-insensitive)
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM instance_users WHERE LOWER(email) = LOWER($1) LIMIT 1",
    )
    .bind(&payload.email)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error during login lookup: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let user = match user {
        Some(u) => {
            info!(
                "User found: {} (ID: {}, Status: {}, Confirmed: {})",
                u.email,
                u.id,
                u.status,
                u.is_confirmed()
            );
            u
        }
        None => {
            warn!("Login failed: User not found for email: {}", payload.email);
            return Ok(Json(LoginResponse {
                success: false,
                token: None,
                user: None,
                error: Some("Invalid email or password".to_string()),
            }));
        }
    };

    // Check if user is confirmed
    if !user.is_confirmed() {
        warn!("Login failed: User {} not confirmed", user.email);
        return Ok(Json(LoginResponse {
            success: false,
            token: None,
            user: None,
            error: Some("Please confirm your email address before signing in".to_string()),
        }));
    }

    // Check if user is active
    if !user.is_active() {
        warn!(
            "Login failed: User {} is not active (status: {})",
            user.email, user.status
        );
        return Ok(Json(LoginResponse {
            success: false,
            token: None,
            user: None,
            error: Some("Your account has been suspended".to_string()),
        }));
    }

    // Verify password
    info!("Verifying password for user: {}", user.email);
    info!(
        "Password hash prefix: {}...",
        &user.encrypted_password.chars().take(20).collect::<String>()
    );

    let password_valid = match verify_password(&payload.password, &user.encrypted_password) {
        Ok(valid) => {
            info!("Password verification result: {}", valid);
            valid
        }
        Err(e) => {
            error!("Password verification error: {}", e);
            false
        }
    };

    if !password_valid {
        warn!("Login failed: Invalid password for user: {}", user.email);
        return Ok(Json(LoginResponse {
            success: false,
            token: None,
            user: None,
            error: Some("Invalid email or password".to_string()),
        }));
    }

    info!("Login successful for user: {}", user.email);

    // Generate JWT token
    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let token = jwt_service
        .encode(user.id.to_string(), user.email.clone())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(LoginResponse {
        success: true,
        token: Some(token),
        user: Some(UserResponse {
            id: user.id.to_string(),
            email: user.email,
            first_name: user.first_name,
            last_name: user.last_name,
        }),
        error: None,
    }))
}
