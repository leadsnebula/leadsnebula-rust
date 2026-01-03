use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use leadsnebula_core::{Claims, JwtHelper, JwtSecret, OtpHelper, PasswordHelper};
use leadsnebula_models::{instance_user::InstanceUser, user_otp_setting::UserOtpSetting};
use leadsnebula_services::AuditService;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::auth::AuthState;

/// Login request
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// OTP verification request
#[derive(Debug, Deserialize)]
pub struct VerifyOtpLoginRequest {
    pub login_token: String,
    pub otp_code: Option<String>,
    pub backup_code: Option<String>,
}

/// Change password request
#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub password: String,
    pub password_confirmation: String,
}

/// Register user request
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub password_confirmation: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

/// Login response
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<UserInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_otp: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_expired: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiry_days: Option<i32>,
    pub status_code: u16,
}

/// User info in response
#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub id: Uuid,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
}

/// Generic response
#[derive(Debug, Serialize)]
pub struct GenericResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub status_code: u16,
}

/// Generate a temporary login token for OTP verification
fn generate_login_token(
    user_id: Uuid,
    email: &str,
    jwt_secret: &JwtSecret,
) -> anyhow::Result<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        exp: now + (5 * 60), // 5 minutes
        iat: now,
        password_verified: Some(true),
    };
    JwtHelper::encode(&claims, jwt_secret)
}

/// POST /api/auth/login
pub async fn login(State(state): State<AuthState>, Json(payload): Json<LoginRequest>) -> Response {
    let email = payload.email.trim().to_lowercase();
    let password = payload.password.trim();

    // Validate input
    if email.is_empty() || password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(LoginResponse {
                success: false,
                token: None,
                user: None,
                error: Some("Email and password are required".to_string()),
                requires_otp: None,
                login_token: None,
                password_expired: None,
                expiry_days: None,
                status_code: 400,
            }),
        )
            .into_response();
    }

    info!("Attempting login for email: {}", email);

    // Find user by email (case-insensitive)
    let user: Option<InstanceUser> = match sqlx::query_as(
        r#"
        SELECT 
            id, email, encrypted_password, first_name, last_name,
            status, created_at, updated_at, last_password_change_at,
            preferred_2fa_method, passwordless_login_enabled
        FROM instance_users
        WHERE LOWER(email) = $1
        LIMIT 1
        "#,
    )
    .bind(&email)
    .fetch_optional(state.pool.as_ref())
    .await
    {
        Ok(user) => user,
        Err(e) => {
            warn!(
                "Database error during login query for email {}: {}",
                email, e
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LoginResponse {
                    success: false,
                    token: None,
                    user: None,
                    error: Some("Internal server error".to_string()),
                    requires_otp: None,
                    login_token: None,
                    password_expired: None,
                    expiry_days: None,
                    status_code: 500,
                }),
            )
                .into_response();
        }
    };

    let user = match user {
        Some(u) => u,
        None => {
            warn!("Login failed: User not found for email: {}", email);
            // Log failed login attempt
            let _ = AuditService::log_login_attempt(
                state.pool.as_ref(),
                None,
                Some(&email),
                false,
                Some("user_not_found"),
                None,
                None,
                None,
            )
            .await;

            return (
                StatusCode::UNAUTHORIZED,
                Json(LoginResponse {
                    success: false,
                    token: None,
                    user: None,
                    error: Some("Invalid email or password".to_string()),
                    requires_otp: None,
                    login_token: None,
                    password_expired: None,
                    expiry_days: None,
                    status_code: 401,
                }),
            )
                .into_response();
        }
    };

    info!(
        "User found: {} (ID: {}, status: {})",
        user.email, user.id, user.status
    );

    // Check if user is active
    if user.status != "active" {
        return (
            StatusCode::FORBIDDEN,
            Json(LoginResponse {
                success: false,
                token: None,
                user: None,
                error: Some("Your account has been suspended".to_string()),
                requires_otp: None,
                login_token: None,
                password_expired: None,
                expiry_days: None,
                status_code: 403,
            }),
        )
            .into_response();
    }

    // Verify password
    let password_valid =
        PasswordHelper::verify_password(password, &user.encrypted_password).unwrap_or(false);

    if !password_valid {
        warn!("Login failed: Invalid password for email: {}", email);
        // Log failed login attempt
        let _ = AuditService::log_login_attempt(
            state.pool.as_ref(),
            Some(user.id),
            Some(&user.email),
            false,
            Some("invalid_password"),
            None,
            None,
            None,
        )
        .await;

        return (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                success: false,
                token: None,
                user: None,
                error: Some("Invalid email or password".to_string()),
                requires_otp: None,
                login_token: None,
                password_expired: None,
                expiry_days: None,
                status_code: 401,
            }),
        )
            .into_response();
    }

    // Check if password has expired (optimized: single query with JOIN)
    if let Some(last_change) = user.last_password_change_at {
        // Load password expiry days from policy (per instance) - combined query
        let expiry_days: Option<i32> = sqlx::query_scalar(
            r#"
            SELECT ppc.config_value::int
            FROM instance_user_roles iur
            LEFT JOIN password_policy_config ppc ON (
                ppc.config_key = 'password_expiry_days'
                AND (ppc.instance_id = iur.instance_id OR ppc.instance_id IS NULL)
            )
            WHERE iur.instance_user_id = $1
            ORDER BY ppc.instance_id NULLS LAST
            LIMIT 1
            "#,
        )
        .bind(user.id)
        .fetch_optional(state.pool.as_ref())
        .await
        .ok()
        .flatten();

        if let Some(expiry) = expiry_days {
            if expiry > 0 {
                let days_since_change = (Utc::now() - last_change).num_days();
                if days_since_change >= expiry as i64 {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(LoginResponse {
                            success: false,
                            token: None,
                            user: None,
                            error: Some(format!(
                                "Your password has expired. Please change your password. Passwords expire after {} days.",
                                expiry
                            )),
                            requires_otp: None,
                            login_token: None,
                            password_expired: Some(true),
                            expiry_days: Some(expiry),
                            status_code: 403,
                        }),
                    )
                        .into_response();
                }
            }
        }
    }

    // Check if OTP is required
    let otp_setting: Option<UserOtpSetting> = sqlx::query_as(
        r#"
        SELECT id, instance_user_id, enabled, secret_encrypted,
               backup_codes_encrypted, last_verified_at, created_at, updated_at
        FROM user_otp_settings
        WHERE instance_user_id = $1 AND enabled = true
        LIMIT 1
        "#,
    )
    .bind(user.id)
    .fetch_optional(state.pool.as_ref())
    .await
    .ok()
    .flatten();

    if let Some(_otp_setting) = otp_setting {
        // OTP is required - generate temporary login token
        let login_token = match generate_login_token(user.id, &user.email, &state.jwt_secret) {
            Ok(token) => token,
            Err(e) => {
                warn!("Failed to generate login token: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(LoginResponse {
                        success: false,
                        token: None,
                        user: None,
                        error: Some("Failed to generate login token".to_string()),
                        requires_otp: None,
                        login_token: None,
                        password_expired: None,
                        expiry_days: None,
                        status_code: 500,
                    }),
                )
                    .into_response();
            }
        };

        return (
            StatusCode::OK,
            Json(LoginResponse {
                success: true,
                token: None,
                user: Some(UserInfo {
                    id: user.id,
                    email: user.email,
                    first_name: user.first_name,
                    last_name: user.last_name,
                }),
                error: None,
                requires_otp: Some(true),
                login_token: Some(login_token),
                password_expired: None,
                expiry_days: None,
                status_code: 200,
            }),
        )
            .into_response();
    }

    // No OTP required - complete login
    // Log successful login
    let _ = AuditService::log_login_attempt(
        state.pool.as_ref(),
        Some(user.id),
        Some(&user.email),
        true,
        None,
        None,
        None,
        None,
    )
    .await;

    // Generate JWT token
    let claims = Claims::new(user.id, user.email.clone(), JwtHelper::EXPIRATION_TIME);
    let token = match JwtHelper::encode(&claims, &state.jwt_secret) {
        Ok(t) => t,
        Err(e) => {
            warn!("Failed to generate JWT token: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LoginResponse {
                    success: false,
                    token: None,
                    user: None,
                    error: Some("Failed to generate authentication token".to_string()),
                    requires_otp: None,
                    login_token: None,
                    password_expired: None,
                    expiry_days: None,
                    status_code: 500,
                }),
            )
                .into_response();
        }
    };

    info!("Login successful for user: {}", user.email);

    (
        StatusCode::OK,
        Json(LoginResponse {
            success: true,
            token: Some(token),
            user: Some(UserInfo {
                id: user.id,
                email: user.email,
                first_name: user.first_name,
                last_name: user.last_name,
            }),
            error: None,
            requires_otp: None,
            login_token: None,
            password_expired: None,
            expiry_days: None,
            status_code: 200,
        }),
    )
        .into_response()
}

/// POST /api/auth/verify-otp-login
pub async fn verify_otp_login(
    State(state): State<AuthState>,
    Json(payload): Json<VerifyOtpLoginRequest>,
) -> Response {
    // Decode and verify login token
    let claims = match JwtHelper::decode(&payload.login_token, &state.jwt_secret) {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(LoginResponse {
                    success: false,
                    token: None,
                    user: None,
                    error: Some("Invalid or expired login token".to_string()),
                    requires_otp: None,
                    login_token: None,
                    password_expired: None,
                    expiry_days: None,
                    status_code: 401,
                }),
            )
                .into_response();
        }
    };

    // Verify password_verified flag
    if !claims.password_verified.unwrap_or(false) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                success: false,
                token: None,
                user: None,
                error: Some("Login token is invalid".to_string()),
                requires_otp: None,
                login_token: None,
                password_expired: None,
                expiry_days: None,
                status_code: 401,
            }),
        )
            .into_response();
    }

    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(LoginResponse {
                    success: false,
                    token: None,
                    user: None,
                    error: Some("Invalid user ID in token".to_string()),
                    requires_otp: None,
                    login_token: None,
                    password_expired: None,
                    expiry_days: None,
                    status_code: 400,
                }),
            )
                .into_response();
        }
    };

    // Load user
    let user: Option<InstanceUser> = sqlx::query_as(
        r#"
        SELECT 
            id, email, encrypted_password, first_name, last_name,
            status, created_at, updated_at, last_password_change_at,
            preferred_2fa_method, passwordless_login_enabled
        FROM instance_users
        WHERE id = $1
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(state.pool.as_ref())
    .await
    .ok()
    .flatten();

    let user = match user {
        Some(u) => u,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(LoginResponse {
                    success: false,
                    token: None,
                    user: None,
                    error: Some("User not found".to_string()),
                    requires_otp: None,
                    login_token: None,
                    password_expired: None,
                    expiry_days: None,
                    status_code: 404,
                }),
            )
                .into_response();
        }
    };

    // Load OTP setting
    let otp_setting: Option<UserOtpSetting> = sqlx::query_as(
        r#"
        SELECT id, instance_user_id, enabled, secret_encrypted,
               backup_codes_encrypted, last_verified_at, created_at, updated_at
        FROM user_otp_settings
        WHERE instance_user_id = $1 AND enabled = true
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(state.pool.as_ref())
    .await
    .ok()
    .flatten();

    let otp_setting = match otp_setting {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(LoginResponse {
                    success: false,
                    token: None,
                    user: None,
                    error: Some("OTP is not enabled for this account".to_string()),
                    requires_otp: None,
                    login_token: None,
                    password_expired: None,
                    expiry_days: None,
                    status_code: 400,
                }),
            )
                .into_response();
        }
    };

    // Verify OTP code or backup code
    let verified = if let Some(otp_code) = &payload.otp_code {
        // Decrypt secret - secret_encrypted is stored as JSON (Encrypted struct)
        // For now, assume it's stored as plain base32 string (will be encrypted later)
        // TODO: Load encryption key from SSM and decrypt properly
        let secret = &otp_setting.secret_encrypted; // Temporary: use as-is until encryption is implemented

        // Verify TOTP code
        OtpHelper::verify_code(
            &secret,
            otp_code,
            "LeadsNebula",
            &user.email,
            1, // drift_behind: 1 window (30 seconds)
            1, // drift_ahead: 1 window (30 seconds)
        )
        .unwrap_or(false)
    } else if let Some(backup_code) = &payload.backup_code {
        // Use backup code
        let mut otp_setting_mut = otp_setting.clone();
        otp_setting_mut
            .use_backup_code(backup_code, state.pool.as_ref())
            .await
            .unwrap_or(false)
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(LoginResponse {
                success: false,
                token: None,
                user: None,
                error: Some("OTP code or backup code is required".to_string()),
                requires_otp: None,
                login_token: None,
                password_expired: None,
                expiry_days: None,
                status_code: 400,
            }),
        )
            .into_response();
    };

    if !verified {
        warn!("OTP verification failed for user: {}", user.email);
        let _ = AuditService::log_login_attempt(
            state.pool.as_ref(),
            Some(user.id),
            Some(&user.email),
            false,
            Some("invalid_otp"),
            None,
            None,
            None,
        )
        .await;

        return (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                success: false,
                token: None,
                user: None,
                error: Some("Invalid OTP code or backup code".to_string()),
                requires_otp: None,
                login_token: None,
                password_expired: None,
                expiry_days: None,
                status_code: 401,
            }),
        )
            .into_response();
    }

    // Update last_verified_at
    let _ = sqlx::query(
        r#"
        UPDATE user_otp_settings
        SET last_verified_at = $1, updated_at = $2
        WHERE id = $3
        "#,
    )
    .bind(Utc::now())
    .bind(Utc::now())
    .bind(otp_setting.id)
    .execute(state.pool.as_ref())
    .await;

    // Log successful login
    let _ = AuditService::log_login_attempt(
        state.pool.as_ref(),
        Some(user.id),
        Some(&user.email),
        true,
        None,
        None,
        None,
        None,
    )
    .await;

    // Generate final JWT token
    let claims = Claims::new(user.id, user.email.clone(), JwtHelper::EXPIRATION_TIME);
    let token = match JwtHelper::encode(&claims, &state.jwt_secret) {
        Ok(t) => t,
        Err(e) => {
            warn!("Failed to generate JWT token: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LoginResponse {
                    success: false,
                    token: None,
                    user: None,
                    error: Some("Failed to generate authentication token".to_string()),
                    requires_otp: None,
                    login_token: None,
                    password_expired: None,
                    expiry_days: None,
                    status_code: 500,
                }),
            )
                .into_response();
        }
    };

    info!("OTP login successful for user: {}", user.email);

    (
        StatusCode::OK,
        Json(LoginResponse {
            success: true,
            token: Some(token),
            user: Some(UserInfo {
                id: user.id,
                email: user.email,
                first_name: user.first_name,
                last_name: user.last_name,
            }),
            error: None,
            requires_otp: None,
            login_token: None,
            password_expired: None,
            expiry_days: None,
            status_code: 200,
        }),
    )
        .into_response()
}

/// POST /api/auth/change-password
/// Requires authentication (JWT token)
pub async fn change_password(
    State(state): State<AuthState>,
    axum::extract::Extension(user): axum::extract::Extension<crate::auth::AuthenticatedUser>,
    Json(payload): Json<ChangePasswordRequest>,
) -> Response {
    // Validate input
    if payload.password != payload.password_confirmation {
        return (
            StatusCode::BAD_REQUEST,
            Json(GenericResponse {
                success: false,
                message: None,
                error: Some("Password and password confirmation do not match".to_string()),
                status_code: 400,
            }),
        )
            .into_response();
    }

    // Load current user from database
    let current_user: Option<InstanceUser> = sqlx::query_as(
        r#"
        SELECT 
            id, email, encrypted_password, first_name, last_name,
            status, created_at, updated_at, last_password_change_at,
            preferred_2fa_method, passwordless_login_enabled
        FROM instance_users
        WHERE id = $1
        LIMIT 1
        "#,
    )
    .bind(user.user_id)
    .fetch_optional(state.pool.as_ref())
    .await
    .ok()
    .flatten();

    let current_user = match current_user {
        Some(u) => u,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some("User not found".to_string()),
                    status_code: 404,
                }),
            )
                .into_response();
        }
    };

    // Verify current password
    let current_password_valid = PasswordHelper::verify_password(
        &payload.current_password,
        &current_user.encrypted_password,
    )
    .unwrap_or(false);

    if !current_password_valid {
        return (
            StatusCode::UNAUTHORIZED,
            Json(GenericResponse {
                success: false,
                message: None,
                error: Some("Current password is incorrect".to_string()),
                status_code: 401,
            }),
        )
            .into_response();
    }

    // Validate password against policy
    let instance_id = match user.instance_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some("User instance not found".to_string()),
                    status_code: 400,
                }),
            )
                .into_response();
        }
    };

    let policy =
        match leadsnebula_core::PasswordPolicyHelper::load_policy(state.pool.as_ref(), instance_id)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to load password policy: {}", e);
                // Use default policy if loading fails
                leadsnebula_core::PasswordPolicy {
                    min_length: 8,
                    require_uppercase: true,
                    require_lowercase: true,
                    require_numbers: true,
                    require_special_chars: false,
                    password_reuse_count: 5,
                }
            }
        };

    let validation_errors =
        match leadsnebula_core::PasswordPolicyHelper::validate_password(&payload.password, &policy)
        {
            Ok(errors) => errors,
            Err(e) => {
                warn!("Password validation error: {}", e);
                vec!["Password validation failed".to_string()]
            }
        };

    if !validation_errors.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(GenericResponse {
                success: false,
                message: None,
                error: Some(format!(
                    "Password validation failed: {}",
                    validation_errors.join(", ")
                )),
                status_code: 400,
            }),
        )
            .into_response();
    }

    // Hash new password
    let new_password_hash = match PasswordHelper::hash_password(&payload.password) {
        Ok(hash) => hash,
        Err(e) => {
            warn!("Failed to hash password: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some("Failed to process password".to_string()),
                    status_code: 500,
                }),
            )
                .into_response();
        }
    };

    // Update password
    let result = sqlx::query(
        r#"
        UPDATE instance_users
        SET encrypted_password = $1, last_password_change_at = $2, updated_at = $3
        WHERE id = $4
        "#,
    )
    .bind(&new_password_hash)
    .bind(Utc::now())
    .bind(Utc::now())
    .bind(user.user_id)
    .execute(state.pool.as_ref())
    .await;

    match result {
        Ok(_) => {
            // Log password change
            let _ = AuditService::log_password_change(
                state.pool.as_ref(),
                user.user_id,
                user.instance_id,
                None,
                None,
            )
            .await;

            info!("Password changed successfully for user: {}", user.email);

            (
                StatusCode::OK,
                Json(GenericResponse {
                    success: true,
                    message: Some("Password changed successfully".to_string()),
                    error: None,
                    status_code: 200,
                }),
            )
                .into_response()
        }
        Err(e) => {
            warn!("Failed to update password: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some("Failed to update password".to_string()),
                    status_code: 500,
                }),
            )
                .into_response()
        }
    }
}

/// POST /api/auth/register
pub async fn register(
    State(state): State<AuthState>,
    Json(payload): Json<RegisterRequest>,
) -> Response {
    let email = payload.email.trim().to_lowercase();

    // Validate input
    if email.is_empty() || payload.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(GenericResponse {
                success: false,
                message: None,
                error: Some("Email and password are required".to_string()),
                status_code: 400,
            }),
        )
            .into_response();
    }

    if payload.password != payload.password_confirmation {
        return (
            StatusCode::BAD_REQUEST,
            Json(GenericResponse {
                success: false,
                message: None,
                error: Some("Password and password confirmation do not match".to_string()),
                status_code: 400,
            }),
        )
            .into_response();
    }

    // Check if user already exists
    let existing_user: Option<InstanceUser> = sqlx::query_as(
        r#"
        SELECT 
            id, email, encrypted_password, first_name, last_name,
            status, created_at, updated_at, last_password_change_at,
            preferred_2fa_method, passwordless_login_enabled
        FROM instance_users
        WHERE LOWER(email) = $1
        LIMIT 1
        "#,
    )
    .bind(&email)
    .fetch_optional(state.pool.as_ref())
    .await
    .ok()
    .flatten();

    if existing_user.is_some() {
        return (
            StatusCode::CONFLICT,
            Json(GenericResponse {
                success: false,
                message: None,
                error: Some("User with this email already exists".to_string()),
                status_code: 409,
            }),
        )
            .into_response();
    }

    // TODO: Validate password against policy
    // TODO: Create instance and instance_user_role

    // Hash password
    let password_hash = match PasswordHelper::hash_password(&payload.password) {
        Ok(hash) => hash,
        Err(e) => {
            warn!("Failed to hash password: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some("Failed to process password".to_string()),
                    status_code: 500,
                }),
            )
                .into_response();
        }
    };

    // Create user
    let user_id = Uuid::new_v4();
    let now = Utc::now();

    let result = sqlx::query(
        r#"
        INSERT INTO instance_users (
            id, email, encrypted_password, first_name, last_name,
            status, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(user_id)
    .bind(&email)
    .bind(&password_hash)
    .bind(&payload.first_name)
    .bind(&payload.last_name)
    .bind("pending_verification") // New users need email confirmation
    .bind(now)
    .bind(now)
    .execute(state.pool.as_ref())
    .await;

    match result {
        Ok(_) => {
            info!("User registered: {}", email);

            (
                StatusCode::CREATED,
                Json(GenericResponse {
                    success: true,
                    message: Some("User registered successfully. Please check your email to verify your account.".to_string()),
                    error: None,
                    status_code: 201,
                }),
            )
                .into_response()
        }
        Err(e) => {
            warn!("Failed to create user: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some("Failed to create user".to_string()),
                    status_code: 500,
                }),
            )
                .into_response()
        }
    }
}
