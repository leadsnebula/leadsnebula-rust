use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use leadsnebula_core::{EmailService, OtpHelper, PasswordResetHelper};
use leadsnebula_models::{
    instance_user::InstanceUser, user_otp_setting::UserOtpSetting,
    webauthn_credential::WebauthnCredential,
};
use leadsnebula_services::AuditService;
use qrcode::render::svg;
use qrcode::QrCode;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;
use webauthn_rs::prelude::*;

use crate::auth::{AuthState, AuthenticatedUser};

/// POST /api/auth/forgot-password
/// Public endpoint to request password reset email (no authentication required)
#[derive(Debug, Deserialize)]
pub struct ForgotPasswordRequest {
    pub email: String,
}

pub async fn forgot_password(
    State(state): State<AuthState>,
    Json(payload): Json<ForgotPasswordRequest>,
) -> Response {
    let email = payload.email.trim().to_lowercase();

    // Validate input
    if email.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(GenericResponse {
                success: false,
                message: None,
                error: Some("Email is required".to_string()),
                status_code: 400,
            }),
        )
            .into_response();
    }

    // Find user by email (case-insensitive)
    let user: Option<InstanceUser> = sqlx::query_as(
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

    // Always return success to prevent email enumeration
    // Only send email if user exists and is active
    if let Some(user) = user {
        if user.status == "active" {
            // Generate reset password token
            let (raw_token, hashed_token) = PasswordResetHelper::generate_token();
            let now = Utc::now();

            // Store the hashed token and timestamp in the database
            let update_result = sqlx::query(
                r#"
                UPDATE instance_users
                SET reset_password_token = $1, reset_password_sent_at = $2, updated_at = $3
                WHERE id = $4
                "#,
            )
            .bind(&hashed_token)
            .bind(now)
            .bind(now)
            .bind(user.id)
            .execute(state.pool.as_ref())
            .await;

            if update_result.is_ok() {
                // Send password reset email
                if let Ok(email_service) = EmailService::new().await {
                    let user_name = format!(
                        "{} {}",
                        user.first_name.as_deref().unwrap_or(""),
                        user.last_name.as_deref().unwrap_or("")
                    )
                    .trim()
                    .to_string();

                    let _ = email_service
                        .send_password_reset_email(
                            &user.email,
                            if user_name.is_empty() {
                                None
                            } else {
                                Some(&user_name)
                            },
                            &raw_token,
                        )
                        .await;

                    info!("Password reset email sent to: {}", user.email);
                } else {
                    warn!("Failed to initialize email service for password reset");
                }
            } else {
                warn!(
                    "Failed to store password reset token for user: {}",
                    user.email
                );
            }
        }
    } else {
        warn!("Password reset requested for non-existent email: {}", email);
    }

    // Always return success message to prevent email enumeration
    (
        StatusCode::OK,
        Json(GenericResponse {
            success: true,
            message: Some(
                "If an account with that email exists, a password reset link has been sent."
                    .to_string(),
            ),
            error: None,
            status_code: 200,
        }),
    )
        .into_response()
}

/// GET /api/security
/// Get security settings and status
pub async fn get_security_info(
    State(state): State<AuthState>,
    axum::extract::Extension(user): axum::extract::Extension<AuthenticatedUser>,
) -> Response {
    // Load OTP setting
    let otp_setting: Option<UserOtpSetting> = sqlx::query_as(
        r#"
        SELECT id, instance_user_id, enabled, secret_encrypted,
               backup_codes_encrypted, last_verified_at, created_at, updated_at
        FROM user_otp_settings
        WHERE instance_user_id = $1
        LIMIT 1
        "#,
    )
    .bind(user.user_id)
    .fetch_optional(state.pool.as_ref())
    .await
    .ok()
    .flatten();

    let otp_enabled = otp_setting.as_ref().map(|s| s.enabled).unwrap_or(false);

    // Load passkeys
    let passkeys: Vec<WebauthnCredential> = sqlx::query_as(
        r#"
        SELECT id, instance_user_id, external_id, public_key,
               sign_count, passkey_type, name, created_at, updated_at, last_used_at
        FROM webauthn_credentials
        WHERE instance_user_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(user.user_id)
    .fetch_all(state.pool.as_ref())
    .await
    .ok()
    .unwrap_or_default();

    let passkeys_response: Vec<PasskeyInfo> = passkeys
        .into_iter()
        .map(|pk| PasskeyInfo {
            id: pk.id,
            name: pk.name.unwrap_or_else(|| "Unnamed".to_string()),
            r#type: pk.passkey_type.unwrap_or_else(|| "unknown".to_string()),
            created_at: pk.created_at.to_rfc3339(),
            last_used_at: pk.last_used_at.map(|d| d.to_rfc3339()),
        })
        .collect();

    (
        StatusCode::OK,
        Json(SecurityInfoResponse {
            success: true,
            otp_enabled,
            passkeys: passkeys_response,
        }),
    )
        .into_response()
}

/// POST /api/security/password-reset-email
/// Request password reset email (when logged in)
pub async fn request_password_reset_email(
    State(state): State<AuthState>,
    axum::extract::Extension(user): axum::extract::Extension<AuthenticatedUser>,
) -> Response {
    // Load user from database
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

    // Generate reset password token
    let (raw_token, hashed_token) = PasswordResetHelper::generate_token();
    let now = Utc::now();

    // Store the hashed token and timestamp in the database
    let update_result = sqlx::query(
        r#"
        UPDATE instance_users
        SET reset_password_token = $1, reset_password_sent_at = $2, updated_at = $3
        WHERE id = $4
        "#,
    )
    .bind(&hashed_token)
    .bind(now)
    .bind(now)
    .bind(user.user_id)
    .execute(state.pool.as_ref())
    .await;

    if update_result.is_err() {
        warn!(
            "Failed to store password reset token for user: {}",
            current_user.email
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(GenericResponse {
                success: false,
                message: None,
                error: Some("Failed to generate password reset token".to_string()),
                status_code: 500,
            }),
        )
            .into_response();
    }

    // Send password reset email
    let email_service = match EmailService::new().await {
        Ok(service) => service,
        Err(e) => {
            warn!("Failed to initialize email service: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some("Failed to initialize email service".to_string()),
                    status_code: 500,
                }),
            )
                .into_response();
        }
    };

    let user_name = format!(
        "{} {}",
        current_user.first_name.as_deref().unwrap_or(""),
        current_user.last_name.as_deref().unwrap_or("")
    )
    .trim()
    .to_string();

    let send_result = email_service
        .send_password_reset_email(
            &current_user.email,
            if user_name.is_empty() {
                None
            } else {
                Some(&user_name)
            },
            &raw_token,
        )
        .await;

    if send_result.is_err() {
        warn!(
            "Failed to send password reset email to: {}",
            current_user.email
        );
        // Still return success to prevent email enumeration
        // The token is stored, so the user can request another email if needed
    }

    info!(
        "Password reset email requested for user: {}",
        current_user.email
    );

    // Log the password change request
    let _ = AuditService::log_password_change_request(
        state.pool.as_ref(),
        user.user_id,
        user.instance_id,
    )
    .await;

    (
        StatusCode::OK,
        Json(GenericResponse {
            success: true,
            message: Some("Password change instructions have been sent to your email.".to_string()),
            error: None,
            status_code: 200,
        }),
    )
        .into_response()
}

/// POST /api/security/otp/setup
/// Generate OTP secret and QR code
pub async fn setup_otp(
    State(state): State<AuthState>,
    axum::extract::Extension(user): axum::extract::Extension<AuthenticatedUser>,
) -> Response {
    // Check if OTP is already enabled
    let existing_otp: Option<UserOtpSetting> = sqlx::query_as(
        r#"
        SELECT id, instance_user_id, enabled, secret_encrypted,
               backup_codes_encrypted, last_verified_at, created_at, updated_at
        FROM user_otp_settings
        WHERE instance_user_id = $1 AND enabled = true
        LIMIT 1
        "#,
    )
    .bind(user.user_id)
    .fetch_optional(state.pool.as_ref())
    .await
    .ok()
    .flatten();

    if existing_otp.is_some() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(OtpSetupResponse {
                success: false,
                secret: None,
                qr_code: None,
                provisioning_uri: None,
                error: Some(
                    "OTP is already enabled. Please disable it first to set up a new one."
                        .to_string(),
                ),
            }),
        )
            .into_response();
    }

    // Load user email
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
                Json(OtpSetupResponse {
                    success: false,
                    secret: None,
                    qr_code: None,
                    provisioning_uri: None,
                    error: Some("User not found".to_string()),
                }),
            )
                .into_response();
        }
    };

    // Generate new secret
    let secret = OtpHelper::generate_secret();

    // Generate provisioning URI
    let provisioning_uri =
        match OtpHelper::generate_provisioning_uri(&secret, "LeadsNebula", &current_user.email) {
            Ok(uri) => uri,
            Err(e) => {
                warn!("Failed to generate provisioning URI: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(OtpSetupResponse {
                        success: false,
                        secret: None,
                        qr_code: None,
                        provisioning_uri: None,
                        error: Some("Failed to generate OTP setup".to_string()),
                    }),
                )
                    .into_response();
            }
        };

    // Generate QR code as SVG
    let qr = QrCode::new(&provisioning_uri).unwrap();
    let qr_svg = qr.render::<svg::Color>().min_dimensions(200, 200).build();

    // Create or update OTP setting (but don't enable it yet - wait for verification)
    let _otp_setting_id = if let Some(existing) = sqlx::query_as::<_, UserOtpSetting>(
        r#"
        SELECT id, instance_user_id, enabled, secret_encrypted,
               backup_codes_encrypted, last_verified_at, created_at, updated_at
        FROM user_otp_settings
        WHERE instance_user_id = $1
        LIMIT 1
        "#,
    )
    .bind(user.user_id)
    .fetch_optional(state.pool.as_ref())
    .await
    .ok()
    .flatten()
    {
        // Update existing
        sqlx::query(
            r#"
            UPDATE user_otp_settings
            SET secret_encrypted = $1, enabled = false, updated_at = $2
            WHERE instance_user_id = $3
            "#,
        )
        .bind(&secret)
        .bind(Utc::now())
        .bind(user.user_id)
        .execute(state.pool.as_ref())
        .await
        .ok();
        existing.id
    } else {
        // Create new
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO user_otp_settings (id, instance_user_id, secret_encrypted, enabled, created_at, updated_at)
            VALUES ($1, $2, $3, false, $4, $5)
            "#,
        )
        .bind(id)
        .bind(user.user_id)
        .bind(&secret)
        .bind(Utc::now())
        .bind(Utc::now())
        .execute(state.pool.as_ref())
        .await
        .ok();
        id
    };

    info!("OTP setup initiated for user: {}", current_user.email);

    (
        StatusCode::OK,
        Json(OtpSetupResponse {
            success: true,
            secret: Some(secret),
            qr_code: Some(qr_svg),
            provisioning_uri: Some(provisioning_uri),
            error: None,
        }),
    )
        .into_response()
}

/// POST /api/security/otp/verify
/// Verify OTP code and enable OTP
#[derive(Debug, Deserialize)]
pub struct VerifyOtpRequest {
    pub code: String,
    pub secret: String,
}

pub async fn verify_otp(
    State(state): State<AuthState>,
    axum::extract::Extension(user): axum::extract::Extension<AuthenticatedUser>,
    Json(payload): Json<VerifyOtpRequest>,
) -> Response {
    // Load user email
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
                Json(OtpVerifyResponse {
                    success: false,
                    backup_codes: None,
                    error: Some("User not found".to_string()),
                }),
            )
                .into_response();
        }
    };

    // Get the OTP setting
    let otp_setting: Option<UserOtpSetting> = sqlx::query_as(
        r#"
        SELECT id, instance_user_id, enabled, secret_encrypted,
               backup_codes_encrypted, last_verified_at, created_at, updated_at
        FROM user_otp_settings
        WHERE instance_user_id = $1 AND secret_encrypted = $2
        LIMIT 1
        "#,
    )
    .bind(user.user_id)
    .bind(&payload.secret)
    .fetch_optional(state.pool.as_ref())
    .await
    .ok()
    .flatten();

    if otp_setting.is_none() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(OtpVerifyResponse {
                success: false,
                backup_codes: None,
                error: Some("Invalid OTP setup. Please start over.".to_string()),
            }),
        )
            .into_response();
    }

    // Verify the code
    let verified = OtpHelper::verify_code(
        &payload.secret,
        &payload.code,
        "LeadsNebula",
        &current_user.email,
        1, // drift_behind: 1 window (30 seconds)
        1, // drift_ahead: 1 window (30 seconds)
    )
    .unwrap_or(false);

    if !verified {
        return (
            StatusCode::UNAUTHORIZED,
            Json(OtpVerifyResponse {
                success: false,
                backup_codes: None,
                error: Some("Invalid OTP code. Please try again.".to_string()),
            }),
        )
            .into_response();
    }

    // Enable OTP and generate backup codes
    let backup_codes = OtpHelper::generate_backup_codes();
    let backup_codes_json = serde_json::to_string(&backup_codes).unwrap_or_default();

    sqlx::query(
        r#"
        UPDATE user_otp_settings
        SET enabled = true, backup_codes_encrypted = $1, last_verified_at = $2, updated_at = $3
        WHERE instance_user_id = $4 AND secret_encrypted = $5
        "#,
    )
    .bind(&backup_codes_json)
    .bind(Utc::now())
    .bind(Utc::now())
    .bind(user.user_id)
    .bind(&payload.secret)
    .execute(state.pool.as_ref())
    .await
    .ok();

    info!("OTP enabled for user: {}", current_user.email);

    // Log OTP enable event
    let _ =
        AuditService::log_otp_enabled(state.pool.as_ref(), user.user_id, user.instance_id).await;

    (
        StatusCode::OK,
        Json(OtpVerifyResponse {
            success: true,
            backup_codes: Some(backup_codes),
            error: None,
        }),
    )
        .into_response()
}

/// POST /api/security/otp/disable
/// Disable OTP
pub async fn disable_otp(
    State(state): State<AuthState>,
    axum::extract::Extension(user): axum::extract::Extension<AuthenticatedUser>,
) -> Response {
    sqlx::query(
        r#"
        UPDATE user_otp_settings
        SET enabled = false, updated_at = $1
        WHERE instance_user_id = $2
        "#,
    )
    .bind(Utc::now())
    .bind(user.user_id)
    .execute(state.pool.as_ref())
    .await
    .ok();

    info!("OTP disabled for user: {}", user.user_id);

    // Log OTP disable event
    let _ =
        AuditService::log_otp_disabled(state.pool.as_ref(), user.user_id, user.instance_id).await;

    (
        StatusCode::OK,
        Json(GenericResponse {
            success: true,
            message: Some("OTP disabled successfully.".to_string()),
            error: None,
            status_code: 200,
        }),
    )
        .into_response()
}

/// POST /api/security/passkeys/registration_options
/// Generate WebAuthn registration options
#[derive(Debug, Deserialize)]
pub struct PasskeyRegistrationOptionsRequest {
    #[allow(dead_code)]
    pub name: String,
}

pub async fn passkey_registration_options(
    State(state): State<AuthState>,
    axum::extract::Extension(user): axum::extract::Extension<AuthenticatedUser>,
    Json(_payload): Json<PasskeyRegistrationOptionsRequest>,
) -> Response {
    // Check passkey limit (max 3)
    let passkey_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM webauthn_credentials
        WHERE instance_user_id = $1
        "#,
    )
    .bind(user.user_id)
    .fetch_one(state.pool.as_ref())
    .await
    .unwrap_or(0);

    if passkey_count >= 3 {
        return (
            StatusCode::BAD_REQUEST,
            Json(GenericResponse {
                success: false,
                message: None,
                error: Some("Maximum of 3 passkeys allowed. Please remove a passkey before adding a new one.".to_string()),
                status_code: 400,
            }),
        )
            .into_response();
    }

    // Load user info
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

    // Generate registration options
    let user_display_name = format!(
        "{} {}",
        current_user.first_name.as_deref().unwrap_or(""),
        current_user.last_name.as_deref().unwrap_or("")
    )
    .trim()
    .to_string();

    let (ccr, reg_state) = match state.webauthn.start_passkey_registration(
        &current_user.email,
        user.user_id,
        Some(if user_display_name.is_empty() {
            &current_user.email
        } else {
            &user_display_name
        }),
    ) {
        Ok(result) => result,
        Err(e) => {
            warn!("Failed to start passkey registration: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some("Failed to generate registration options".to_string()),
                    status_code: 500,
                }),
            )
                .into_response();
        }
    };

    // Generate challenge token (before await to avoid Send issues)
    let challenge_token: String = {
        let mut rng = rand::thread_rng();
        (0..16).map(|_| format!("{:x}", rng.gen::<u8>())).collect()
    };

    // Store registration state for later verification
    state
        .challenge_store
        .store_registration_state(challenge_token.clone(), reg_state, user.user_id)
        .await;

    // Convert CreationChallengeResponse to JSON for the frontend
    let options_json = serde_json::to_value(&ccr).unwrap_or_default();

    info!(
        "Passkey registration options generated for user: {}",
        current_user.email
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "options": options_json,
            "challenge_token": challenge_token
        })),
    )
        .into_response()
}

/// POST /api/security/passkeys/register
/// Complete passkey registration
#[derive(Debug, Deserialize)]
pub struct RegisterPasskeyRequest {
    pub credential: RegisterCredential,
    pub challenge_token: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterCredential {
    pub id: String,
    #[allow(non_snake_case)]
    pub rawId: String,
    #[serde(rename = "type")]
    pub cred_type: String,
    pub response: RegisterResponse,
    #[serde(rename = "authenticatorAttachment")]
    pub authenticator_attachment: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterResponse {
    #[serde(rename = "attestationObject")]
    pub attestation_object: String,
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
}

pub async fn register_passkey(
    State(state): State<AuthState>,
    axum::extract::Extension(user): axum::extract::Extension<AuthenticatedUser>,
    Json(payload): Json<RegisterPasskeyRequest>,
) -> Response {
    // Verify challenge token and get registration state
    let reg_state = match state
        .challenge_store
        .get_registration_state(&payload.challenge_token, user.user_id)
        .await
    {
        Some(state) => state,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some(
                        "Invalid or expired registration challenge. Please try again.".to_string(),
                    ),
                    status_code: 400,
                }),
            )
                .into_response();
        }
    };

    // Check passkey limit again
    let passkey_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM webauthn_credentials
        WHERE instance_user_id = $1
        "#,
    )
    .bind(user.user_id)
    .fetch_one(state.pool.as_ref())
    .await
    .unwrap_or(0);

    if passkey_count >= 3 {
        return (
            StatusCode::BAD_REQUEST,
            Json(GenericResponse {
                success: false,
                message: None,
                error: Some("Maximum of 3 passkeys allowed.".to_string()),
                status_code: 400,
            }),
        )
            .into_response();
    }

    // Convert credential to the format expected by webauthn-rs
    // The credential from the frontend needs to be converted to RegisterPublicKeyCredential
    // We'll deserialize it directly from the JSON payload
    let credential_json = serde_json::json!({
        "id": payload.credential.id,
        "rawId": payload.credential.rawId,
        "type": payload.credential.cred_type,
        "response": {
            "attestationObject": payload.credential.response.attestation_object,
            "clientDataJSON": payload.credential.response.client_data_json
        },
        "authenticatorAttachment": payload.credential.authenticator_attachment
    });

    // Deserialize to RegisterPublicKeyCredential (from webauthn_rs::prelude)
    let credential: RegisterPublicKeyCredential = match serde_json::from_value(credential_json) {
        Ok(cred) => cred,
        Err(e) => {
            warn!("Failed to parse credential: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some("Invalid credential format".to_string()),
                    status_code: 400,
                }),
            )
                .into_response();
        }
    };

    // Finish passkey registration
    let passkey = match state
        .webauthn
        .get_webauthn()
        .finish_passkey_registration(&credential, &reg_state)
    {
        Ok(pk) => pk,
        Err(e) => {
            warn!("Failed to verify passkey registration: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some(format!("Passkey verification failed: {}", e)),
                    status_code: 400,
                }),
            )
                .into_response();
        }
    };

    // Determine passkey type
    let passkey_type = payload
        .credential
        .authenticator_attachment
        .as_deref()
        .map(|att| match att {
            "platform" => "soft",
            "cross-platform" => "physical",
            _ => "unknown",
        })
        .unwrap_or_else(|| "soft");

    // Save passkey to database
    // Get credential ID (it's already base64url encoded in webauthn-rs)
    let credential_id_base64 = passkey.cred_id().to_string();
    // Serialize the entire Passkey to JSON for storage
    // The Passkey struct contains all the credential information including the public key
    let public_key_json = serde_json::to_string(&passkey).unwrap_or_else(|_| "{}".to_string());

    let passkey_id = Uuid::new_v4();
    let result = sqlx::query(
        r#"
        INSERT INTO webauthn_credentials (
            id, instance_user_id, external_id, public_key, sign_count,
            name, passkey_type, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(passkey_id)
    .bind(user.user_id)
    .bind(&credential_id_base64)
    .bind(&public_key_json)
    .bind(0i32) // Counter starts at 0 for new passkeys
    .bind(&payload.name)
    .bind(passkey_type)
    .bind(Utc::now())
    .bind(Utc::now())
    .execute(state.pool.as_ref())
    .await;

    match result {
        Ok(_) => {
            info!("Passkey registered successfully for user: {}", user.user_id);

            // Log passkey registration
            let _ = AuditService::log_event(
                state.pool.as_ref(),
                user.instance_id,
                Some(user.user_id),
                "passkey_enabled",
                Some("WebauthnCredential"),
                Some(passkey_id),
                serde_json::json!({
                    "action": "create",
                    "target_type": "WebauthnCredential",
                    "target_id": passkey_id,
                    "passkey_type": passkey_type,
                    "outcome": "success",
                    "timestamp": Utc::now().to_rfc3339()
                }),
                None,
                None,
                None,
            )
            .await;

            (
                StatusCode::OK,
                Json(GenericResponse {
                    success: true,
                    message: Some("Passkey registered successfully.".to_string()),
                    error: None,
                    status_code: 200,
                }),
            )
                .into_response()
        }
        Err(e) => {
            warn!("Failed to save passkey to database: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GenericResponse {
                    success: false,
                    message: None,
                    error: Some("Failed to save passkey".to_string()),
                    status_code: 500,
                }),
            )
                .into_response()
        }
    }
}

/// DELETE /api/security/passkeys/:id
/// Delete a passkey
pub async fn delete_passkey(
    State(state): State<AuthState>,
    axum::extract::Extension(user): axum::extract::Extension<AuthenticatedUser>,
    Path(passkey_id): Path<Uuid>,
) -> Response {
    // Verify passkey belongs to user
    let deleted = sqlx::query(
        r#"
        DELETE FROM webauthn_credentials
        WHERE id = $1 AND instance_user_id = $2
        "#,
    )
    .bind(passkey_id)
    .bind(user.user_id)
    .execute(state.pool.as_ref())
    .await
    .ok()
    .map(|r| r.rows_affected() > 0)
    .unwrap_or(false);

    if !deleted {
        return (
            StatusCode::NOT_FOUND,
            Json(GenericResponse {
                success: false,
                message: None,
                error: Some("Passkey not found".to_string()),
                status_code: 404,
            }),
        )
            .into_response();
    }

    info!("Passkey deleted: {} for user: {}", passkey_id, user.user_id);

    (
        StatusCode::OK,
        Json(GenericResponse {
            success: true,
            message: Some("Passkey deleted successfully.".to_string()),
            error: None,
            status_code: 200,
        }),
    )
        .into_response()
}

// Response types
#[derive(Debug, Serialize)]
pub struct SecurityInfoResponse {
    pub success: bool,
    pub otp_enabled: bool,
    pub passkeys: Vec<PasskeyInfo>,
}

#[derive(Debug, Serialize)]
pub struct PasskeyInfo {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GenericResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub status_code: u16,
}

#[derive(Debug, Serialize)]
pub struct OtpSetupResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qr_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OtpVerifyResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_codes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
