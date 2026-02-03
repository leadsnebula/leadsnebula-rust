// Note: This module is currently unused because we only serve minimal app with /live endpoint
// It will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

use axum::{extract::State, http::StatusCode, response::Json, routing::post, Router};
use leadsnebula_core::auth::{verify_password, JwtService};
use leadsnebula_core::models::user::User;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    requires_otp: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    login_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Serialize)]
pub struct UserResponse {
    id: String,
    email: String,
    first_name: Option<String>,
    last_name: Option<String>,
}

#[derive(Deserialize)]
pub struct VerifyOtpLoginRequest {
    login_token: String,
    otp_code: Option<String>,
    backup_code: Option<String>,
}

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/verify-otp-login", post(verify_otp_login))
}

async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    use std::time::Instant;
    use tracing::{error, info, warn};

    let start = Instant::now();
    info!("Login request received for {}", payload.email);

    // Find user by email (case-insensitive)
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM instance_users WHERE LOWER(email) = LOWER($1) LIMIT 1",
    )
    .bind(&payload.email)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error during login lookup: {}", e);
        info!(
            duration_ms = start.elapsed().as_millis(),
            "Login completed in {}ms (success=false, error=db_lookup)",
            start.elapsed().as_millis()
        );
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
            info!(
                duration_ms = start.elapsed().as_millis(),
                "Login completed in {}ms (success=false, reason=user_not_found)",
                start.elapsed().as_millis()
            );
            return Ok(Json(LoginResponse {
                success: false,
                token: None,
                user: None,
                error: Some("Invalid email or password".to_string()),
                requires_otp: None,
                login_token: None,
                message: None,
            }));
        }
    };

    // Check if user is confirmed
    if !user.is_confirmed() {
        warn!("Login failed: User {} not confirmed", user.email);
        info!(
            duration_ms = start.elapsed().as_millis(),
            "Login completed in {}ms (success=false, reason=not_confirmed)",
            start.elapsed().as_millis()
        );
        return Ok(Json(LoginResponse {
            success: false,
            token: None,
            user: None,
            error: Some("Please confirm your email address before signing in".to_string()),
            requires_otp: None,
            login_token: None,
            message: None,
        }));
    }

    // Check if user is active
    if !user.is_active() {
        warn!(
            "Login failed: User {} is not active (status: {})",
            user.email, user.status
        );
        info!(
            duration_ms = start.elapsed().as_millis(),
            "Login completed in {}ms (success=false, reason=account_suspended)",
            start.elapsed().as_millis()
        );
        return Ok(Json(LoginResponse {
            success: false,
            token: None,
            user: None,
            error: Some("Your account has been suspended".to_string()),
            requires_otp: None,
            login_token: None,
            message: None,
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
        info!(
            duration_ms = start.elapsed().as_millis(),
            "Login completed in {}ms (success=false, reason=invalid_password)",
            start.elapsed().as_millis()
        );
        return Ok(Json(LoginResponse {
            success: false,
            token: None,
            user: None,
            error: Some("Invalid email or password".to_string()),
            requires_otp: None,
            login_token: None,
            message: None,
        }));
    }

    // Check if OTP is enabled
    let otp_enabled: Option<bool> = sqlx::query_scalar::<_, Option<bool>>(
        "SELECT enabled FROM user_otp_settings WHERE instance_user_id = $1",
    )
    .bind(user.id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error checking OTP status: {}", e);
        info!(
            duration_ms = start.elapsed().as_millis(),
            "Login completed in {}ms (success=false, error=db_otp_check)",
            start.elapsed().as_millis()
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .flatten();

    if otp_enabled.unwrap_or(false) {
        // OTP is required - generate a temporary login token (expires in 5 minutes)
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let exp = now + 300; // 5 minutes

        // Create a special JWT for OTP verification
        let login_token_payload = serde_json::json!({
            "user_id": user.id.to_string(),
            "email": user.email,
            "password_verified": true,
            "exp": exp,
            "iat": now
        });

        let login_token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &login_token_payload,
            &jsonwebtoken::EncodingKey::from_secret(state.config.jwt_secret.as_ref()),
        )
        .map_err(|e| {
            error!("Failed to generate login token: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!(
            "OTP required for user: {}, returning login token",
            user.email
        );
        info!(
            duration_ms = start.elapsed().as_millis(),
            "Login completed in {}ms (success=otp_required)",
            start.elapsed().as_millis()
        );
        return Ok(Json(LoginResponse {
            success: false,
            token: None,
            user: None,
            error: None,
            requires_otp: Some(true),
            login_token: Some(login_token),
            message: Some("Please enter your OTP code to complete login".to_string()),
        }));
    }

    info!("Login successful for user: {}", user.email);

    // Generate JWT token
    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let token = jwt_service
        .encode(user.id.to_string(), user.email.clone())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(
        duration_ms = start.elapsed().as_millis(),
        "Login completed in {}ms (success=true)",
        start.elapsed().as_millis()
    );
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
        requires_otp: None,
        login_token: None,
        message: None,
    }))
}

async fn verify_otp_login(
    State(state): State<AppState>,
    Json(payload): Json<VerifyOtpLoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    use leadsnebula_core::otp::OtpService;
    use tracing::{error, info, warn};

    if payload.otp_code.is_none() && payload.backup_code.is_none() {
        return Ok(Json(LoginResponse {
            success: false,
            token: None,
            user: None,
            error: Some("OTP code or backup code is required".to_string()),
            requires_otp: None,
            login_token: None,
            message: None,
        }));
    }

    // Decode and verify login token
    let decoded = jsonwebtoken::decode::<JsonValue>(
        &payload.login_token,
        &jsonwebtoken::DecodingKey::from_secret(state.config.jwt_secret.as_ref()),
        &jsonwebtoken::Validation::default(),
    )
    .map_err(|e| {
        warn!("Invalid login token: {}", e);
        StatusCode::UNAUTHORIZED
    })?;

    let claims = decoded.claims;
    let user_id_str = claims
        .get("user_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            error!("Missing user_id in login token");
            StatusCode::UNAUTHORIZED
        })?;

    // Check if token is expired
    if let Some(exp) = claims.get("exp").and_then(|v| v.as_u64()) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if exp < now {
            return Ok(Json(LoginResponse {
                success: false,
                token: None,
                user: None,
                error: Some("Login session expired. Please log in again.".to_string()),
                requires_otp: None,
                login_token: None,
                message: None,
            }));
        }
    }

    let user_id = Uuid::parse_str(user_id_str).map_err(|_| {
        error!("Invalid user_id format in login token");
        StatusCode::UNAUTHORIZED
    })?;

    // Load user from database
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM instance_users WHERE id = $1 AND status = 'active'",
    )
    .bind(user_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error loading user: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or_else(|| {
        warn!("User not found for login token");
        StatusCode::UNAUTHORIZED
    })?;

    // Verify OTP is enabled
    let otp_enabled: Option<bool> = sqlx::query_scalar::<_, Option<bool>>(
        "SELECT enabled FROM user_otp_settings WHERE instance_user_id = $1",
    )
    .bind(user.id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error checking OTP status: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .flatten();

    if !otp_enabled.unwrap_or(false) {
        return Ok(Json(LoginResponse {
            success: false,
            token: None,
            user: None,
            error: Some("OTP is not enabled for this account".to_string()),
            requires_otp: None,
            login_token: None,
            message: None,
        }));
    }

    // Get OTP secret and backup codes
    use sqlx::Row;
    let otp_row = sqlx::query(
        "SELECT secret_encrypted, backup_codes_encrypted FROM user_otp_settings WHERE instance_user_id = $1",
    )
    .bind(user.id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error loading OTP settings: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or_else(|| {
        error!("OTP settings not found");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let otp_secret: Option<String> = otp_row.try_get("secret").ok();
    let backup_codes_json: Option<String> = otp_row.try_get("backup_codes").ok();

    let secret = otp_secret.ok_or_else(|| {
        error!("OTP secret not found");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut verified = false;

    // Verify backup code or OTP code
    if let Some(backup_code) = payload.backup_code {
        // Verify backup code
        if let Some(backup_codes_json) = backup_codes_json {
            let mut backup_codes: Vec<String> =
                serde_json::from_str(&backup_codes_json).unwrap_or_default();

            // Check if code exists (case-insensitive)
            let code_upper = backup_code.to_uppercase();
            if let Some(index) = backup_codes
                .iter()
                .position(|c| c.to_uppercase() == code_upper)
            {
                // Remove used backup code
                backup_codes.remove(index);
                let updated_codes_json = serde_json::to_string(&backup_codes)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

                // Update database
                sqlx::query(
                    "UPDATE user_otp_settings SET backup_codes_encrypted = $1, updated_at = NOW() WHERE instance_user_id = $2",
                )
                .bind(&updated_codes_json)
                .bind(user.id)
                .execute(state.db_pool.as_ref())
                .await
                .map_err(|e| {
                    error!("Database error updating backup codes: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

                verified = true;
                info!("Backup code verified and removed for user: {}", user.email);
            }
        }
    } else if let Some(otp_code) = payload.otp_code {
        // Verify OTP code
        let otp_service = OtpService::new(&secret).map_err(|e| {
            error!("Failed to create OtpService: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        verified = otp_service.verify(&otp_code);
        if verified {
            info!("OTP code verified for user: {}", user.email);
        }
    }

    if !verified {
        warn!("Invalid OTP code or backup code for user: {}", user.email);
        return Ok(Json(LoginResponse {
            success: false,
            token: None,
            user: None,
            error: Some("Invalid OTP code or backup code".to_string()),
            requires_otp: None,
            login_token: None,
            message: None,
        }));
    }

    // OTP verified - complete login
    info!("OTP verified, completing login for user: {}", user.email);

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
        requires_otp: None,
        login_token: None,
        message: None,
    }))
}
