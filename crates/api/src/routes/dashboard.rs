// Note: This module is currently unused because we only serve minimal app with /live endpoint
// It will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

use axum::{
    extract::{Extension, Path, Request, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post, put},
    Router,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::str::FromStr;
use uuid::Uuid;

use crate::AppState;

/// Resolve WebAuthn RP ID and origin. In non-development, uses the request's Origin header
/// when present and valid (HTTPS, host matches our domain) so dev.dashboard.leadsnebula.com works.
///
/// When `use_host_as_rp_id` is true, uses the origin's host as rp_id (host-bound) for passkey
/// managers that reject parent-domain rpId (e.g. Proton Pass). When false, uses parent-domain rpId
/// so passkeys work across subdomains.
fn webauthn_rp_id_and_origin(
    environment: &str,
    headers: &axum::http::HeaderMap,
    use_host_as_rp_id: bool,
) -> Result<(String, String), StatusCode> {
    let origin_header_str = headers
        .get("origin")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let rp_id_production =
        std::env::var("WEBAUTHN_RP_ID").unwrap_or_else(|_| "leadsnebula.com".to_string());
    let rp_id: String = if environment == "development" {
        "localhost".to_string()
    } else {
        rp_id_production.clone()
    };

    let (origin, rp_id_used) = if environment == "development" {
        // Prefer request Origin in development so 127.0.0.1 works (secure-context carveout);
        // fall back to WEBAUTHN_LOCAL_HTTPS or http://localhost:3000.
        let fallback_o = std::env::var("WEBAUTHN_LOCAL_HTTPS")
            .unwrap_or_else(|_| "http://localhost:3000".to_string());
        if !origin_header_str.is_empty() {
            if let Ok(url) = url::Url::parse(origin_header_str) {
                let host = url.host_str().unwrap_or("");
                // Deployed dev (e.g. dev.dashboard.leadsnebula.com calling dev.leadsnebula.com) sends our domain as Origin.
                // Use production-style rp_id so passkeys work; otherwise we'd return (localhost, fallback) and cause OriginRpMissmatch.
                let host_matches_domain =
                    host == rp_id_production || host.ends_with(&format!(".{}", rp_id_production));
                if url.scheme() == "https" && host_matches_domain {
                    let effective_rp_id = if use_host_as_rp_id {
                        host.to_string()
                    } else {
                        rp_id_production.clone()
                    };
                    tracing::info!(
                        "Passkey (dev env, domain origin): origin={} rp_id={} (use_host_as_rp_id={})",
                        origin_header_str,
                        effective_rp_id,
                        use_host_as_rp_id
                    );
                    (origin_header_str.to_string(), effective_rp_id)
                } else if url.scheme() == "https" && host == "localhost.localdomain" {
                    // Use .localdomain so passkey managers (e.g. Proton Pass) that reject RP ID "localhost" can be tested.
                    (
                        origin_header_str.to_string(),
                        "localhost.localdomain".to_string(),
                    )
                } else if url.scheme() == "https" && (host == "localhost" || host == "127.0.0.1") {
                    let rp = if host == "127.0.0.1" {
                        "127.0.0.1".to_string()
                    } else {
                        rp_id.clone()
                    };
                    (origin_header_str.to_string(), rp)
                } else {
                    (fallback_o, rp_id)
                }
            } else {
                (fallback_o, rp_id)
            }
        } else {
            (fallback_o, rp_id)
        }
    } else {
        // Use request Origin when present and valid (HTTPS, host is our domain or subdomain).
        let fallback_origin = format!("https://{}", rp_id);
        let origin_header = origin_header_str;
        if origin_header.is_empty() {
            tracing::info!(
                "Passkey: no Origin header received (possible proxy stripping). Using fallback origin={}",
                fallback_origin
            );
            (fallback_origin, rp_id.clone())
        } else if let Ok(url) = url::Url::parse(origin_header) {
            let scheme_ok = url.scheme() == "https";
            let host = url.host_str().unwrap_or("");
            let host_matches = host == rp_id || host.ends_with(&format!(".{}", rp_id));
            if scheme_ok && host_matches {
                // Parent-domain: rp_id stays as leadsnebula.com so passkeys work across subdomains.
                // Host-bound: use exact host for passkey managers that reject parent-domain (e.g. Proton Pass).
                let effective_rp_id = if use_host_as_rp_id {
                    host.to_string()
                } else {
                    rp_id.clone()
                };
                tracing::info!(
                    "Passkey: origin={} rp_id={} (use_host_as_rp_id={})",
                    origin_header,
                    effective_rp_id,
                    use_host_as_rp_id
                );
                (origin_header.to_string(), effective_rp_id)
            } else {
                tracing::warn!(
                    "Passkey: origin {} invalid (scheme_ok={} host_matches={}). Using fallback.",
                    origin_header,
                    scheme_ok,
                    host_matches
                );
                (fallback_origin, rp_id)
            }
        } else {
            (fallback_origin, rp_id)
        }
    };

    Ok((rp_id_used, origin))
}

/// Mask email for use in application logs (AUTO-MEMORY). Example: "u***@example.com"
fn mask_email_for_log(email: &str) -> String {
    let email = email.trim();
    if email.is_empty() {
        return "***".to_string();
    }
    if let Some(at) = email.find('@') {
        let local = &email[..at];
        let domain = &email[at..];
        let visible = local.chars().take(1).collect::<String>();
        format!("{}***{}", visible, domain)
    } else {
        "***".to_string()
    }
}

// Helper function to check if user is admin
// For now, checks if user owns the instance (instance_user_id matches)
// TODO: Implement proper role-based access control when instance_user_roles table exists
async fn is_user_admin(
    db_pool: &sqlx::PgPool,
    user_id: Uuid,
    instance_id: Option<Uuid>,
) -> Result<bool, sqlx::Error> {
    if let Some(inst_id) = instance_id {
        // Check if user owns the instance (instance_user_id matches)
        // This is a temporary solution until proper role-based access control is implemented
        let result = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM instances WHERE id = $1 AND instance_user_id = $2 AND deleted_at IS NULL)",
        )
        .bind(inst_id)
        .bind(user_id)
        .fetch_one(db_pool)
        .await?;
        Ok(result)
    } else {
        // Check if user owns any instance
        let result = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL)",
        )
        .bind(user_id)
        .fetch_one(db_pool)
        .await?;
        Ok(result)
    }
}

// Helper to get user from request extensions
fn get_user_from_request(request: &Request) -> Option<leadsnebula_core::models::user::User> {
    request
        .extensions()
        .get::<leadsnebula_core::models::user::User>()
        .cloned()
}

/// Returns the instance ID for the current user (instance they own). Used to scope all
/// dashboard list/create to that instance so records are never shared across instances.
async fn get_instance_id_for_user(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
) -> Result<Option<uuid::Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

// Publishers API
#[derive(Serialize)]
pub struct PublisherResponse {
    pub id: String,
    pub name: String,
    pub email: String,
    pub status: String,
    pub api_key_prefix: String,
    pub hmac_required: bool,
    pub created_at: String,
    pub deleted_at: Option<String>,
    pub verticals: Vec<VerticalInfo>,
}

#[derive(Serialize)]
pub struct VerticalInfo {
    pub id: String,
    pub name: String,
    pub slug: String,
}

#[derive(Deserialize)]
pub struct CreatePublisherRequest {
    pub name: String,
    pub email: String,
    pub instance_id: Option<Uuid>,
    pub representative_first_name: Option<String>,
    pub representative_last_name: Option<String>,
    pub address_street: Option<String>,
    pub address_city: Option<String>,
    pub address_state: Option<String>,
    pub address_zip: Option<String>,
    pub timezone: Option<String>,
    pub ein_tin: Option<String>,
    pub vertical_ids: Option<Vec<Uuid>>,
}

#[derive(Deserialize)]
pub struct UpdatePublisherRequest {
    pub name: Option<String>,
    pub email: Option<String>,
    pub status: Option<String>,
    pub hmac_required: Option<bool>,
    pub representative_first_name: Option<String>,
    pub representative_last_name: Option<String>,
    pub address_street: Option<String>,
    pub address_city: Option<String>,
    pub address_state: Option<String>,
    pub address_zip: Option<String>,
    pub timezone: Option<String>,
    pub ein_tin: Option<String>,
}

pub fn dashboard_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/dashboard/publishers", get(list_publishers))
        .route("/api/v1/dashboard/publishers", post(create_publisher))
        .route("/api/v1/dashboard/publishers/:id", get(get_publisher))
        .route("/api/v1/dashboard/publishers/:id", post(update_publisher))
        .route("/api/v1/dashboard/publishers/:id", delete(delete_publisher))
        .route(
            "/api/v1/dashboard/publishers/:id/regenerate-api-key",
            post(regenerate_publisher_api_key),
        )
        .route(
            "/api/v1/dashboard/publishers/:id/api-key",
            get(get_publisher_api_key),
        )
        .route(
            "/api/v1/dashboard/publishers/:id/generate-hmac-secret_encrypted",
            post(generate_publisher_hmac_secret_encrypted),
        )
        .route(
            "/api/v1/dashboard/publishers/:publisher_id/audit_logs",
            get(list_publisher_audit_logs),
        )
        .route("/api/v1/dashboard/buyers", get(list_buyers))
        .route("/api/v1/dashboard/buyers", post(create_buyer))
        .route("/api/v1/dashboard/buyers/:id", get(get_buyer))
        .route("/api/v1/dashboard/buyers/:id", post(update_buyer))
        .route("/api/v1/dashboard/buyers/:id", delete(delete_buyer))
        .route("/api/v1/dashboard/campaigns", get(list_campaigns))
        .route("/api/v1/dashboard/campaigns", post(create_campaign))
        .route("/api/v1/dashboard/campaigns/:id", get(get_campaign))
        .route("/api/v1/dashboard/campaigns/:id", post(update_campaign))
        .route("/api/v1/dashboard/campaigns/:id", delete(delete_campaign))
        .route(
            "/api/v1/dashboard/campaigns/:campaign_id/audit_logs",
            get(list_campaign_audit_logs),
        )
        .route("/api/v1/dashboard/ping_trees", get(list_ping_trees))
        .route("/api/v1/dashboard/ping_trees", post(create_ping_tree))
        .route("/api/v1/dashboard/ping_trees/:id", get(get_ping_tree))
        .route("/api/v1/dashboard/ping_trees/:id", post(update_ping_tree))
        .route("/api/v1/dashboard/ping_trees/:id", delete(delete_ping_tree))
        .route(
            "/api/v1/dashboard/ping_trees/:id/campaigns",
            post(add_campaign_to_ping_tree),
        )
        .route(
            "/api/v1/dashboard/ping_trees/:id/campaigns/:campaign_id",
            delete(remove_campaign_from_ping_tree),
        )
        .route(
            "/api/v1/dashboard/ping_trees/:ping_tree_id/audit_logs",
            get(list_ping_tree_audit_logs),
        )
        .route(
            "/api/v1/dashboard/ping_trees/:id/publishers",
            get(list_ping_tree_publishers),
        )
        .route(
            "/api/v1/dashboard/ping_trees/:id/publishers",
            post(add_publisher_to_ping_tree),
        )
        .route(
            "/api/v1/dashboard/ping_trees/:id/publishers/:publisher_id",
            delete(remove_publisher_from_ping_tree),
        )
        .route(
            "/api/v1/dashboard/ping_trees/:id/publishers/:publisher_id",
            put(update_ping_tree_publisher_revshare),
        )
        .route(
            "/api/v1/dashboard/publishers/:id/revenue-share",
            get(get_publisher_revenue_share),
        )
        .route("/api/v1/dashboard/verticals", get(list_verticals))
        .route("/api/v1/dashboard/leads", get(list_leads))
        .route("/api/v1/dashboard/leads/:id/details", get(get_lead_details))
        .route(
            "/api/v1/dashboard/buyer_integrations",
            get(list_buyer_integrations),
        )
        .route(
            "/api/v1/dashboard/buyers/:buyer_id/rule_sets",
            get(list_buyer_rule_sets),
        )
        .route(
            "/api/v1/dashboard/buyers/:buyer_id/rule_sets",
            post(create_buyer_rule_set),
        )
        .route(
            "/api/v1/dashboard/buyers/:buyer_id/rule_sets/:rule_set_id",
            get(get_buyer_rule_set),
        )
        .route(
            "/api/v1/dashboard/buyers/:buyer_id/rule_sets/:rule_set_id",
            put(update_buyer_rule_set),
        )
        .route(
            "/api/v1/dashboard/buyers/:buyer_id/rule_sets/:rule_set_id",
            delete(delete_buyer_rule_set),
        )
        .route(
            "/api/v1/dashboard/buyers/:buyer_id/audit_logs",
            get(list_buyer_audit_logs),
        )
        .route("/api/security", get(get_security_status))
        .route("/api/security/otp/setup", post(setup_otp))
        .route("/api/security/otp/verify", post(verify_otp))
        .route("/api/security/otp/disable", post(disable_otp))
        .route(
            "/api/security/passkeys/registration_options",
            post(passkey_registration_options),
        )
        .route("/api/security/passkeys/register", post(register_passkey))
        .route("/api/security/passkeys/:id", delete(delete_passkey))
        .route("/api/security/audit_logs", get(list_security_audit_logs))
        .route(
            "/api/security/password-reset-email",
            post(send_password_reset_email),
        )
}

// Security API
async fn get_security_status(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use leadsnebula_core::auth::JwtService;
    use tracing::error;
    use uuid::Uuid;

    // Extract and decode JWT token
    let token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let claims = jwt_service
        .decode(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Load user from database
    let user = sqlx::query_as::<_, leadsnebula_core::models::user::User>(
        "SELECT * FROM instance_users WHERE id = $1 AND status = 'active'",
    )
    .bind(Uuid::parse_str(&claims.user_id).map_err(|_| StatusCode::UNAUTHORIZED)?)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error loading user: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    // Check OTP status
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

    // Load passkeys for this user (so the frontend can list and delete them)
    // DB columns created_at/last_used_at are TIMESTAMP (no TZ); use NaiveDateTime to match.
    #[derive(sqlx::FromRow)]
    struct PasskeyRow {
        id: Uuid,
        name: Option<String>,
        passkey_type: Option<String>,
        created_at: Option<chrono::NaiveDateTime>,
        last_used_at: Option<chrono::NaiveDateTime>,
    }
    let passkey_rows: Vec<PasskeyRow> = sqlx::query_as::<_, PasskeyRow>(
        "SELECT id, name, passkey_type, created_at, last_used_at FROM webauthn_credentials WHERE instance_user_id = $1 ORDER BY created_at ASC",
    )
    .bind(user.id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error loading passkeys: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let passkeys: Vec<serde_json::Value> = passkey_rows
        .into_iter()
        .map(|r| {
            let created_at_rfc3339 = r
                .created_at
                .map(|t| {
                    chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(t, chrono::Utc)
                        .to_rfc3339()
                })
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
            let last_used_at_rfc3339 = r.last_used_at.map(|t| {
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(t, chrono::Utc)
                    .to_rfc3339()
            });
            serde_json::json!({
                "id": r.id.to_string(),
                "name": r.name.unwrap_or_else(|| "Passkey".to_string()),
                "type": r.passkey_type.unwrap_or_else(|| "platform".to_string()),
                "created_at": created_at_rfc3339,
                "last_used_at": last_used_at_rfc3339,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "otp_enabled": otp_enabled.unwrap_or(false),
        "passkeys": passkeys
    })))
}

/// POST /api/security/password-reset-email — send password reset email for the authenticated user.
/// Rate-limited by reset_password_sent_at (5 min). Uses SES via EmailService.
async fn send_password_reset_email(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use base64::Engine;
    use leadsnebula_core::auth::JwtService;
    use leadsnebula_core::password_reset::PasswordResetService;
    use tracing::error;
    use uuid::Uuid;

    let token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let claims = jwt_service
        .decode(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let user_id = Uuid::parse_str(&claims.user_id).map_err(|_| StatusCode::UNAUTHORIZED)?;

    let user = sqlx::query_as::<_, leadsnebula_core::models::user::User>(
        "SELECT id, email, encrypted_password, first_name, last_name, status, confirmed_at, created_at, updated_at FROM instance_users WHERE id = $1 AND status = 'active'",
    )
    .bind(user_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error loading user for password reset: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    // Rate limit: do not send again if last reset email was sent within 5 minutes
    // Column can be NULL (never sent), so decode as Option<DateTime>
    let last_sent: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
            "SELECT reset_password_sent_at FROM instance_users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .map_err(|e| {
            error!("Database error checking reset rate limit: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .flatten();
    if let Some(ts) = last_sent {
        if chrono::Utc::now().signed_duration_since(ts).num_seconds() < 300 {
            return Ok(Json(serde_json::json!({
                "success": true,
                "message": "If an account exists for this email, a reset link was already sent. Please check your inbox or try again later."
            })));
        }
    }

    let reset_token = {
        let mut bytes = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    };

    sqlx::query(
        "UPDATE instance_users SET reset_password_token = $1, reset_password_sent_at = NOW(), updated_at = NOW() WHERE id = $2",
    )
    .bind(&reset_token)
    .bind(user_id)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error saving reset token: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // In development, use request Origin/Referer so the link in the email matches where the user requested from
    let reset_base_url = if state.config.environment.eq_ignore_ascii_case("development") {
        headers
            .get("origin")
            .or_else(|| headers.get("referer"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.trim())
            .map(|s| {
                // Strip path and query (e.g. https://localhost:3000/dashboard -> https://localhost:3000)
                let s = s.trim_end_matches('/');
                if let Some(colon_slash) = s.find("://") {
                    let after_scheme = colon_slash + 3;
                    let rest = &s[after_scheme..];
                    if let Some(slash) = rest.find('/') {
                        s[..after_scheme + slash].to_string()
                    } else {
                        s.to_string()
                    }
                } else {
                    s.to_string()
                }
            })
            .unwrap_or_else(|| state.config.password_reset_base_url.clone())
    } else {
        state.config.password_reset_base_url.clone()
    };

    let password_reset_service =
        PasswordResetService::new(state.email_service.clone(), reset_base_url.clone());
    if let Err(e) = password_reset_service
        .send_reset_email(&user, &reset_token)
        .await
    {
        error!("Failed to send password reset email: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Minimal application log when reset email is sent (AUTO-MEMORY: mask email in logs)
    let masked = mask_email_for_log(&user.email);
    tracing::info!("Password reset email sent for {}", masked);

    Ok(Json(serde_json::json!({ "success": true })))
}

async fn list_security_audit_logs(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use leadsnebula_core::auth::JwtService;
    use sqlx::Row;
    use tracing::error;
    use uuid::Uuid;

    let token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let claims = jwt_service
        .decode(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let user_id = Uuid::parse_str(&claims.user_id).map_err(|_| StatusCode::UNAUTHORIZED)?;

    let logs = sqlx::query(
        r#"
        SELECT al.id, al.action_type, al.details, al.created_at, al.updated_at,
               u.email as user_email, u.first_name as user_first_name, u.last_name as user_last_name
        FROM audit_logs al
        LEFT JOIN instance_users u ON al.instance_user_id = u.id
        WHERE al.instance_user_id = $1
          AND al.action_type IN ('otp_enabled', 'otp_disabled', 'passkey_registered', 'passkey_deleted')
        ORDER BY al.created_at DESC
        LIMIT 100
        "#,
    )
    .bind(user_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing security audit logs: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let audit_logs: Vec<serde_json::Value> = logs
        .iter()
        .map(|row| {
            let user_name = match (
                row.try_get::<Option<String>, _>("user_first_name").ok().flatten(),
                row.try_get::<Option<String>, _>("user_last_name").ok().flatten(),
            ) {
                (Some(first), Some(last)) if !first.is_empty() || !last.is_empty() => {
                    format!("{} {}", first, last).trim().to_string()
                }
                _ => row.try_get::<Option<String>, _>("user_email").ok().flatten().unwrap_or_else(|| "Unknown".to_string()),
            };
            let details_value = row.try_get::<serde_json::Value, _>("details").ok().unwrap_or_else(|| serde_json::json!({}));
            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "action_type": row.try_get::<String, _>("action_type").ok(),
                "user": user_name,
                "details": details_value,
                "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok().map(|dt| dt.to_rfc3339()),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "audit_logs": audit_logs
    })))
}

async fn setup_otp(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use leadsnebula_core::auth::JwtService;
    use tracing::error;
    use uuid::Uuid;

    // Extract and decode JWT token (middleware already validated it, but we decode to get user_id)
    let token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let claims = jwt_service
        .decode(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Load user from database
    let user = sqlx::query_as::<_, leadsnebula_core::models::user::User>(
        "SELECT * FROM instance_users WHERE id = $1 AND status = 'active'",
    )
    .bind(Uuid::parse_str(&claims.user_id).map_err(|_| StatusCode::UNAUTHORIZED)?)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error loading user: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    // Check if OTP is already enabled
    let existing_otp = sqlx::query_scalar::<_, bool>(
        "SELECT enabled FROM user_otp_settings WHERE instance_user_id = $1",
    )
    .bind(user.id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error checking OTP status: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if let Some(true) = existing_otp {
        return Ok(Json(serde_json::json!({
            "success": false,
            "error": "OTP is already enabled. Please disable it first to set up a new one."
        })));
    }

    // Generate a new secret_encrypted (base32 encoded)
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut rng = OsRng;
    let mut secret_encrypted_bytes = [0u8; 20];
    rng.fill_bytes(&mut secret_encrypted_bytes);
    let secret_encrypted = base32::encode(
        base32::Alphabet::Rfc4648 { padding: false },
        &secret_encrypted_bytes,
    );

    // Create or update OTP setting (but don't enable it yet - wait for verification)
    sqlx::query(
        r#"
        INSERT INTO user_otp_settings (instance_user_id, secret_encrypted, enabled, created_at, updated_at)
        VALUES ($1, $2, false, NOW(), NOW())
        ON CONFLICT (instance_user_id) 
        DO UPDATE SET secret_encrypted = $2, enabled = false, updated_at = NOW()
        "#,
    )
    .bind(user.id)
    .bind(&secret_encrypted)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error saving OTP secret_encrypted: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Generate provisioning URI
    let provisioning_uri = format!(
        "otpauth://totp/LeadsNebula:{}?secret={}&issuer=LeadsNebula",
        urlencoding::encode(&user.email),
        secret_encrypted
    );

    // Return the secret (frontend expects "secret") and provisioning URI.
    Ok(Json(serde_json::json!({
        "success": true,
        "secret": secret_encrypted,
        "secret_encrypted": secret_encrypted,
        "provisioning_uri": provisioning_uri
    })))
}

#[derive(Deserialize)]
struct VerifyOtpRequest {
    code: String,
    secret_encrypted: Option<String>, // Optional for now, but we'll get it from DB
}

async fn verify_otp(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<VerifyOtpRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use leadsnebula_core::auth::JwtService;
    use leadsnebula_core::otp::OtpService;
    use tracing::error;
    use uuid::Uuid;

    // Extract and decode JWT token
    let token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let claims = jwt_service
        .decode(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Load user from database
    let user = sqlx::query_as::<_, leadsnebula_core::models::user::User>(
        "SELECT * FROM instance_users WHERE id = $1 AND status = 'active'",
    )
    .bind(Uuid::parse_str(&claims.user_id).map_err(|_| StatusCode::UNAUTHORIZED)?)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error loading user: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    // Get OTP secret_encrypted from database (prefer DB over request payload for security)
    // fetch_optional on query_scalar returns Option<Option<String>> (row found? and value is Some?)
    let db_secret_encrypted: Option<String> = sqlx::query_scalar::<_, Option<String>>(
        "SELECT secret_encrypted FROM user_otp_settings WHERE instance_user_id = $1",
    )
    .bind(user.id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error loading OTP secret_encrypted: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .flatten(); // Flatten Option<Option<String>> to Option<String>

    // Use DB secret_encrypted if available, otherwise fallback to payload secret_encrypted (during setup flow)
    let otp_secret_encrypted = db_secret_encrypted.or(payload.secret_encrypted);

    let secret_encrypted = otp_secret_encrypted.ok_or(StatusCode::BAD_REQUEST)?;

    // Verify OTP code
    let otp_service = OtpService::new(&secret_encrypted).map_err(|e| {
        error!("Failed to create OtpService: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let is_valid = otp_service.verify(&payload.code);

    if !is_valid {
        return Ok(Json(serde_json::json!({
            "success": false,
            "error": "Invalid OTP code. Please try again."
        })));
    }

    // Generate backup codes (8 codes, each 8 characters)
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut rng = OsRng;
    let mut backup_codes_encrypted = Vec::new();
    let chars = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // Exclude ambiguous chars
    for _ in 0..8 {
        let code: String = (0..8)
            .map(|_| {
                let mut buf = [0u8; 1];
                rng.fill_bytes(&mut buf);
                chars.chars().nth((buf[0] as usize) % chars.len()).unwrap()
            })
            .collect();
        backup_codes_encrypted.push(code);
    }
    let backup_codes_encrypted_json = serde_json::to_string(&backup_codes_encrypted)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Enable OTP and save backup codes
    sqlx::query(
        r#"
        UPDATE user_otp_settings
        SET enabled = true, backup_codes_encrypted = $1, last_verified_at = NOW(), updated_at = NOW()
        WHERE instance_user_id = $2
        "#,
    )
    .bind(&backup_codes_encrypted_json)
    .bind(user.id)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error enabling OTP: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if let Some(instance_id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(user.id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .ok()
    .flatten()
    {
        let ip_address = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
        let user_agent = headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let request_id = headers
            .get("x-request-id")
            .or_else(|| headers.get("x-correlation-id"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let audit_details = serde_json::json!({
            "action": "otp_enabled",
            "target_type": "Security",
            "target_id": user.id.to_string(),
            "target_name": user.email,
            "context": { "reason": "User enabled OTP 2FA", "request_id": request_id, "ip_address": ip_address, "user_agent": user_agent, "method": "POST", "endpoint": "/api/security/otp/verify", "source": "dashboard_web_ui" },
            "compliance": { "standard": "ISO_27001_SOC2_NIST", "version": "2024" },
            "outcome": "success",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let _ = create_audit_log(
            state.db_pool.as_ref(),
            Some(instance_id),
            Some(user.id),
            "otp_enabled",
            Some("Security"),
            Some(user.id),
            audit_details,
            serde_json::json!({}),
            ip_address.as_deref(),
            user_agent.as_deref(),
        )
        .await;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "backup_codes_encrypted": backup_codes_encrypted
    })))
}

async fn disable_otp(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use leadsnebula_core::auth::JwtService;
    use tracing::error;
    use uuid::Uuid;

    // Extract and decode JWT token
    let token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let claims = jwt_service
        .decode(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Load user from database
    let user = sqlx::query_as::<_, leadsnebula_core::models::user::User>(
        "SELECT * FROM instance_users WHERE id = $1 AND status = 'active'",
    )
    .bind(Uuid::parse_str(&claims.user_id).map_err(|_| StatusCode::UNAUTHORIZED)?)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error loading user: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    // Check if OTP is enabled
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
        return Ok(Json(serde_json::json!({
            "success": false,
            "error": "OTP is not enabled."
        })));
    }

    // Disable OTP (set enabled to false)
    sqlx::query(
        "UPDATE user_otp_settings SET enabled = false, updated_at = NOW() WHERE instance_user_id = $1",
    )
    .bind(user.id)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error disabling OTP: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if let Some(instance_id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(user.id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .ok()
    .flatten()
    {
        let ip_address = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
        let user_agent = headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let request_id = headers
            .get("x-request-id")
            .or_else(|| headers.get("x-correlation-id"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let audit_details = serde_json::json!({
            "action": "otp_disabled",
            "target_type": "Security",
            "target_id": user.id.to_string(),
            "target_name": user.email,
            "context": { "reason": "User disabled OTP 2FA", "request_id": request_id, "ip_address": ip_address, "user_agent": user_agent, "method": "POST", "endpoint": "/api/security/otp/disable", "source": "dashboard_web_ui" },
            "compliance": { "standard": "ISO_27001_SOC2_NIST", "version": "2024" },
            "outcome": "success",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let _ = create_audit_log(
            state.db_pool.as_ref(),
            Some(instance_id),
            Some(user.id),
            "otp_disabled",
            Some("Security"),
            Some(user.id),
            audit_details,
            serde_json::json!({}),
            ip_address.as_deref(),
            user_agent.as_deref(),
        )
        .await;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "OTP disabled successfully."
    })))
}

#[derive(Deserialize)]
struct PasskeyRegistrationOptionsRequest {
    name: String,
}

#[derive(Deserialize)]
struct RegisterPasskeyRequest {
    challenge_token: String,
    name: String,
    credential: serde_json::Value,
}

async fn passkey_registration_options(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(_payload): Json<PasskeyRegistrationOptionsRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use leadsnebula_core::auth::JwtService;
    #[cfg(feature = "webauthn")]
    use rand::rngs::OsRng;
    #[cfg(feature = "webauthn")]
    use rand::RngCore;
    use tracing::error;
    use uuid::Uuid;
    #[cfg(feature = "webauthn")]
    use webauthn_rs::prelude::*;

    // Extract and decode JWT token
    let token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let claims = jwt_service
        .decode(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Load user from database
    let user = sqlx::query_as::<_, leadsnebula_core::models::user::User>(
        "SELECT * FROM instance_users WHERE id = $1 AND status = 'active'",
    )
    .bind(Uuid::parse_str(&claims.user_id).map_err(|_| StatusCode::UNAUTHORIZED)?)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error loading user: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    // Check if user can add passkey (max 3)
    let passkey_count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM webauthn_credentials WHERE instance_user_id = $1",
    )
    .bind(user.id)
    .fetch_one(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error checking passkey count: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if passkey_count >= 3 {
        return Ok(Json(serde_json::json!({
            "success": false,
            "error": "Maximum of 3 passkeys allowed. Please remove a passkey before adding a new one."
        })));
    }

    #[cfg(feature = "webauthn")]
    {
        // Host-bound by default so Proton Pass works (OriginRpMissmatch). Set WEBAUTHN_USE_HOST_AS_RP_ID=false for parent-domain.
        let use_host_as_rp_id = std::env::var("WEBAUTHN_USE_HOST_AS_RP_ID")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        let (rp_id, origin) =
            webauthn_rp_id_and_origin(&state.config.environment, &headers, use_host_as_rp_id)?;

        // Create WebAuthn instance. The url crate treats "localhost" as no host, so
        // rp_origin.domain() is None and webauthn-rs returns Configuration. Use a
        // synthetic origin that passes the check and add the real browser origin.
        let url = url::Url::parse(&origin).map_err(|e| {
            error!("Failed to parse origin URL: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let webauthn = match WebauthnBuilder::new(&rp_id, &url) {
            Ok(builder) => builder
                .allow_any_port(rp_id == "localhost.localdomain")
                .rp_name("LeadsNebula")
                .build()
                .map_err(|e| {
                    error!("Failed to build WebAuthn instance: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?,
            Err(webauthn_rs::prelude::WebauthnError::Configuration) if rp_id == "localhost" => {
                let builder_url =
                    url::Url::parse("https://localhost.localdomain").map_err(|e| {
                        error!("Failed to parse localhost workaround URL: {}", e);
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
                let mut b = WebauthnBuilder::new(&rp_id, &builder_url)
                    .map_err(|e| {
                        error!(
                            "Failed to create WebAuthn builder (localhost workaround): {}",
                            e
                        );
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?
                    .append_allowed_origin(&url);
                for origin in [
                    "http://localhost:3000",
                    "https://localhost.localdomain:3000",
                ] {
                    if let Ok(u) = url::Url::parse(origin) {
                        b = b.append_allowed_origin(&u);
                    }
                }
                b.allow_any_port(true)
                    .rp_name("LeadsNebula")
                    .build()
                    .map_err(|e| {
                        error!(
                            "Failed to build WebAuthn instance (localhost workaround): {}",
                            e
                        );
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?
            }
            Err(_) if rp_id == "127.0.0.1" => {
                return Ok(Json(serde_json::json!({
                    "success": false,
                    "error": "Passkey registration from 127.0.0.1 is not supported by the server library. Please use https://localhost:3000 and ensure your certificate is trusted (e.g. mkcert)."
                })));
            }
            Err(e) => {
                error!("Failed to create WebAuthn builder: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        };

        // Create registration options
        let user_id_bytes = user.id.as_bytes().to_vec();
        let (ccr, reg_session) = webauthn
            .start_passkey_registration(
                Uuid::from_bytes(user_id_bytes.try_into().map_err(|_| {
                    error!("Invalid user ID format");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?),
                &user.email,
                &user.email,
                None,
            )
            .map_err(|e| {
                error!("Failed to create registration options: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        // Generate challenge token
        let mut rng = OsRng;
        let mut challenge_token_bytes = [0u8; 16];
        rng.fill_bytes(&mut challenge_token_bytes);
        let challenge_token = hex::encode(challenge_token_bytes);

        // Store full PasskeyRegistration session in Redis for verification (same challenge as options)
        let cache_key = format!("webauthn_registration:{}:{}", user.id, challenge_token);
        if let Some(redis) = &state.redis {
            let reg_session_json = serde_json::to_value(&reg_session).map_err(|e| {
                error!("Failed to serialize PasskeyRegistration: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            let cache_data = serde_json::json!({
                "user_id": user.id.to_string(),
                "reg_session": reg_session_json
            });
            redis
                .set_with_ttl(&cache_key, &cache_data.to_string(), 300)
                .await
                .map_err(|e| {
                    error!("Failed to store registration session in Redis: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
        } else {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }

        // Convert options to JSON
        // webauthn-rs serializes CreationChallengeResponse with 'publicKey' (camelCase)
        // but frontend expects 'public_key' (snake_case), so we need to transform it
        let options_json = serde_json::to_value(&ccr).map_err(|e| {
            error!("Failed to serialize options: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        // Log the structure for debugging
        use tracing::debug;
        debug!(
            "Serialized CreationChallengeResponse: {}",
            serde_json::to_string_pretty(&options_json).unwrap_or_default()
        );

        // Transform publicKey to public_key for frontend compatibility
        // Also ensure challenge is accessible at the expected location
        let transformed_options = if let Some(public_key_value) = options_json.get("publicKey") {
            let mut transformed = options_json.clone();
            transformed.as_object_mut().unwrap().remove("publicKey");
            transformed
                .as_object_mut()
                .unwrap()
                .insert("public_key".to_string(), public_key_value.clone());
            transformed
        } else if options_json.get("public_key").is_some() {
            // Already has public_key, use as-is
            options_json
        } else {
            // No publicKey/public_key found, check if challenge is at top level
            // This shouldn't happen with webauthn-rs, but handle it gracefully
            error!("CreationChallengeResponse missing publicKey/public_key field");
            options_json
        };

        let mut response = serde_json::json!({
            "success": true,
            "options": transformed_options,
            "challenge_token": challenge_token
        });
        // Include WebAuthn debug info when X-Debug-Webauthn: 1 to verify Origin header
        if headers
            .get("x-debug-webauthn")
            .and_then(|h| h.to_str().ok())
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            if let Some(obj) = response.as_object_mut() {
                obj.insert(
                    "webauthn_debug".to_string(),
                    serde_json::json!({
                        "origin_received": headers.get("origin").and_then(|h| h.to_str().ok()).unwrap_or("(no Origin header)"),
                        "rp_id": rp_id,
                        "origin_used": origin,
                        "use_host_as_rp_id": use_host_as_rp_id
                    }),
                );
            }
        }
        Ok(Json(response))
    }

    #[cfg(not(feature = "webauthn"))]
    {
        Err(StatusCode::NOT_IMPLEMENTED)
    }
}

async fn register_passkey(
    State(_state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<RegisterPasskeyRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    #[cfg(feature = "webauthn")]
    {
        let state = _state;
        use leadsnebula_core::auth::JwtService;
        use tracing::error;
        use uuid::Uuid;
        use webauthn_rs::prelude::*;

        // Extract and decode JWT token
        let token = headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or(StatusCode::UNAUTHORIZED)?;

        let jwt_service = JwtService::new(state.config.jwt_secret.clone());
        let claims = jwt_service
            .decode(token)
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        // Load user from database
        let user = sqlx::query_as::<_, leadsnebula_core::models::user::User>(
            "SELECT * FROM instance_users WHERE id = $1 AND status = 'active'",
        )
        .bind(Uuid::parse_str(&claims.user_id).map_err(|_| StatusCode::UNAUTHORIZED)?)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .map_err(|e| {
            error!("Database error loading user: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::UNAUTHORIZED)?;
        // Verify challenge from Redis
        let cache_key = format!(
            "webauthn_registration:{}:{}",
            user.id, payload.challenge_token
        );
        let cached_data = if let Some(redis) = &state.redis {
            redis.get(&cache_key).await.map_err(|e| {
                error!("Failed to get challenge from Redis: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
        } else {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        };

        let cached_data_str = cached_data.ok_or_else(|| {
            error!("Invalid or expired registration challenge");
            StatusCode::BAD_REQUEST
        })?;

        let cached_data: serde_json::Value =
            serde_json::from_str(&cached_data_str).map_err(|_| {
                error!("Failed to parse cached registration data");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        let stored_user_id = cached_data
            .get("user_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                error!("Invalid cached data format: missing user_id");
                StatusCode::BAD_REQUEST
            })?;

        if stored_user_id != user.id.to_string() {
            return Ok(Json(serde_json::json!({
                "success": false,
                "error": "Invalid or expired registration challenge. Please try again."
            })));
        }

        let reg_session_value = cached_data.get("reg_session").ok_or_else(|| {
            error!(
                "Invalid cached data format: missing reg_session (store full PasskeyRegistration)"
            );
            StatusCode::BAD_REQUEST
        })?;
        let reg_session: webauthn_rs::prelude::PasskeyRegistration =
            serde_json::from_value(reg_session_value.clone()).map_err(|e| {
                error!("Failed to deserialize PasskeyRegistration: {}", e);
                StatusCode::BAD_REQUEST
            })?;

        let use_host_as_rp_id = std::env::var("WEBAUTHN_USE_HOST_AS_RP_ID")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        let (rp_id, origin) =
            webauthn_rp_id_and_origin(&state.config.environment, &headers, use_host_as_rp_id)?;

        // Create WebAuthn instance (same rp_id/origin as registration_options; localhost workaround as in passkey_registration_options)
        let url = url::Url::parse(&origin).map_err(|e| {
            error!("Failed to parse origin URL: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let webauthn = match WebauthnBuilder::new(&rp_id, &url) {
            Ok(builder) => builder
                .allow_any_port(rp_id == "localhost.localdomain")
                .rp_name("LeadsNebula")
                .build()
                .map_err(|e| {
                    error!("Failed to build WebAuthn instance: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?,
            Err(webauthn_rs::prelude::WebauthnError::Configuration) if rp_id == "localhost" => {
                let builder_url =
                    url::Url::parse("https://localhost.localdomain").map_err(|e| {
                        error!("Failed to parse localhost workaround URL: {}", e);
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
                let mut b = WebauthnBuilder::new(&rp_id, &builder_url)
                    .map_err(|e| {
                        error!(
                            "Failed to create WebAuthn builder (localhost workaround): {}",
                            e
                        );
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?
                    .append_allowed_origin(&url);
                for origin in [
                    "http://localhost:3000",
                    "https://localhost.localdomain:3000",
                ] {
                    if let Ok(u) = url::Url::parse(origin) {
                        b = b.append_allowed_origin(&u);
                    }
                }
                b.allow_any_port(true)
                    .rp_name("LeadsNebula")
                    .build()
                    .map_err(|e| {
                        error!(
                            "Failed to build WebAuthn instance (localhost workaround): {}",
                            e
                        );
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?
            }
            Err(_) if rp_id == "127.0.0.1" => {
                return Ok(Json(serde_json::json!({
                    "success": false,
                    "error": "Passkey registration from 127.0.0.1 is not supported. Please use https://localhost:3000."
                })));
            }
            Err(e) => {
                error!("Failed to create WebAuthn builder: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        };

        // Parse credential from request
        let reg_pkc =
            serde_json::from_value::<RegisterPublicKeyCredential>(payload.credential.clone())
                .map_err(|e| {
                    error!("Failed to parse credential: {}", e);
                    StatusCode::BAD_REQUEST
                })?;

        // Verify the credential using the stored session (same challenge we sent to the client)
        let passkey = webauthn
            .finish_passkey_registration(&reg_pkc, &reg_session)
            .map_err(|e| {
                error!("Failed to verify credential: {}", e);
                StatusCode::BAD_REQUEST
            })?;

        // One-time use: remove registration session from Redis
        if let Some(redis) = &state.redis {
            let _ = redis.delete(&cache_key).await;
        }

        // Determine passkey type from credential
        // Check if it's a platform authenticator (soft) or cross-platform (physical)
        let passkey_type = if let Some(_authenticator_data) = payload
            .credential
            .get("response")
            .and_then(|r| r.get("authenticatorData"))
            .and_then(|a| a.as_str())
        {
            // Could parse authenticator data to determine type
            // For now, default to 'soft' (platform authenticator)
            "soft"
        } else {
            "soft"
        };

        // Save to database
        let passkey_id = Uuid::new_v4();

        // Access credential data through public methods
        let cred_id = passkey.cred_id().to_string();
        // Serialize the passkey to get public key - webauthn-rs Passkey should be serializable
        let public_key_str = serde_json::to_string(&passkey).map_err(|e| {
            error!("Failed to serialize passkey: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        // Get counter/sign_count - will be updated on first use
        let sign_count = 0i32;

        sqlx::query(
            r#"
            INSERT INTO webauthn_credentials (
                id, platform_user_id, instance_user_id, external_id, public_key, sign_count,
                name, passkey_type, created_at, updated_at
            ) VALUES ($1, $2, $2, $3, $4, $5, $6, $7, NOW(), NOW())
            "#,
        )
        .bind(passkey_id)
        .bind(user.id)
        .bind(cred_id)
        .bind(public_key_str)
        .bind(sign_count)
        .bind(&payload.name)
        .bind(passkey_type)
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|e| {
            error!("Database error saving passkey: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        // Clear cache entry
        if let Some(redis) = &state.redis {
            let _ = redis.delete(&cache_key).await;
        }

        if let Some(instance_id) = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(user.id)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .ok()
        .flatten()
        {
            let ip_address = headers
                .get("x-forwarded-for")
                .or_else(|| headers.get("x-real-ip"))
                .and_then(|h| h.to_str().ok())
                .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
            let user_agent = headers
                .get("user-agent")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string());
            let request_id = headers
                .get("x-request-id")
                .or_else(|| headers.get("x-correlation-id"))
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let audit_details = serde_json::json!({
                "action": "passkey_registered",
                "target_type": "Security",
                "target_id": user.id.to_string(),
                "target_name": payload.name,
                "context": { "reason": "User registered passkey", "request_id": request_id, "ip_address": ip_address, "user_agent": user_agent, "method": "POST", "endpoint": "/api/security/passkeys/register", "source": "dashboard_web_ui" },
                "compliance": { "standard": "ISO_27001_SOC2_NIST", "version": "2024" },
                "outcome": "success",
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            let _ = create_audit_log(
                state.db_pool.as_ref(),
                Some(instance_id),
                Some(user.id),
                "passkey_registered",
                Some("Security"),
                Some(user.id),
                audit_details,
                serde_json::json!({}),
                ip_address.as_deref(),
                user_agent.as_deref(),
            )
            .await;
        }

        Ok(Json(serde_json::json!({
            "success": true,
            "passkey": {
                "id": passkey_id.to_string(),
                "name": payload.name,
                "type": passkey_type,
                "created_at": chrono::Utc::now().to_rfc3339()
            }
        })))
    }

    #[cfg(not(feature = "webauthn"))]
    {
        let _ = _state;
        let _ = headers;
        let _ = payload;
        Err(StatusCode::NOT_IMPLEMENTED)
    }
}

async fn delete_passkey(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(passkey_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use leadsnebula_core::auth::JwtService;
    use tracing::error;
    use uuid::Uuid;

    let token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let claims = jwt_service
        .decode(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let user_id = Uuid::parse_str(&claims.user_id).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let passkey_uuid = Uuid::parse_str(&passkey_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let result =
        sqlx::query("DELETE FROM webauthn_credentials WHERE id = $1 AND instance_user_id = $2")
            .bind(passkey_uuid)
            .bind(user_id)
            .execute(state.db_pool.as_ref())
            .await
            .map_err(|e| {
                error!("Database error deleting passkey: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

    if result.rows_affected() == 0 {
        return Ok(Json(serde_json::json!({
            "success": false,
            "error": "Passkey not found or you do not have permission to delete it."
        })));
    }

    if let Some(instance_id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .ok()
    .flatten()
    {
        let ip_address = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
        let user_agent = headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let request_id = headers
            .get("x-request-id")
            .or_else(|| headers.get("x-correlation-id"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let audit_details = serde_json::json!({
            "action": "passkey_deleted",
            "target_type": "Security",
            "target_id": user_id.to_string(),
            "target_name": passkey_id,
            "context": { "reason": "User deleted passkey", "request_id": request_id, "ip_address": ip_address, "user_agent": user_agent, "method": "DELETE", "endpoint": format!("/api/security/passkeys/{}", passkey_id), "source": "dashboard_web_ui" },
            "compliance": { "standard": "ISO_27001_SOC2_NIST", "version": "2024" },
            "outcome": "success",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let _ = create_audit_log(
            state.db_pool.as_ref(),
            Some(instance_id),
            Some(user_id),
            "passkey_deleted",
            Some("Security"),
            Some(user_id),
            audit_details,
            serde_json::json!({}),
            ip_address.as_deref(),
            user_agent.as_deref(),
        )
        .await;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Passkey deleted successfully."
    })))
}

// Temporary handler to test - will be replaced
async fn _setup_otp_with_user(
    State(state): State<AppState>,
    user: leadsnebula_core::models::user::User,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Check if OTP is already enabled
    let existing_otp = sqlx::query_scalar::<_, bool>(
        "SELECT enabled FROM user_otp_settings WHERE instance_user_id = $1",
    )
    .bind(user.id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error checking OTP status: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if let Some(true) = existing_otp {
        return Ok(Json(serde_json::json!({
            "success": false,
            "error": "OTP is already enabled. Please disable it first to set up a new one."
        })));
    }

    // Generate a new secret_encrypted (base32 encoded)
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut rng = OsRng;
    let mut secret_encrypted_bytes = [0u8; 20];
    rng.fill_bytes(&mut secret_encrypted_bytes);
    let secret_encrypted = base32::encode(
        base32::Alphabet::Rfc4648 { padding: false },
        &secret_encrypted_bytes,
    );

    // Create or update OTP setting (but don't enable it yet - wait for verification)
    sqlx::query(
        r#"
        INSERT INTO user_otp_settings (instance_user_id, secret_encrypted, enabled, created_at, updated_at)
        VALUES ($1, $2, false, NOW(), NOW())
        ON CONFLICT (instance_user_id) 
        DO UPDATE SET secret_encrypted = $2, enabled = false, updated_at = NOW()
        "#,
    )
    .bind(user.id)
    .bind(&secret_encrypted)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error saving OTP secret_encrypted: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Generate provisioning URI
    let provisioning_uri = format!(
        "otpauth://totp/LeadsNebula:{}?secret={}&issuer=LeadsNebula",
        urlencoding::encode(&user.email),
        secret_encrypted
    );

    // Return the secret (frontend expects "secret") and provisioning URI.
    Ok(Json(serde_json::json!({
        "success": true,
        "secret": secret_encrypted,
        "secret_encrypted": secret_encrypted,
        "provisioning_uri": provisioning_uri
    })))
}

async fn list_publishers(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sqlx::Row;
    use tracing::{error, info};
    let user = get_user_from_request(&request).ok_or(StatusCode::UNAUTHORIZED)?;
    let instance_id = get_instance_id_for_user(state.db_pool.as_ref(), user.id)
        .await
        .map_err(|e| {
            error!("Database error getting instance for user: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or_else(|| {
            tracing::error!("User has no instance");
            StatusCode::BAD_REQUEST
        })?;
    let start = std::time::Instant::now();

    let publishers = sqlx::query_as::<_, leadsnebula_core::models::publisher::Publisher>(
        "SELECT * FROM publishers WHERE instance_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
    )
    .bind(instance_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing publishers: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    info!("Found {} publishers", publishers.len());

    // If there are publishers, fetch all publisher vertical associations in one query (avoid N+1)
    let mut publisher_verticals_map: std::collections::HashMap<uuid::Uuid, Vec<VerticalInfo>> =
        std::collections::HashMap::new();

    if !publishers.is_empty() {
        let ids: Vec<uuid::Uuid> = publishers.iter().map(|p| p.id).collect();

        let pv_rows = sqlx::query(
            r#"SELECT pv.publisher_id AS publisher_id, v.id AS id, v.name AS name, v.slug AS slug
               FROM publisher_verticals pv
               JOIN verticals v ON v.id = pv.vertical_id
               WHERE pv.publisher_id = ANY($1)
               ORDER BY v.name"#,
        )
        .bind(&ids)
        .fetch_all(state.db_pool.as_ref())
        .await
        .map_err(|e| {
            error!("Database error loading publisher verticals: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        for row in pv_rows {
            let pub_id: uuid::Uuid = row.get("publisher_id");
            let v_id: uuid::Uuid = row.get("id");
            let v_name: String = row.get("name");
            let v_slug: String = row.get("slug");

            publisher_verticals_map
                .entry(pub_id)
                .or_default()
                .push(VerticalInfo {
                    id: v_id.to_string(),
                    name: v_name,
                    slug: v_slug,
                });
        }
    }

    let mut response: Vec<PublisherResponse> = Vec::with_capacity(publishers.len());
    for p in &publishers {
        let vertical_info = publisher_verticals_map.remove(&p.id).unwrap_or_default();
        response.push(PublisherResponse {
            id: p.id.to_string(),
            name: p.name.clone(),
            email: p.email.clone(),
            status: p.status.as_str().to_string(),
            api_key_prefix: p.api_key_prefix.clone(),
            hmac_required: p.hmac_required,
            created_at: p.created_at.to_rfc3339(),
            deleted_at: p.deleted_at.map(|dt| dt.to_rfc3339()),
            verticals: vertical_info,
        });
    }

    let elapsed = start.elapsed().as_millis();
    info!("list_publishers completed in {}ms", elapsed);

    Ok(Json(serde_json::json!({
        "success": true,
        "publishers": response
    })))
}

async fn create_publisher(
    State(state): State<AppState>,
    Extension(user): Extension<leadsnebula_core::models::user::User>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreatePublisherRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use leadsnebula_core::encryption::EncryptionService;
    use rand::Rng;
    use sha2::{Digest, Sha256};

    // Scope to current user's instance only (never attach to another instance)
    let instance_id = if let Some(id) = payload.instance_id {
        id
    } else {
        get_instance_id_for_user(state.db_pool.as_ref(), user.id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or_else(|| {
                tracing::error!(
                    "User has no instance. Cannot create publisher without an instance."
                );
                StatusCode::BAD_REQUEST
            })?
    };

    // Generate API key
    let api_key = format!("pk_live_{}", {
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.gen_range(0..=255)).collect();
        hex::encode(bytes)
    });

    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let api_key_hash = hex::encode(hasher.finalize());

    // Encrypt API key for storage (api_key_encrypted column is NOT NULL)
    let encryption_service = EncryptionService::new(&state.config.encryption_key).map_err(|e| {
        tracing::error!("Failed to initialize encryption service: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let api_key_encrypted = encryption_service.encrypt(&api_key).map_err(|e| {
        tracing::error!("Failed to encrypt API key: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Check if a non-deleted publisher with this email already exists
    // Allow emails from deleted publishers to be reused
    let existing_publisher = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM publishers WHERE email = $1 AND deleted_at IS NULL)",
    )
    .bind(&payload.email)
    .fetch_one(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        tracing::error!("Database error checking existing publisher: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if existing_publisher {
        return Ok(Json(serde_json::json!({
            "success": false,
            "error": format!("A publisher with the email '{}' already exists. Please use a different email address.", payload.email)
        })));
    }

    let publisher_id = Uuid::new_v4();

    let insert_result = sqlx::query(
        r#"
        INSERT INTO publishers (
            id, name, email, api_key_hash, api_key_prefix, api_key_encrypted, status,
            instance_id, is_documentation_test, created_at, updated_at,
            representative_first_name, representative_last_name,
            address_street, address_city, address_state, address_zip,
            timezone, ein_tin
        ) VALUES (
            $1, $2, $3, $4, 'pk_live_', $5, 'active',
            $6, false, NOW(), NOW(),
            $7, $8, $9, $10, $11, $12, $13, $14
        )
        "#,
    )
    .bind(publisher_id)
    .bind(&payload.name)
    .bind(&payload.email)
    .bind(&api_key_hash)
    .bind(&api_key_encrypted)
    .bind(instance_id)
    .bind(&payload.representative_first_name)
    .bind(&payload.representative_last_name)
    .bind(&payload.address_street)
    .bind(&payload.address_city)
    .bind(&payload.address_state)
    .bind(&payload.address_zip)
    .bind(&payload.timezone)
    .bind(&payload.ein_tin)
    .execute(state.db_pool.as_ref())
    .await;

    match insert_result {
        Ok(_result) => {
            // Insert verticals if provided
            if let Some(vertical_ids) = &payload.vertical_ids {
                for vertical_id in vertical_ids {
                    let _ = sqlx::query(
                        "INSERT INTO publisher_verticals (publisher_id, vertical_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"
                    )
                    .bind(publisher_id)
                    .bind(vertical_id)
                    .execute(state.db_pool.as_ref())
                    .await;
                }
            }
        }
        Err(e) => {
            // Check for unique constraint violations (fallback in case migration hasn't been run)
            let error_str = e.to_string();
            if error_str.contains("duplicate key") {
                if error_str.contains("publishers_email_key")
                    || error_str.contains("publishers_email_unique_not_deleted")
                {
                    // Return a user-friendly error response instead of 500
                    return Ok(Json(serde_json::json!({
                        "success": false,
                        "error": format!("A publisher with the email '{}' already exists. Please use a different email address.", payload.email)
                    })));
                } else if error_str.contains("publishers_api_key_hash") {
                    // Extremely unlikely but handle api_key_hash collision
                    tracing::warn!(
                        "API key hash collision detected for publisher creation. Retrying..."
                    );
                    // For now, return error - in production, you might want to retry with a new key
                    return Ok(Json(serde_json::json!({
                        "success": false,
                        "error": "An internal error occurred while generating the API key. Please try again."
                    })));
                }
            }

            // Log other database errors for debugging
            tracing::error!("Database error creating publisher: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    {
        let instance_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(user.id)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .ok()
        .flatten();
        let ip_address = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
        let user_agent = headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let request_id = headers
            .get("x-request-id")
            .or_else(|| headers.get("x-correlation-id"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let audit_details = serde_json::json!({
            "action": "create",
            "target_type": "Publisher",
            "target_id": publisher_id.to_string(),
            "target_name": payload.name,
            "context": {
                "reason": "User created publisher via dashboard",
                "request_id": request_id,
                "ip_address": ip_address,
                "user_agent": user_agent,
                "method": "POST",
                "endpoint": "/api/v1/dashboard/publishers",
                "source": "dashboard_web_ui"
            },
            "compliance": { "standard": "ISO_27001_SOC2_NIST", "version": "2024" },
            "outcome": "success",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "after": { "name": payload.name, "email": payload.email, "status": "active" }
        });
        let _ = create_audit_log(
            state.db_pool.as_ref(),
            instance_id,
            Some(user.id),
            "publisher_created",
            Some("Publisher"),
            Some(publisher_id),
            audit_details,
            serde_json::json!({}),
            ip_address.as_deref(),
            user_agent.as_deref(),
        )
        .await;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "publisher": {
            "id": publisher_id.to_string(),
            "name": payload.name,
            "email": payload.email,
            "status": "active",
            "api_key_prefix": "pk_live_"
        },
        "api_key": api_key,
        "message": "Publisher created successfully. Save your API key - it will not be shown again!"
    })))
}

async fn get_publisher(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let publisher = sqlx::query_as::<_, leadsnebula_core::models::publisher::Publisher>(
        "SELECT * FROM publishers WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    // Get verticals for this publisher
    let verticals = sqlx::query_as::<_, leadsnebula_core::models::vertical::Vertical>(
        r#"
        SELECT v.* FROM verticals v
        INNER JOIN publisher_verticals pv ON v.id = pv.vertical_id
        WHERE pv.publisher_id = $1
        ORDER BY v.name
        "#,
    )
    .bind(id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .unwrap_or_default();

    let vertical_info: Vec<serde_json::Value> = verticals
        .iter()
        .map(|v| {
            serde_json::json!({
                "id": v.id.to_string(),
                "name": v.name,
                "slug": v.slug,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "publisher": {
            "id": publisher.id.to_string(),
            "name": publisher.name,
            "email": publisher.email,
            "status": publisher.status,
            "api_key_prefix": publisher.api_key_prefix,
            "hmac_required": publisher.hmac_required,
            "hmac_secret_encrypted_prefix": publisher.hmac_secret_prefix.clone(),
            "hmac_secret_encrypted_generated": publisher.hmac_secret_hash.is_some(),
            "total_requests": publisher.total_requests,
            "last_request_at": publisher.last_request_at.map(|dt| dt.to_rfc3339()),
            "representative_first_name": publisher.representative_first_name,
            "representative_last_name": publisher.representative_last_name,
            "address_street": publisher.address_street,
            "address_city": publisher.address_city,
            "address_state": publisher.address_state,
            "address_zip": publisher.address_zip,
            "timezone": publisher.timezone,
            "ein_tin": publisher.ein_tin,
            "created_at": publisher.created_at.to_rfc3339(),
            "deleted_at": publisher.deleted_at.map(|dt| dt.to_rfc3339()),
            "verticals": vertical_info
        }
    })))
}

async fn update_publisher(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(user): Extension<leadsnebula_core::models::user::User>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<UpdatePublisherRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    let publisher_before = sqlx::query_as::<_, leadsnebula_core::models::publisher::Publisher>(
        "SELECT * FROM publishers WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching publisher: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let publisher_before = match &publisher_before {
        Some(p) => p,
        None => return Err(StatusCode::NOT_FOUND),
    };

    // Build update query dynamically
    let mut query_builder = sqlx::QueryBuilder::new("UPDATE publishers SET ");

    let mut has_updates = false;

    if let Some(name) = &payload.name {
        query_builder.push("name = ");
        query_builder.push_bind(name);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(email) = &payload.email {
        query_builder.push("email = ");
        query_builder.push_bind(email);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(status) = &payload.status {
        query_builder.push("status = ");
        query_builder.push_bind(status);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(hmac_required) = &payload.hmac_required {
        query_builder.push("hmac_required = ");
        query_builder.push_bind(hmac_required);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(rep_first) = &payload.representative_first_name {
        query_builder.push("representative_first_name = ");
        query_builder.push_bind(rep_first);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(rep_last) = &payload.representative_last_name {
        query_builder.push("representative_last_name = ");
        query_builder.push_bind(rep_last);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(addr_street) = &payload.address_street {
        query_builder.push("address_street = ");
        query_builder.push_bind(addr_street);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(addr_city) = &payload.address_city {
        query_builder.push("address_city = ");
        query_builder.push_bind(addr_city);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(addr_state) = &payload.address_state {
        query_builder.push("address_state = ");
        query_builder.push_bind(addr_state);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(addr_zip) = &payload.address_zip {
        query_builder.push("address_zip = ");
        query_builder.push_bind(addr_zip);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(tz) = &payload.timezone {
        query_builder.push("timezone = ");
        query_builder.push_bind(tz);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(ein) = &payload.ein_tin {
        query_builder.push("ein_tin = ");
        query_builder.push_bind(ein);
        query_builder.push(", ");
        has_updates = true;
    }

    if !has_updates {
        return Err(StatusCode::BAD_REQUEST);
    }

    query_builder.push("updated_at = NOW() WHERE id = ");
    query_builder.push_bind(id);
    query_builder.push(" AND deleted_at IS NULL");

    let query = query_builder.build();
    query.execute(state.db_pool.as_ref()).await.map_err(|e| {
        error!("Database error updating publisher: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let publisher_after = sqlx::query_as::<_, leadsnebula_core::models::publisher::Publisher>(
        "SELECT * FROM publishers WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .ok()
    .flatten();

    if let Some(after) = publisher_after {
        let instance_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(user.id)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .ok()
        .flatten();
        let ip_address = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
        let user_agent = headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let request_id = headers
            .get("x-request-id")
            .or_else(|| headers.get("x-correlation-id"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let before_json = serde_json::json!({
            "name": publisher_before.name,
            "email": publisher_before.email,
            "status": publisher_before.status,
            "representative_first_name": publisher_before.representative_first_name,
            "representative_last_name": publisher_before.representative_last_name,
            "address_street": publisher_before.address_street,
            "address_city": publisher_before.address_city,
            "address_state": publisher_before.address_state,
            "address_zip": publisher_before.address_zip,
            "timezone": publisher_before.timezone,
            "ein_tin": publisher_before.ein_tin,
            "hmac_required": publisher_before.hmac_required,
        });
        let after_json = serde_json::json!({
            "name": after.name,
            "email": after.email,
            "status": after.status,
            "representative_first_name": after.representative_first_name,
            "representative_last_name": after.representative_last_name,
            "address_street": after.address_street,
            "address_city": after.address_city,
            "address_state": after.address_state,
            "address_zip": after.address_zip,
            "timezone": after.timezone,
            "ein_tin": after.ein_tin,
            "hmac_required": after.hmac_required,
        });
        let changed_fields: std::collections::HashMap<String, serde_json::Value> = [
            "name",
            "email",
            "status",
            "representative_first_name",
            "representative_last_name",
            "address_street",
            "address_city",
            "address_state",
            "address_zip",
            "timezone",
            "ein_tin",
            "hmac_required",
        ]
        .iter()
        .filter_map(|&key| {
            let b = before_json.get(key)?;
            let a = after_json.get(key)?;
            if b != a {
                Some((
                    key.to_string(),
                    serde_json::json!({ "before": b, "after": a }),
                ))
            } else {
                None
            }
        })
        .collect();
        let audit_details = serde_json::json!({
            "action": "update",
            "target_type": "Publisher",
            "target_id": id.to_string(),
            "target_name": after.name,
            "actor": { "id": user.id.to_string(), "email": user.email },
            "changes": changed_fields,
            "before": before_json,
            "after": after_json,
            "context": {
                "reason": "User updated publisher via dashboard",
                "request_id": request_id,
                "ip_address": ip_address,
                "user_agent": user_agent,
                "method": "POST",
                "endpoint": format!("/api/v1/dashboard/publishers/{}", id),
                "source": "dashboard_web_ui"
            },
            "compliance": { "standard": "ISO_27001_SOC2_NIST", "version": "2024" },
            "outcome": "success",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let _ = create_audit_log(
            state.db_pool.as_ref(),
            instance_id,
            Some(user.id),
            "publisher_updated",
            Some("Publisher"),
            Some(id),
            audit_details,
            serde_json::json!({}),
            ip_address.as_deref(),
            user_agent.as_deref(),
        )
        .await;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Publisher updated successfully"
    })))
}

async fn delete_publisher(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(user): Extension<leadsnebula_core::models::user::User>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let publisher_before = sqlx::query_as::<_, leadsnebula_core::models::publisher::Publisher>(
        "SELECT * FROM publishers WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    sqlx::query("UPDATE publishers SET deleted_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(pub_before) = publisher_before {
        let instance_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(user.id)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .ok()
        .flatten();
        let ip_address = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
        let user_agent = headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let request_id = headers
            .get("x-request-id")
            .or_else(|| headers.get("x-correlation-id"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let audit_details = serde_json::json!({
            "action": "delete",
            "target_type": "Publisher",
            "target_id": id.to_string(),
            "target_name": pub_before.name,
            "context": {
                "reason": "User deleted publisher via dashboard",
                "request_id": request_id,
                "ip_address": ip_address,
                "user_agent": user_agent,
                "method": "DELETE",
                "endpoint": format!("/api/v1/dashboard/publishers/{}", id),
                "source": "dashboard_web_ui"
            },
            "compliance": { "standard": "ISO_27001_SOC2_NIST", "version": "2024" },
            "outcome": "success",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "before": { "name": pub_before.name, "email": pub_before.email }
        });
        let _ = create_audit_log(
            state.db_pool.as_ref(),
            instance_id,
            Some(user.id),
            "publisher_deleted",
            Some("Publisher"),
            Some(id),
            audit_details,
            serde_json::json!({}),
            ip_address.as_deref(),
            user_agent.as_deref(),
        )
        .await;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Publisher deleted successfully"
    })))
}

async fn regenerate_publisher_api_key(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(user): Extension<leadsnebula_core::models::user::User>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use leadsnebula_core::encryption::EncryptionService;
    use rand::Rng;
    use sha2::{Digest, Sha256};
    use tracing::error;

    // Verify publisher exists
    let publisher = sqlx::query_as::<_, leadsnebula_core::models::publisher::Publisher>(
        "SELECT * FROM publishers WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching publisher: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    // Generate new API key
    let api_key = format!("pk_live_{}", {
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.gen_range(0..=255)).collect();
        hex::encode(bytes)
    });

    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let api_key_hash = hex::encode(hasher.finalize());

    // Encrypt API key for storage (api_key_encrypted column is NOT NULL)
    let encryption_service = EncryptionService::new(&state.config.encryption_key).map_err(|e| {
        error!("Failed to initialize encryption service: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let api_key_encrypted = encryption_service.encrypt(&api_key).map_err(|e| {
        error!("Failed to encrypt API key: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Update publisher with new API key (both hash and encrypted)
    sqlx::query(
        "UPDATE publishers SET api_key_hash = $1, api_key_prefix = 'pk_live_', api_key_encrypted = $2, updated_at = NOW() WHERE id = $3 AND deleted_at IS NULL"
    )
    .bind(&api_key_hash)
    .bind(&api_key_encrypted)
    .bind(id)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error updating API key: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    {
        let instance_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(user.id)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .ok()
        .flatten();
        let ip_address = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
        let user_agent = headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let request_id = headers
            .get("x-request-id")
            .or_else(|| headers.get("x-correlation-id"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let audit_details = serde_json::json!({
            "action": "api_key_rotated",
            "target_type": "Publisher",
            "target_id": id.to_string(),
            "target_name": publisher.name,
            "context": {
                "reason": "User regenerated API key via dashboard",
                "request_id": request_id,
                "ip_address": ip_address,
                "user_agent": user_agent,
                "method": "POST",
                "endpoint": format!("/api/v1/dashboard/publishers/{}/regenerate-api-key", id),
                "source": "dashboard_web_ui"
            },
            "compliance": { "standard": "ISO_27001_SOC2_NIST", "version": "2024" },
            "outcome": "success",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let _ = create_audit_log(
            state.db_pool.as_ref(),
            instance_id,
            Some(user.id),
            "publisher_api_key_rotated",
            Some("Publisher"),
            Some(id),
            audit_details,
            serde_json::json!({}),
            ip_address.as_deref(),
            user_agent.as_deref(),
        )
        .await;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "api_key": api_key,
        "message": "API key regenerated successfully"
    })))
}

async fn get_publisher_api_key(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    request: Request,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use leadsnebula_core::encryption::EncryptionService;
    use tracing::error;

    // Get user from request extensions (set by jwt_auth_middleware)
    let user = get_user_from_request(&request).ok_or(StatusCode::UNAUTHORIZED)?;

    // Get publisher to check instance_id
    let publisher = sqlx::query_as::<_, leadsnebula_core::models::publisher::Publisher>(
        "SELECT * FROM publishers WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching publisher: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    // Check if user is admin (for the publisher's instance)
    let is_admin = is_user_admin(state.db_pool.as_ref(), user.id, Some(publisher.instance_id))
        .await
        .map_err(|e| {
            error!("Database error checking admin status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if !is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    // Decrypt API key

    // After migration, api_key_encrypted is NOT NULL, but may be empty string for old records
    let api_key_encrypted = match &publisher.api_key_encrypted {
        Some(encrypted) if !encrypted.is_empty() => encrypted.clone(),
        _ => {
            // API key was not encrypted (created before encryption was enabled or encryption key wasn't configured)
            // Return a helpful error message indicating the key needs to be regenerated
            return Ok(Json(serde_json::json!({
                "success": false,
                "error": "API key was not encrypted when created. Please regenerate the API key to enable encryption and retrieval.",
                "requires_regeneration": true
            })));
        }
    };

    // Decrypt the API key
    let encryption_service = EncryptionService::new(&state.config.encryption_key).map_err(|e| {
        error!("Failed to initialize encryption service: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let api_key = encryption_service
        .decrypt(&api_key_encrypted)
        .map_err(|e| {
            error!("Failed to decrypt API key: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "api_key": api_key
    })))
}

async fn generate_publisher_hmac_secret_encrypted(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(user): Extension<leadsnebula_core::models::user::User>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use rand::Rng;
    use sha2::{Digest, Sha256};
    use tracing::error;

    // Verify publisher exists
    let publisher = sqlx::query_as::<_, leadsnebula_core::models::publisher::Publisher>(
        "SELECT * FROM publishers WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching publisher: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    // Generate new HMAC secret_encrypted (128 hex characters = 64 bytes)
    let hmac_secret_encrypted = {
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..64).map(|_| rng.gen_range(0..=255)).collect();
        hex::encode(bytes)
    };

    let mut hasher = Sha256::new();
    hasher.update(hmac_secret_encrypted.as_bytes());
    let hmac_secret_encrypted_hash = hex::encode(hasher.finalize());
    let hmac_secret_encrypted_prefix = hmac_secret_encrypted.chars().take(20).collect::<String>();

    // Update publisher with new HMAC secret_encrypted
    sqlx::query(
        "UPDATE publishers SET hmac_secret_hash = $1, hmac_secret_prefix = $2, updated_at = NOW() WHERE id = $3 AND deleted_at IS NULL"
    )
    .bind(&hmac_secret_encrypted_hash)
    .bind(&hmac_secret_encrypted_prefix)
    .bind(id)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error updating HMAC secret_encrypted: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    {
        let instance_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(user.id)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .ok()
        .flatten();
        let ip_address = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
        let user_agent = headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let request_id = headers
            .get("x-request-id")
            .or_else(|| headers.get("x-correlation-id"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let audit_details = serde_json::json!({
            "action": "hmac_secret_generated",
            "target_type": "Publisher",
            "target_id": id.to_string(),
            "target_name": publisher.name,
            "context": {
                "reason": "User generated HMAC secret via dashboard",
                "request_id": request_id,
                "ip_address": ip_address,
                "user_agent": user_agent,
                "method": "POST",
                "endpoint": format!("/api/v1/dashboard/publishers/{}/generate-hmac-secret_encrypted", id),
                "source": "dashboard_web_ui"
            },
            "compliance": { "standard": "ISO_27001_SOC2_NIST", "version": "2024" },
            "outcome": "success",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let _ = create_audit_log(
            state.db_pool.as_ref(),
            instance_id,
            Some(user.id),
            "publisher_hmac_secret_generated",
            Some("Publisher"),
            Some(id),
            audit_details,
            serde_json::json!({}),
            ip_address.as_deref(),
            user_agent.as_deref(),
        )
        .await;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "hmac_secret_encrypted": hmac_secret_encrypted,
        "message": "HMAC secret_encrypted generated successfully"
    })))
}

// Buyers API
async fn list_buyers(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::{error, info};
    let user = get_user_from_request(&request).ok_or(StatusCode::UNAUTHORIZED)?;
    let instance_id = get_instance_id_for_user(state.db_pool.as_ref(), user.id)
        .await
        .map_err(|e| {
            error!("Database error getting instance for user: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::BAD_REQUEST)?;
    let active_buyers = sqlx::query_as::<_, leadsnebula_core::models::buyer::Buyer>(
        "SELECT * FROM buyers WHERE instance_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
    )
    .bind(instance_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing active buyers: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let deleted_buyers = sqlx::query_as::<_, leadsnebula_core::models::buyer::Buyer>(
        "SELECT * FROM buyers WHERE instance_id = $1 AND deleted_at IS NOT NULL ORDER BY deleted_at DESC",
    )
    .bind(instance_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing deleted buyers: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    info!(
        "Found {} active buyers, {} deleted buyers",
        active_buyers.len(),
        deleted_buyers.len()
    );

    // Load vertical info for all buyers
    use sqlx::Row;
    let active_response: Vec<serde_json::Value> = active_buyers
        .iter()
        .map(|b| {
            let mut buyer_json = serde_json::json!({
                "id": b.id.to_string(),
                "name": b.name,
                "status": b.status,
                "created_at": b.created_at.to_rfc3339(),
                "post_type": b.post_type,
                "vertical_id": b.vertical_id.map(|v| v.to_string())
            });

            // Load vertical info if vertical_id exists
            if let Some(vertical_id) = b.vertical_id {
                // We'll load this in a separate query for efficiency
                // For now, just include the ID - frontend can load details if needed
                buyer_json["vertical"] = serde_json::json!({
                    "id": vertical_id.to_string()
                });
            }

            buyer_json
        })
        .collect();

    // Load all verticals in one query
    let verticals_map: std::collections::HashMap<Uuid, serde_json::Value> =
        sqlx::query("SELECT id, name, slug FROM verticals")
            .fetch_all(state.db_pool.as_ref())
            .await
            .ok()
            .map(|rows| {
                rows.iter()
                    .map(|row| {
                        let id: Uuid = row.try_get("id").unwrap_or(Uuid::nil());
                        let name: String = row.try_get("name").unwrap_or_default();
                        let slug: String = row.try_get("slug").unwrap_or_default();
                        (
                            id,
                            serde_json::json!({
                                "id": id.to_string(),
                                "name": name,
                                "slug": slug
                            }),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

    // Process deleted buyers similarly
    let deleted_response: Vec<serde_json::Value> = deleted_buyers
        .iter()
        .map(|b| {
            let mut buyer_json = serde_json::json!({
                "id": b.id.to_string(),
                "name": b.name,
                "status": b.status,
                "created_at": b.created_at.to_rfc3339(),
                "deleted_at": b.deleted_at.map(|d| d.to_rfc3339()),
                "post_type": b.post_type,
                "vertical_id": b.vertical_id.map(|v| v.to_string())
            });

            if let Some(vertical_id) = b.vertical_id {
                buyer_json["vertical"] = serde_json::json!({
                    "id": vertical_id.to_string()
                });
            }

            buyer_json
        })
        .collect();

    // Enrich active buyer response with vertical info
    let active_response_enriched: Vec<serde_json::Value> = active_response
        .into_iter()
        .map(|mut buyer_json| {
            if let Some(vertical_id_str) = buyer_json.get("vertical_id").and_then(|v| v.as_str()) {
                if let Ok(vertical_id) = Uuid::parse_str(vertical_id_str) {
                    if let Some(vertical_info) = verticals_map.get(&vertical_id) {
                        buyer_json["vertical"] = vertical_info.clone();
                    }
                }
            }
            buyer_json
        })
        .collect();

    // Enrich deleted buyer response with vertical info
    let deleted_response_enriched: Vec<serde_json::Value> = deleted_response
        .into_iter()
        .map(|mut buyer_json| {
            if let Some(vertical_id_str) = buyer_json.get("vertical_id").and_then(|v| v.as_str()) {
                if let Ok(vertical_id) = Uuid::parse_str(vertical_id_str) {
                    if let Some(vertical_info) = verticals_map.get(&vertical_id) {
                        buyer_json["vertical"] = vertical_info.clone();
                    }
                }
            }
            buyer_json
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "buyers": active_response_enriched,
        "deleted_buyers": deleted_response_enriched
    })))
}

#[derive(Deserialize)]
pub struct CreateBuyerRequest {
    pub name: String,
    #[serde(default)]
    pub instance_id: Option<Uuid>,
    #[serde(default)]
    pub post_type: Option<String>,
    #[serde(default)]
    pub vertical_id: Option<Uuid>,
    #[serde(default)]
    pub buyer_type: Option<String>,
    #[serde(default)]
    pub email_address: Option<String>,
    #[serde(default)]
    pub ein_tin: Option<String>,
    #[serde(default)]
    pub address_street: Option<String>,
    #[serde(default)]
    pub address_city: Option<String>,
    #[serde(default)]
    pub address_state: Option<String>,
    #[serde(default)]
    pub address_zip: Option<String>,
    #[serde(default)]
    pub representative_first_name: Option<String>,
    #[serde(default)]
    pub representative_last_name: Option<String>,
}

async fn create_buyer(
    State(state): State<AppState>,
    Extension(user): Extension<leadsnebula_core::models::user::User>,
    Json(payload): Json<CreateBuyerRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let instance_id = if let Some(id) = payload.instance_id {
        id
    } else {
        get_instance_id_for_user(state.db_pool.as_ref(), user.id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::BAD_REQUEST)?
    };

    // Validate post_type
    let post_type = payload.post_type.as_deref().unwrap_or("full_post");
    if post_type != "full_post" && post_type != "ping_post" {
        return Ok(Json(serde_json::json!({
            "success": false,
            "error": format!("Invalid post_type: {}. Must be 'full_post' or 'ping_post'", post_type)
        })));
    }

    // Validate buyer_type - it's optional, but if provided must be valid
    let buyer_type = payload.buyer_type.as_deref();
    if let Some(bt) = buyer_type {
        if bt != "internal" && bt != "external" {
            return Ok(Json(serde_json::json!({
                "success": false,
                "error": format!("Invalid buyer_type: {}. Must be 'internal' or 'external'", bt)
            })));
        }
    }

    // Validate vertical_id - it's optional but recommended
    if payload.vertical_id.is_none() {
        // Allow creation without vertical_id, but it should be set later
    }

    // Check for duplicate active buyer names (only allow duplicates if existing buyer is deleted)
    let existing_buyer = sqlx::query(
        "SELECT id, name, deleted_at FROM buyers WHERE instance_id = $1 AND name = $2 AND deleted_at IS NULL LIMIT 1"
    )
    .bind(instance_id)
    .bind(&payload.name)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        use tracing::error;
        error!("Database error checking for duplicate buyer name: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if existing_buyer.is_some() {
        return Ok(Json(serde_json::json!({
            "success": false,
            "error": format!("A buyer with the name '{}' already exists. Please choose a different name.", payload.name)
        })));
    }

    let buyer_id = Uuid::new_v4();

    let insert_result = sqlx::query(
        r#"
        INSERT INTO buyers (
            id, name, instance_id, status, post_type, vertical_id, buyer_type,
            email_address, ein_tin, address_street, address_city, address_state, address_zip,
            representative_first_name, representative_last_name,
            created_at, updated_at
        ) VALUES (
            $1, $2, $3, 'incomplete', $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, NOW(), NOW()
        )
        "#,
    )
    .bind(buyer_id)
    .bind(&payload.name)
    .bind(instance_id)
    .bind(post_type)
    .bind(payload.vertical_id)
    .bind(buyer_type)
    .bind(payload.email_address.as_ref())
    .bind(payload.ein_tin.as_ref())
    .bind(payload.address_street.as_ref())
    .bind(payload.address_city.as_ref())
    .bind(payload.address_state.as_ref())
    .bind(payload.address_zip.as_ref())
    .bind(payload.representative_first_name.as_ref())
    .bind(payload.representative_last_name.as_ref())
    .execute(state.db_pool.as_ref())
    .await;

    match insert_result {
        Ok(_) => {}
        Err(e) => {
            // Check if it's a unique constraint violation - duplicate names should be allowed
            let error_str = e.to_string();
            if error_str.contains("duplicate key")
                && (error_str.contains("instance_id_and_name")
                    || error_str.contains("idx_buyers_instance_name"))
            {
                // This shouldn't happen if the constraint was removed, but handle it gracefully
                use tracing::warn;
                warn!("Duplicate buyer name constraint violation (constraint should be removed): {}. Error: {}", payload.name, error_str);
                // Return a user-friendly error message
                // Note: The constraint has been removed from the database, but the server connection pool may be stale
                // Restart the server to refresh the connection pool
                return Ok(Json(serde_json::json!({
                    "success": false,
                    "error": format!("A buyer with the name '{}' already exists. Please restart the API server to refresh the database connection pool (the constraint has been removed from the database).", payload.name)
                })));
            }

            // Log other database errors
            use tracing::error;
            error!("Database error creating buyer: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // If buyer_type is internal and vertical_id is set, auto-select Pulsar integration
    if buyer_type == Some("internal") && payload.vertical_id.is_some() {
        // Find or create Pulsar integration for this vertical
        use sqlx::Row;
        if let Some(vertical_id) = payload.vertical_id {
            // Get vertical slug for unique slug generation
            let vertical_slug = sqlx::query("SELECT slug, name FROM verticals WHERE id = $1")
                .bind(vertical_id)
                .fetch_optional(state.db_pool.as_ref())
                .await
                .ok()
                .flatten()
                .and_then(|row| {
                    let slug: String = row.try_get("slug").ok()?;
                    Some((
                        slug,
                        row.try_get::<String, _>("name").ok().unwrap_or_default(),
                    ))
                });

            if let Some((v_slug, v_name)) = vertical_slug {
                // First try to find existing Pulsar integration for this vertical
                // Since slug is unique, we'll search by vertical_id and is_internal=true
                let pulsar_id = sqlx::query("SELECT id FROM buyer_integrations WHERE vertical_id = $1 AND is_internal = true AND status = 'available' LIMIT 1")
                    .bind(vertical_id)
                    .fetch_optional(state.db_pool.as_ref())
                    .await
                    .ok()
                    .flatten()
                    .and_then(|row| row.try_get::<Uuid, _>("id").ok());

                // If not found, create it with name matching vertical (e.g., "Solar")
                let pulsar_id = if let Some(id) = pulsar_id {
                    id
                } else {
                    let new_id = Uuid::new_v4();
                    // Use vertical name as integration name (e.g., "Solar")
                    let integration_name = v_name.clone();
                    // Create unique slug: pulsar-{vertical-slug}
                    let integration_slug = format!("pulsar-{}", v_slug);

                    sqlx::query(
                        r#"
                        INSERT INTO buyer_integrations (id, name, slug, vertical_id, description, is_internal, status, default_timeout, created_at, updated_at)
                        VALUES ($1, $2, $3, $4, 'Internal Pulsar qualification engine', true, 'available', 1.2, NOW(), NOW())
                        RETURNING id
                        "#
                    )
                    .bind(new_id)
                    .bind(&integration_name)
                    .bind(&integration_slug)
                    .bind(vertical_id)
                    .fetch_optional(state.db_pool.as_ref())
                    .await
                    .ok()
                    .flatten()
                    .and_then(|row| row.try_get::<Uuid, _>("id").ok())
                    .unwrap_or(new_id)
                };

                // Update buyer with Pulsar integration
                let _ = sqlx::query("UPDATE buyers SET buyer_integration_id = $1 WHERE id = $2")
                    .bind(pulsar_id)
                    .bind(buyer_id)
                    .execute(state.db_pool.as_ref())
                    .await;
            }
        }
    }

    // Reload buyer to get all fields including buyer_integration_id if auto-selected
    let created_buyer = sqlx::query_as::<_, leadsnebula_core::models::buyer::Buyer>(
        "SELECT * FROM buyers WHERE id = $1",
    )
    .bind(buyer_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .ok()
    .flatten();

    // Log audit for buyer creation - include all created fields in "after"
    let _ = create_audit_log(
        state.db_pool.as_ref(),
        Some(instance_id),
        None, // TODO: Extract user_id from request extensions
        "buyer_created",
        Some("Buyer"),
        Some(buyer_id),
        serde_json::json!({
            "action": "create",
            "after": {
                "name": payload.name,
                "buyer_type": buyer_type,
                "vertical_id": payload.vertical_id.map(|v| v.to_string()),
                "post_type": post_type,
                "email_address": payload.email_address,
                "ein_tin": payload.ein_tin,
                "address_street": payload.address_street,
                "address_city": payload.address_city,
                "address_state": payload.address_state,
                "address_zip": payload.address_zip,
                "representative_first_name": payload.representative_first_name,
                "representative_last_name": payload.representative_last_name,
            }
        }),
        serde_json::json!({}),
        None, // TODO: Extract IP address from request
        None, // TODO: Extract user agent from request
    )
    .await;

    Ok(Json(serde_json::json!({
        "success": true,
        "buyer": {
            "id": buyer_id.to_string(),
            "name": payload.name,
            "status": created_buyer.as_ref().map(|b| b.status.as_str()).unwrap_or("incomplete"),
            "buyer_type": buyer_type,
            "post_type": post_type,
            "vertical_id": payload.vertical_id.map(|v| v.to_string()),
            "buyer_integration_id": created_buyer.as_ref().and_then(|b| b.buyer_integration_id.map(|v| v.to_string()))
        },
        "message": "Buyer created successfully"
    })))
}

async fn get_buyer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Allow viewing deleted buyers (remove deleted_at filter)
    let buyer = sqlx::query_as::<_, leadsnebula_core::models::buyer::Buyer>(
        "SELECT * FROM buyers WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    // Load vertical info if vertical_id exists
    let vertical_info = if let Some(vertical_id) = buyer.vertical_id {
        sqlx::query_as::<_, leadsnebula_core::models::vertical::Vertical>(
            "SELECT * FROM verticals WHERE id = $1",
        )
        .bind(vertical_id)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .ok()
        .flatten()
        .map(|v| {
            serde_json::json!({
                "id": v.id.to_string(),
                "name": v.name,
                "slug": v.slug
            })
        })
    } else {
        None
    };

    // Load integration info if buyer_integration_id exists
    let integration_info = if let Some(integration_id) = buyer.buyer_integration_id {
        sqlx::query("SELECT id, name, slug FROM buyer_integrations WHERE id = $1")
            .bind(integration_id)
            .fetch_optional(state.db_pool.as_ref())
            .await
            .ok()
            .flatten()
            .map(|row: sqlx::postgres::PgRow| {
                use sqlx::Row;
                serde_json::json!({
                    "id": row.try_get::<Uuid, _>("id").unwrap_or(Uuid::nil()).to_string(),
                    "name": row.try_get::<String, _>("name").unwrap_or_default(),
                    "slug": row.try_get::<String, _>("slug").unwrap_or_default()
                })
            })
    } else {
        None
    };

    // Check if buyer can be activated (all general info filled + integration selected)
    let can_activate = !buyer.name.is_empty()
        && buyer.email_address.is_some()
        && buyer.ein_tin.is_some()
        && buyer.address_street.is_some()
        && buyer.address_city.is_some()
        && buyer.address_state.is_some()
        && buyer.address_zip.is_some()
        && buyer.representative_first_name.is_some()
        && buyer.representative_last_name.is_some()
        && buyer.buyer_integration_id.is_some();

    Ok(Json(serde_json::json!({
        "success": true,
        "buyer": {
            "id": buyer.id.to_string(),
            "name": buyer.name,
            "status": buyer.status.as_str(),
            "created_at": buyer.created_at.to_rfc3339(),
            "deleted_at": buyer.deleted_at.map(|d| d.to_rfc3339()),
            "buyer_type": buyer.buyer_type,
            "vertical_id": buyer.vertical_id.map(|v| v.to_string()),
            "vertical": vertical_info,
            "buyer_integration_id": buyer.buyer_integration_id.map(|v| v.to_string()),
            "integration": integration_info,
            "post_type": buyer.post_type,
            "email_address": buyer.email_address,
            "ein_tin": buyer.ein_tin,
            "address_street": buyer.address_street,
            "address_city": buyer.address_city,
            "address_state": buyer.address_state,
            "address_zip": buyer.address_zip,
            "representative_first_name": buyer.representative_first_name,
            "representative_last_name": buyer.representative_last_name,
            "can_activate": can_activate
        }
    })))
}

async fn update_buyer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: axum::http::HeaderMap,
    axum::extract::Extension(user): axum::extract::Extension<leadsnebula_core::models::user::User>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Check if buyer is deleted - prevent updates to deleted buyers
    let buyer_check = sqlx::query("SELECT deleted_at FROM buyers WHERE id = $1")
        .bind(id)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(row) = buyer_check {
        use sqlx::Row;
        if let Ok(Some(_deleted_at)) =
            row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("deleted_at")
        {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    // Get buyer before update for audit log
    let buyer_before = sqlx::query_as::<_, leadsnebula_core::models::buyer::Buyer>(
        "SELECT * FROM buyers WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        tracing::error!(
            "[INVESTIGATION] Database error fetching buyer {}: {}",
            id,
            e
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Build update query dynamically like update_publisher
    let mut query_builder = sqlx::QueryBuilder::new("UPDATE buyers SET ");
    let mut has_updates = false;
    let mut changed_fields = serde_json::Map::new();

    if let Some(name) = payload.get("name").and_then(|v| v.as_str()) {
        if let Some(ref buyer) = buyer_before {
            if buyer.name != name {
                changed_fields.insert(
                    "name".to_string(),
                    serde_json::json!({
                        "before": buyer.name,
                        "after": name
                    }),
                );
            }
        }
        query_builder.push("name = ");
        query_builder.push_bind(name);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(email) = payload.get("email_address").and_then(|v| v.as_str()) {
        if let Some(ref buyer) = buyer_before {
            let before_email = buyer.email_address.as_deref().unwrap_or("");
            if before_email != email {
                changed_fields.insert(
                    "email_address".to_string(),
                    serde_json::json!({
                        "before": before_email,
                        "after": email
                    }),
                );
            }
        }
        query_builder.push("email_address = ");
        query_builder.push_bind(email);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(ein_tin) = payload.get("ein_tin").and_then(|v| v.as_str()) {
        if let Some(ref buyer) = buyer_before {
            let before_ein_tin = buyer.ein_tin.as_deref().unwrap_or("");
            if before_ein_tin != ein_tin {
                changed_fields.insert(
                    "ein_tin".to_string(),
                    serde_json::json!({
                        "before": before_ein_tin,
                        "after": ein_tin
                    }),
                );
            }
        }
        query_builder.push("ein_tin = ");
        query_builder.push_bind(ein_tin);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(addr_street) = payload.get("address_street").and_then(|v| v.as_str()) {
        if let Some(ref buyer) = buyer_before {
            let before_addr_street = buyer.address_street.as_deref().unwrap_or("");
            if before_addr_street != addr_street {
                changed_fields.insert(
                    "address_street".to_string(),
                    serde_json::json!({
                        "before": before_addr_street,
                        "after": addr_street
                    }),
                );
            }
        }
        query_builder.push("address_street = ");
        query_builder.push_bind(addr_street);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(addr_city) = payload.get("address_city").and_then(|v| v.as_str()) {
        if let Some(ref buyer) = buyer_before {
            let before_addr_city = buyer.address_city.as_deref().unwrap_or("");
            if before_addr_city != addr_city {
                changed_fields.insert(
                    "address_city".to_string(),
                    serde_json::json!({
                        "before": before_addr_city,
                        "after": addr_city
                    }),
                );
            }
        }
        query_builder.push("address_city = ");
        query_builder.push_bind(addr_city);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(addr_state) = payload.get("address_state").and_then(|v| v.as_str()) {
        if let Some(ref buyer) = buyer_before {
            let before_addr_state = buyer.address_state.as_deref().unwrap_or("");
            if before_addr_state != addr_state {
                changed_fields.insert(
                    "address_state".to_string(),
                    serde_json::json!({
                        "before": before_addr_state,
                        "after": addr_state
                    }),
                );
            }
        }
        query_builder.push("address_state = ");
        query_builder.push_bind(addr_state);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(addr_zip) = payload.get("address_zip").and_then(|v| v.as_str()) {
        if let Some(ref buyer) = buyer_before {
            let before_addr_zip = buyer.address_zip.as_deref().unwrap_or("");
            if before_addr_zip != addr_zip {
                changed_fields.insert(
                    "address_zip".to_string(),
                    serde_json::json!({
                        "before": before_addr_zip,
                        "after": addr_zip
                    }),
                );
            }
        }
        query_builder.push("address_zip = ");
        query_builder.push_bind(addr_zip);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(rep_first) = payload
        .get("representative_first_name")
        .and_then(|v| v.as_str())
    {
        if let Some(ref buyer) = buyer_before {
            let before_rep_first = buyer.representative_first_name.as_deref().unwrap_or("");
            if before_rep_first != rep_first {
                changed_fields.insert(
                    "representative_first_name".to_string(),
                    serde_json::json!({
                        "before": before_rep_first,
                        "after": rep_first
                    }),
                );
            }
        }
        query_builder.push("representative_first_name = ");
        query_builder.push_bind(rep_first);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(rep_last) = payload
        .get("representative_last_name")
        .and_then(|v| v.as_str())
    {
        if let Some(ref buyer) = buyer_before {
            let before_rep_last = buyer.representative_last_name.as_deref().unwrap_or("");
            if before_rep_last != rep_last {
                changed_fields.insert(
                    "representative_last_name".to_string(),
                    serde_json::json!({
                        "before": before_rep_last,
                        "after": rep_last
                    }),
                );
            }
        }
        query_builder.push("representative_last_name = ");
        query_builder.push_bind(rep_last);
        query_builder.push(", ");
        has_updates = true;
    }
    if let Some(buyer_integration_id) = payload.get("buyer_integration_id") {
        if buyer_integration_id.is_null()
            || (buyer_integration_id.is_string()
                && buyer_integration_id.as_str().unwrap_or("").is_empty())
        {
            query_builder.push("buyer_integration_id = NULL, ");
            has_updates = true;
        } else if let Some(id_str) = buyer_integration_id.as_str() {
            if !id_str.is_empty() {
                if let Ok(id) = Uuid::parse_str(id_str) {
                    query_builder.push("buyer_integration_id = ");
                    query_builder.push_bind(id);
                    query_builder.push(", ");
                    has_updates = true;
                }
            }
        }
    }
    if let Some(status_str) = payload.get("status").and_then(|v| v.as_str()) {
        // Parse string to BuyerStatus enum for proper type binding
        use leadsnebula_core::models::enums::BuyerStatus;
        let status_enum = match status_str {
            "active" => BuyerStatus::Active,
            "incomplete" => BuyerStatus::Incomplete,
            "inactive" => BuyerStatus::Inactive,
            "suspended" => BuyerStatus::Suspended,
            _ => {
                tracing::warn!("Invalid buyer status value: {}", status_str);
                return Ok(Json(serde_json::json!({
                    "success": false,
                    "error": format!("Invalid status value: {}. Must be one of: active, incomplete, inactive, suspended", status_str)
                })));
            }
        };

        if let Some(ref buyer) = buyer_before {
            if buyer.status.as_str() != status_str {
                changed_fields.insert(
                    "status".to_string(),
                    serde_json::json!({
                        "before": buyer.status.as_str(),
                        "after": status_str
                    }),
                );
            }
        }
        query_builder.push("status = ");
        query_builder.push_bind(status_enum);
        query_builder.push(", ");
        has_updates = true;
    }

    if !has_updates {
        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "Buyer updated successfully"
        })));
    }

    // Add updated_at and WHERE clause
    query_builder.push(" updated_at = NOW() WHERE id = ");
    query_builder.push_bind(id);
    query_builder.push(" AND deleted_at IS NULL");

    let query = query_builder.build();

    let update_result = query.execute(state.db_pool.as_ref()).await;

    update_result.map_err(|e| {
        tracing::error!(
            "[INVESTIGATION] Database error updating buyer {}: {}",
            id,
            e
        );
        tracing::error!("[INVESTIGATION] Error details: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Log audit for buyer update - log even if changed_fields is empty (to track all updates)
    if has_updates {
        let instance_id = buyer_before.as_ref().map(|b| b.instance_id);

        // Determine action type based on what changed
        let action_type = if changed_fields.contains_key("status") {
            let new_status = changed_fields
                .get("status")
                .and_then(|v| v.get("after").and_then(|a| a.as_str()))
                .unwrap_or("");
            if new_status == "active" {
                "buyer_activated"
            } else if new_status == "suspended" {
                "buyer_deactivated"
            } else {
                "buyer_updated"
            }
        } else {
            "buyer_updated"
        };

        // Build full audit log with all data points (not just changed ones) - matching Ruby Security page format
        // Build buyer_before_full from the buyer_before object
        let buyer_before_full = buyer_before.as_ref().map(|b| {
            let mut json = serde_json::Map::new();
            json.insert("name".to_string(), serde_json::json!(b.name));
            json.insert(
                "email_address".to_string(),
                serde_json::json!(b.email_address),
            );
            json.insert("ein_tin".to_string(), serde_json::json!(b.ein_tin));
            json.insert(
                "address_street".to_string(),
                serde_json::json!(b.address_street),
            );
            json.insert(
                "address_city".to_string(),
                serde_json::json!(b.address_city),
            );
            json.insert(
                "address_state".to_string(),
                serde_json::json!(b.address_state),
            );
            json.insert("address_zip".to_string(), serde_json::json!(b.address_zip));
            json.insert(
                "representative_first_name".to_string(),
                serde_json::json!(b.representative_first_name),
            );
            json.insert(
                "representative_last_name".to_string(),
                serde_json::json!(b.representative_last_name),
            );
            json.insert("status".to_string(), serde_json::json!(b.status));
            json.insert("post_type".to_string(), serde_json::json!(b.post_type));
            json.insert("buyer_type".to_string(), serde_json::json!(b.buyer_type));
            json.insert(
                "vertical_id".to_string(),
                serde_json::json!(b.vertical_id.map(|v| v.to_string())),
            );
            json.insert(
                "buyer_integration_id".to_string(),
                serde_json::json!(b.buyer_integration_id.map(|v| v.to_string())),
            );
            serde_json::Value::Object(json)
        });

        // Build buyer_after_full from buyer_before + changed_fields (more reliable than fetching from DB)
        let buyer_after_full = buyer_before.as_ref().map(|b| {
            let mut json = serde_json::Map::new();
            // Start with all fields from buyer_before
            json.insert("name".to_string(), serde_json::json!(b.name));
            json.insert(
                "email_address".to_string(),
                serde_json::json!(b.email_address),
            );
            json.insert("ein_tin".to_string(), serde_json::json!(b.ein_tin));
            json.insert(
                "address_street".to_string(),
                serde_json::json!(b.address_street),
            );
            json.insert(
                "address_city".to_string(),
                serde_json::json!(b.address_city),
            );
            json.insert(
                "address_state".to_string(),
                serde_json::json!(b.address_state),
            );
            json.insert("address_zip".to_string(), serde_json::json!(b.address_zip));
            json.insert(
                "representative_first_name".to_string(),
                serde_json::json!(b.representative_first_name),
            );
            json.insert(
                "representative_last_name".to_string(),
                serde_json::json!(b.representative_last_name),
            );
            json.insert("status".to_string(), serde_json::json!(b.status));
            json.insert("post_type".to_string(), serde_json::json!(b.post_type));
            json.insert("buyer_type".to_string(), serde_json::json!(b.buyer_type));
            json.insert(
                "vertical_id".to_string(),
                serde_json::json!(b.vertical_id.map(|v| v.to_string())),
            );
            json.insert(
                "buyer_integration_id".to_string(),
                serde_json::json!(b.buyer_integration_id.map(|v| v.to_string())),
            );

            // Override with changed values from changed_fields
            for (key, change_obj) in &changed_fields {
                if let Some(after_val) = change_obj.get("after") {
                    match key.as_str() {
                        "name" => json.insert("name".to_string(), after_val.clone()),
                        "email_address" => {
                            json.insert("email_address".to_string(), after_val.clone())
                        }
                        "ein_tin" => json.insert("ein_tin".to_string(), after_val.clone()),
                        "address_street" => {
                            json.insert("address_street".to_string(), after_val.clone())
                        }
                        "address_city" => {
                            json.insert("address_city".to_string(), after_val.clone())
                        }
                        "address_state" => {
                            json.insert("address_state".to_string(), after_val.clone())
                        }
                        "address_zip" => json.insert("address_zip".to_string(), after_val.clone()),
                        "representative_first_name" => {
                            json.insert("representative_first_name".to_string(), after_val.clone())
                        }
                        "representative_last_name" => {
                            json.insert("representative_last_name".to_string(), after_val.clone())
                        }
                        "status" => json.insert("status".to_string(), after_val.clone()),
                        "post_type" => json.insert("post_type".to_string(), after_val.clone()),
                        "buyer_type" => json.insert("buyer_type".to_string(), after_val.clone()),
                        "vertical_id" => json.insert("vertical_id".to_string(), after_val.clone()),
                        "buyer_integration_id" => {
                            json.insert("buyer_integration_id".to_string(), after_val.clone())
                        }
                        _ => None,
                    };
                }
            }

            serde_json::Value::Object(json)
        });

        // Extract compliance-required information from headers (ISO 27001, SOC 2, NIST)
        let ip_address = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());

        let user_agent = headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        // Extract request ID from headers (X-Request-ID, X-Correlation-ID, or generate)
        let request_id = headers
            .get("x-request-id")
            .or_else(|| headers.get("x-correlation-id"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Extract session ID if available
        let session_id = headers
            .get("x-session-id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        // Extract referer for context
        let referer = headers
            .get("referer")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        // Build actor information (ISO 27001 A.12.4.1: User identification required)
        let actor_name = format!(
            "{} {}",
            user.first_name.as_deref().unwrap_or(""),
            user.last_name.as_deref().unwrap_or("")
        )
        .trim()
        .to_string();
        let actor_name = if actor_name.is_empty() {
            user.email.clone()
        } else {
            actor_name
        };

        // Determine user role (TODO: Query from database)
        let user_role = "instance_admin"; // TODO: Determine actual role from user_roles table

        // Build full audit log details matching compliance standards (ISO 27001, SOC 2, NIST)
        // ISO 27001 A.12.4.1 requires: user identification, type of event, date/time, success/failure, source
        // SOC 2 CC6.6 requires: who, what, when, where, why, result
        // NIST/OWASP best practices: actor, action, target, context, outcome, timestamp
        let timestamp = chrono::Utc::now();
        let audit_details = serde_json::json!({
            "action": "update",
            "target_type": "Buyer",
            "target_id": id.to_string(),
            "target_name": buyer_after_full.as_ref().and_then(|b| b.get("name").and_then(|n| n.as_str())).unwrap_or(""),
            "actor": {
                "id": user.id.to_string(),
                "name": actor_name,
                "email": user.email,
                "role": user_role,
                "instance_id": instance_id.map(|id| id.to_string())
            },
            "changes": changed_fields,
            "before": buyer_before_full,
            "after": buyer_after_full,
            "context": {
                "reason": "User updated buyer via dashboard",
                "request_id": request_id,
                "session_id": session_id,
                "ip_address": ip_address,
                "user_agent": user_agent,
                "referer": referer,
                "method": "POST",
                "endpoint": format!("/api/v1/dashboard/buyers/{}", id),
                "source": "dashboard_web_ui"
            },
            "outcome": "success",
            "timestamp": timestamp.to_rfc3339(),
            "compliance": {
                "standard": "ISO_27001_SOC2_NIST",
                "version": "2024"
            }
        });

        let audit_result = create_audit_log(
            state.db_pool.as_ref(),
            instance_id,
            Some(user.id),
            action_type,
            Some("Buyer"),
            Some(id),
            audit_details,
            serde_json::json!({}),
            ip_address.as_deref(),
            user_agent.as_deref(),
        )
        .await;

        if let Err(e) = audit_result {
            tracing::error!(
                "[INVESTIGATION] Failed to create audit log for buyer {}: {}",
                id,
                e
            );
            // Don't fail the request if audit log fails, but log it
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Buyer updated successfully"
    })))
}

async fn delete_buyer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Get buyer info before deletion for audit log
    let buyer_before = sqlx::query_as::<_, leadsnebula_core::models::buyer::Buyer>(
        "SELECT * FROM buyers WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching buyer: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let buyer_before = match buyer_before {
        Some(b) => b,
        None => return Err(StatusCode::NOT_FOUND),
    };

    // Get affected campaigns for audit log
    let affected_campaigns: Vec<serde_json::Value> =
        sqlx::query("SELECT id, name, campaign_token FROM campaigns WHERE buyer_id = $1")
            .bind(id)
            .fetch_all(state.db_pool.as_ref())
            .await
            .ok()
            .unwrap_or_default()
            .iter()
            .filter_map(|row| {
                use sqlx::Row;
                let id: Option<Uuid> = row.try_get("id").ok();
                let name: Option<String> = row.try_get("name").ok();
                let token: Option<String> = row.try_get("campaign_token").ok();
                id.map(|id| {
                    serde_json::json!({
                        "id": id.to_string(),
                        "name": name.unwrap_or_default(),
                        "campaign_token": token.unwrap_or_default()
                    })
                })
            })
            .collect();

    // Soft delete the buyer (set deleted_at)
    sqlx::query("UPDATE buyers SET deleted_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|e| {
            error!("Database error soft deleting buyer: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Log audit for buyer deletion
    let _ = create_audit_log(
        state.db_pool.as_ref(),
        Some(buyer_before.instance_id),
        None, // TODO: Extract user_id from request extensions
        "buyer_deleted",
        Some("Buyer"),
        Some(id),
        serde_json::json!({
            "buyer_name": buyer_before.name,
            "buyer_type": buyer_before.buyer_type,
            "post_type": buyer_before.post_type
        }),
        serde_json::json!({
            "campaigns": affected_campaigns
        }),
        None, // TODO: Extract IP address from request
        None, // TODO: Extract user agent from request
    )
    .await;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Buyer deleted successfully"
    })))
}

// Campaigns API
async fn list_campaigns(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::{error, info};
    let user = get_user_from_request(&request).ok_or(StatusCode::UNAUTHORIZED)?;
    let instance_id = get_instance_id_for_user(state.db_pool.as_ref(), user.id)
        .await
        .map_err(|e| {
            error!("Database error getting instance for user: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::BAD_REQUEST)?;

    let campaigns = sqlx::query_as::<_, leadsnebula_core::models::campaign::Campaign>(
        "SELECT * FROM campaigns WHERE instance_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
    )
    .bind(instance_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing campaigns: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    info!("Found {} campaigns", campaigns.len());

    // Get ping tree membership for all campaigns
    let campaign_ids: Vec<Uuid> = campaigns.iter().map(|c| c.id).collect();
    let ping_tree_memberships: std::collections::HashMap<Uuid, bool> = if !campaign_ids.is_empty() {
        // Query all ping tree memberships at once
        let members: std::collections::HashSet<Uuid> =
            sqlx::query_scalar::<_, Uuid>("SELECT DISTINCT campaign_id FROM ping_tree_campaigns")
                .fetch_all(state.db_pool.as_ref())
                .await
                .ok()
                .unwrap_or_default()
                .into_iter()
                .collect();

        campaign_ids
            .iter()
            .map(|id| (*id, members.contains(id)))
            .collect()
    } else {
        std::collections::HashMap::new()
    };

    let response: Vec<serde_json::Value> = campaigns
        .iter()
        .map(|c| {
            let in_ping_tree = ping_tree_memberships.get(&c.id).copied().unwrap_or(false);
            serde_json::json!({
                "id": c.id.to_string(),
                "name": c.name,
                "vertical": c.vertical,
                "campaign_token": c.campaign_token,
                "status": c.status.as_str(),
                "publisher_id": c.publisher_id.to_string(),
                "buyer_id": c.buyer_id.to_string(),
                "created_at": c.created_at.to_rfc3339(),
                "deleted_at": c.deleted_at.map(|dt| dt.to_rfc3339()),
                "in_ping_tree": in_ping_tree
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "campaigns": response
    })))
}

#[derive(Deserialize)]
pub struct CreateCampaignRequest {
    pub name: Option<String>,
    pub vertical: String,
    pub publisher_id: Uuid,
    pub buyer_id: Uuid,
    pub instance_id: Option<Uuid>,
    pub ping_tree_id: Option<Uuid>,
}

#[axum::debug_handler]
async fn create_campaign(
    State(state): State<AppState>,
    Extension(user): Extension<leadsnebula_core::models::user::User>,
    Json(payload): Json<CreateCampaignRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let instance_id = if let Some(id) = payload.instance_id {
        id
    } else {
        get_instance_id_for_user(state.db_pool.as_ref(), user.id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::BAD_REQUEST)?
    };

    // Generate campaign token
    let token_bytes: Vec<u8> = (0..20).map(|_| rand::random::<u8>()).collect();
    let campaign_token = hex::encode(token_bytes);

    let campaign_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO campaigns (
            id, buyer_id, publisher_id, instance_id, name, vertical,
            campaign_token, status, is_documentation_test, created_at, updated_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, 'active', false, NOW(), NOW()
        )
        "#,
    )
    .bind(campaign_id)
    .bind(payload.buyer_id)
    .bind(payload.publisher_id)
    .bind(instance_id)
    .bind(payload.name.as_deref())
    .bind(&payload.vertical)
    .bind(&campaign_token)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // If ping_tree_id provided, add campaign to ping tree
    if let Some(ping_tree_id) = payload.ping_tree_id {
        sqlx::query(
            r#"
            INSERT INTO ping_tree_campaigns (
                id, ping_tree_id, campaign_id, priority, enabled, created_at, updated_at
            ) VALUES (
                gen_random_uuid(), $1, $2, 1, true, NOW(), NOW()
            )
            ON CONFLICT (ping_tree_id, campaign_id) DO NOTHING
            "#,
        )
        .bind(ping_tree_id)
        .bind(campaign_id)
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "campaign": {
            "id": campaign_id.to_string(),
            "name": payload.name,
            "vertical": payload.vertical,
            "campaign_token": campaign_token,
            "status": "active",
            "publisher_id": payload.publisher_id.to_string(),
            "buyer_id": payload.buyer_id.to_string()
        },
        "message": "Campaign created successfully"
    })))
}

async fn get_campaign(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    request: Request,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sqlx::Row;
    let user = get_user_from_request(&request).ok_or(StatusCode::UNAUTHORIZED)?;
    let user_instance_id = get_instance_id_for_user(state.db_pool.as_ref(), user.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::BAD_REQUEST)?;
    let campaign = sqlx::query_as::<_, leadsnebula_core::models::campaign::Campaign>(
        "SELECT * FROM campaigns WHERE id = $1 AND instance_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user_instance_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    // Check if campaign is in any ping tree
    let in_ping_tree: bool =
        sqlx::query("SELECT COUNT(*) > 0 FROM ping_tree_campaigns WHERE campaign_id = $1")
            .bind(id)
            .fetch_one(state.db_pool.as_ref())
            .await
            .map(|row: sqlx::postgres::PgRow| row.get::<bool, _>(0))
            .unwrap_or(false);

    Ok(Json(serde_json::json!({
        "success": true,
        "campaign": {
            "id": campaign.id.to_string(),
            "name": campaign.name,
            "vertical": campaign.vertical,
            "campaign_token": campaign.campaign_token,
            "status": campaign.status.as_str(),
            "publisher_id": campaign.publisher_id.to_string(),
            "buyer_id": campaign.buyer_id.to_string(),
            "created_at": campaign.created_at.to_rfc3339(),
            "in_ping_tree": in_ping_tree
        }
    })))
}

async fn update_campaign(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Get campaign before update for audit log
    let campaign_before = sqlx::query_as::<_, leadsnebula_core::models::campaign::Campaign>(
        "SELECT * FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching campaign: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let campaign_before = match campaign_before {
        Some(c) => c,
        None => return Err(StatusCode::NOT_FOUND),
    };

    // Track changes for audit log
    let mut changed_fields = serde_json::Map::new();
    let mut has_updates = false;

    // Build update query dynamically
    let mut query_builder = sqlx::QueryBuilder::new("UPDATE campaigns SET ");

    if let Some(name) = payload.get("name").and_then(|v| v.as_str()) {
        if campaign_before.name.as_deref().unwrap_or("") != name {
            changed_fields.insert(
                "name".to_string(),
                serde_json::json!({
                    "before": campaign_before.name,
                    "after": name
                }),
            );
        }
        query_builder.push("name = ");
        query_builder.push_bind(name);
        query_builder.push(", ");
        has_updates = true;
    }

    if let Some(publisher_id_str) = payload.get("publisher_id").and_then(|v| v.as_str()) {
        if let Ok(publisher_id) = Uuid::parse_str(publisher_id_str) {
            if campaign_before.publisher_id != publisher_id {
                changed_fields.insert(
                    "publisher_id".to_string(),
                    serde_json::json!({
                        "before": campaign_before.publisher_id.to_string(),
                        "after": publisher_id.to_string()
                    }),
                );
            }
            query_builder.push("publisher_id = ");
            query_builder.push_bind(publisher_id);
            query_builder.push(", ");
            has_updates = true;
        }
    }

    if let Some(buyer_id_str) = payload.get("buyer_id").and_then(|v| v.as_str()) {
        if let Ok(buyer_id) = Uuid::parse_str(buyer_id_str) {
            if campaign_before.buyer_id != buyer_id {
                changed_fields.insert(
                    "buyer_id".to_string(),
                    serde_json::json!({
                        "before": campaign_before.buyer_id.to_string(),
                        "after": buyer_id.to_string()
                    }),
                );
            }
            query_builder.push("buyer_id = ");
            query_builder.push_bind(buyer_id);
            query_builder.push(", ");
            has_updates = true;
        }
    }

    // Handle status updates based on ping tree membership
    if let Some(status_str) = payload.get("status").and_then(|v| v.as_str()) {
        use leadsnebula_core::models::enums::CampaignStatus;
        use sqlx::Row;

        // Check if campaign is in any ping tree
        let in_ping_tree: bool =
            sqlx::query("SELECT COUNT(*) > 0 FROM ping_tree_campaigns WHERE campaign_id = $1")
                .bind(id)
                .fetch_one(state.db_pool.as_ref())
                .await
                .map(|row: sqlx::postgres::PgRow| row.get::<bool, _>(0))
                .unwrap_or(false);

        let status_enum = match status_str {
            "active" => {
                // If activating and not in ping tree, set to "inactive" (ronin)
                // If activating and in ping tree, set to "active"
                if in_ping_tree {
                    CampaignStatus::Active
                } else {
                    CampaignStatus::Inactive
                }
            }
            "paused" => CampaignStatus::Paused,
            "inactive" => CampaignStatus::Inactive,
            _ => {
                error!("Invalid campaign status value: {}", status_str);
                return Ok(Json(serde_json::json!({
                    "success": false,
                    "error": format!("Invalid status value: {}. Must be one of: active, paused, inactive", status_str)
                })));
            }
        };

        if campaign_before.status != status_enum {
            changed_fields.insert(
                "status".to_string(),
                serde_json::json!({
                    "before": campaign_before.status.as_str(),
                    "after": status_enum.as_str()
                }),
            );
            query_builder.push("status = ");
            query_builder.push_bind(status_enum);
            query_builder.push(", ");
            has_updates = true;
        }
    }

    if !has_updates {
        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "No changes to update"
        })));
    }

    query_builder.push("updated_at = NOW() WHERE id = ");
    query_builder.push_bind(id);
    query_builder.push(" AND deleted_at IS NULL");

    let update_query = query_builder.build();
    update_query
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|e| {
            error!("Database error updating campaign: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Get campaign after update for audit log
    let campaign_after = sqlx::query_as::<_, leadsnebula_core::models::campaign::Campaign>(
        "SELECT * FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .ok()
    .flatten();

    // Build before/after JSON for audit log
    let campaign_before_full = serde_json::json!({
        "id": campaign_before.id.to_string(),
        "name": campaign_before.name,
        "vertical": campaign_before.vertical,
        "campaign_token": campaign_before.campaign_token,
        "status": campaign_before.status.to_string(),
        "publisher_id": campaign_before.publisher_id.to_string(),
        "buyer_id": campaign_before.buyer_id.to_string(),
        "instance_id": campaign_before.instance_id.to_string(),
    });

    let campaign_after_full = campaign_after.as_ref().map(|c| {
        serde_json::json!({
            "id": c.id.to_string(),
            "name": c.name,
            "vertical": c.vertical,
            "campaign_token": c.campaign_token,
            "status": c.status.to_string(),
            "publisher_id": c.publisher_id.to_string(),
            "buyer_id": c.buyer_id.to_string(),
            "instance_id": c.instance_id.to_string(),
        })
    });

    // Determine action type
    let action_type = if changed_fields.contains_key("name") && changed_fields.len() == 1 {
        "campaign_name_changed"
    } else if changed_fields.contains_key("publisher_id") && changed_fields.len() == 1 {
        "campaign_publisher_changed"
    } else if changed_fields.contains_key("buyer_id") && changed_fields.len() == 1 {
        "campaign_buyer_changed"
    } else {
        "campaign_updated"
    };

    // Build audit log details following ISO/SOC format
    let timestamp = chrono::Utc::now();
    let audit_details = serde_json::json!({
        "action": "update",
        "target_type": "Campaign",
        "target_id": id.to_string(),
        "target_name": campaign_after_full.as_ref().and_then(|c| c.get("name").and_then(|n| n.as_str())).unwrap_or(""),
        "changes": changed_fields,
        "before": campaign_before_full,
        "after": campaign_after_full,
        "context": {
            "reason": "User updated campaign via dashboard",
            "method": "POST",
            "endpoint": format!("/api/v1/dashboard/campaigns/{}", id),
            "source": "dashboard_web_ui"
        },
        "outcome": "success",
        "timestamp": timestamp.to_rfc3339(),
        "compliance": {
            "standard": "ISO_27001_SOC2_NIST",
            "version": "2024"
        }
    });

    // Create audit log entry
    let _ = create_audit_log(
        state.db_pool.as_ref(),
        Some(campaign_before.instance_id),
        None, // TODO: Extract user_id from request extensions
        action_type,
        Some("Campaign"),
        Some(id),
        audit_details,
        serde_json::json!({}),
        None, // TODO: Extract IP address from request
        None, // TODO: Extract user agent from request
    )
    .await;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Campaign updated successfully"
    })))
}

async fn delete_campaign(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    sqlx::query("UPDATE campaigns SET deleted_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Campaign deleted successfully"
    })))
}

// Ping Trees API
async fn list_ping_trees(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::{error, info};
    let user = get_user_from_request(&request).ok_or(StatusCode::UNAUTHORIZED)?;
    let instance_id = get_instance_id_for_user(state.db_pool.as_ref(), user.id)
        .await
        .map_err(|e| {
            error!("Database error getting instance for user: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::BAD_REQUEST)?;
    let ping_trees = sqlx::query_as::<_, leadsnebula_core::models::ping_tree::PingTree>(
        "SELECT id, instance_id, name, vertical, strategy, status, priority, deleted_at, created_at, updated_at FROM ping_trees WHERE instance_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
    )
    .bind(instance_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing ping trees: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    info!("Found {} ping trees", ping_trees.len());

    // Get publisher counts for each ping tree
    let mut response = Vec::new();
    for pt in &ping_trees {
        let publisher_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ping_tree_publishers WHERE ping_tree_id = $1")
                .bind(pt.id)
                .fetch_one(state.db_pool.as_ref())
                .await
                .unwrap_or(0);

        response.push(serde_json::json!({
            "id": pt.id.to_string(),
            "name": pt.name,
            "vertical": pt.vertical,
            "strategy": pt.strategy,
            "status": pt.status,
            "publisher_count": publisher_count,
            "priority": pt.priority,
            "created_at": pt.created_at.to_rfc3339(),
            "deleted_at": pt.deleted_at.map(|dt| dt.to_rfc3339())
        }));
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "ping_trees": response
    })))
}

#[derive(Deserialize)]
pub struct CreatePingTreeRequest {
    pub name: String,
    pub vertical: String,
    pub strategy: String,
    pub instance_id: Option<Uuid>,
    pub priority: Option<i32>,
}

async fn create_ping_tree(
    State(state): State<AppState>,
    Extension(user): Extension<leadsnebula_core::models::user::User>,
    Json(payload): Json<CreatePingTreeRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Validate strategy
    if payload.strategy != "ping_post" && payload.strategy != "fullpost" {
        return Err(StatusCode::BAD_REQUEST);
    }

    let instance_id = if let Some(id) = payload.instance_id {
        id
    } else {
        get_instance_id_for_user(state.db_pool.as_ref(), user.id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::BAD_REQUEST)?
    };

    let ping_tree_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO ping_trees (
            id, instance_id, name, vertical, strategy, status,
            priority, created_at, updated_at
        ) VALUES (
            $1, $2, $3, $4, $5, 'active',
            $6, NOW(), NOW()
        )
        "#,
    )
    .bind(ping_tree_id)
    .bind(instance_id)
    .bind(&payload.name)
    .bind(&payload.vertical)
    .bind(&payload.strategy)
    .bind(payload.priority)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "success": true,
        "ping_tree": {
            "id": ping_tree_id.to_string(),
            "name": payload.name,
            "vertical": payload.vertical,
            "strategy": payload.strategy,
            "status": "active",
            "priority": payload.priority
        },
        "message": "Ping tree created successfully"
    })))
}

async fn get_ping_tree(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    request: Request,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let user = get_user_from_request(&request).ok_or(StatusCode::UNAUTHORIZED)?;
    let user_instance_id = get_instance_id_for_user(state.db_pool.as_ref(), user.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::BAD_REQUEST)?;
    let ping_tree = sqlx::query_as::<_, leadsnebula_core::models::ping_tree::PingTree>(
        "SELECT id, instance_id, name, vertical, strategy, status, priority, deleted_at, created_at, updated_at FROM ping_trees WHERE id = $1 AND instance_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user_instance_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    // Get campaigns in this ping tree
    let campaigns = sqlx::query_as::<_, leadsnebula_core::models::ping_tree_campaign::PingTreeCampaign>(
        "SELECT * FROM ping_tree_campaigns WHERE ping_tree_id = $1 ORDER BY priority ASC NULLS LAST"
    )
    .bind(id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get publishers assigned to this ping tree
    let publishers =
        leadsnebula_core::models::ping_tree_publisher::PingTreePublisher::find_by_ping_tree(
            state.db_pool.as_ref(),
            &id,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "success": true,
        "ping_tree": {
            "id": ping_tree.id.to_string(),
            "name": ping_tree.name,
            "vertical": ping_tree.vertical,
            "strategy": ping_tree.strategy,
            "status": ping_tree.status,
            "priority": ping_tree.priority,
            "created_at": ping_tree.created_at.to_rfc3339()
        },
        "publishers": publishers.iter().map(|p| {
            // Convert Decimal to f64 for JSON response
            serde_json::json!({
                "id": p.id.to_string(),
                "publisher_id": p.publisher_id.to_string(),
                "revshare_percentage": p.revshare_percentage.map(|d| d.to_string().parse::<f64>().unwrap_or(0.0)),
                "revshare_flat_amount": p.revshare_flat_amount.map(|d| d.to_string().parse::<f64>().unwrap_or(0.0))
            })
        }).collect::<Vec<_>>(),
        "campaigns": campaigns.iter().map(|c| serde_json::json!({
            "id": c.id.to_string(),
            "campaign_id": c.campaign_id.to_string(),
            "priority": c.priority,
            "enabled": c.enabled
        })).collect::<Vec<_>>()
    })))
}

async fn update_ping_tree(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Get ping tree before update for audit log
    let ping_tree_before = sqlx::query_as::<_, leadsnebula_core::models::ping_tree::PingTree>(
        "SELECT id, instance_id, name, vertical, strategy, status, priority, deleted_at, created_at, updated_at FROM ping_trees WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching ping tree: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let ping_tree_before = match ping_tree_before {
        Some(pt) => pt,
        None => return Err(StatusCode::NOT_FOUND),
    };

    let mut changed_fields = serde_json::Map::new();
    let mut has_updates = false;

    // Build update query dynamically
    let mut query_builder = sqlx::QueryBuilder::new("UPDATE ping_trees SET ");

    if let Some(name) = payload.get("name").and_then(|v| v.as_str()) {
        if ping_tree_before.name != name {
            changed_fields.insert(
                "name".to_string(),
                serde_json::json!({
                    "before": ping_tree_before.name,
                    "after": name
                }),
            );
        }
        query_builder.push("name = ");
        query_builder.push_bind(name);
        query_builder.push(", ");
        has_updates = true;
    }

    if let Some(priority) = payload.get("priority").and_then(|v| v.as_i64()) {
        let priority_i32 = priority as i32;
        if ping_tree_before.priority != Some(priority_i32) {
            changed_fields.insert(
                "priority".to_string(),
                serde_json::json!({
                    "before": ping_tree_before.priority,
                    "after": priority_i32
                }),
            );
        }
        query_builder.push("priority = ");
        query_builder.push_bind(priority_i32);
        query_builder.push(", ");
        has_updates = true;
    }

    if let Some(status_str) = payload.get("status").and_then(|v| v.as_str()) {
        if ping_tree_before.status != status_str {
            changed_fields.insert(
                "status".to_string(),
                serde_json::json!({
                    "before": ping_tree_before.status,
                    "after": status_str
                }),
            );
        }
        if has_updates {
            query_builder.push(", ");
        }
        query_builder.push("status = ");
        query_builder.push_bind(status_str);
        has_updates = true;
    }

    if !has_updates {
        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "No changes to update"
        })));
    }

    query_builder.push("updated_at = NOW() WHERE id = ");
    query_builder.push_bind(id);
    query_builder.push(" AND deleted_at IS NULL");

    let update_query = query_builder.build();
    update_query
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|e| {
            error!("Database error updating ping tree: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Get ping tree after update
    let ping_tree_after = sqlx::query_as::<_, leadsnebula_core::models::ping_tree::PingTree>(
        "SELECT id, instance_id, name, vertical, strategy, status, priority, deleted_at, created_at, updated_at FROM ping_trees WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .ok()
    .flatten();

    // Build before/after JSON for audit log
    let ping_tree_before_full = serde_json::json!({
        "id": ping_tree_before.id.to_string(),
        "name": ping_tree_before.name,
        "vertical": ping_tree_before.vertical,
        "strategy": ping_tree_before.strategy,
        "status": ping_tree_before.status,
        "priority": ping_tree_before.priority,
        "instance_id": ping_tree_before.instance_id.to_string(),
    });

    let ping_tree_after_full = ping_tree_after.as_ref().map(|pt| {
        serde_json::json!({
            "id": pt.id.to_string(),
            "name": pt.name,
            "vertical": pt.vertical,
            "strategy": pt.strategy,
            "status": pt.status,
            "priority": pt.priority,
            "instance_id": pt.instance_id.to_string(),
        })
    });

    // Build audit log details
    let timestamp = chrono::Utc::now();
    let audit_details = serde_json::json!({
        "action": "update",
        "target_type": "PingTree",
        "target_id": id.to_string(),
        "target_name": ping_tree_after_full.as_ref().and_then(|pt| pt.get("name").and_then(|n| n.as_str())).unwrap_or(""),
        "changes": changed_fields,
        "before": ping_tree_before_full,
        "after": ping_tree_after_full,
        "context": {
            "reason": "User updated ping tree via dashboard",
            "method": "POST",
            "endpoint": format!("/api/v1/dashboard/ping_trees/{}", id),
            "source": "dashboard_web_ui"
        },
        "outcome": "success",
        "timestamp": timestamp.to_rfc3339(),
        "compliance": {
            "standard": "ISO_27001_SOC2_NIST",
            "version": "2024"
        }
    });

    // Create audit log entry
    let _ = create_audit_log(
        state.db_pool.as_ref(),
        Some(ping_tree_before.instance_id),
        None, // TODO: Extract user_id from request extensions
        "ping_tree_updated",
        Some("PingTree"),
        Some(id),
        audit_details,
        serde_json::json!({}),
        None, // TODO: Extract IP address from request
        None, // TODO: Extract user agent from request
    )
    .await;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Ping tree updated successfully"
    })))
}

async fn delete_ping_tree(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Get ping tree before deletion for audit log
    let ping_tree_before = sqlx::query_as::<_, leadsnebula_core::models::ping_tree::PingTree>(
        "SELECT id, instance_id, name, vertical, strategy, status, priority, deleted_at, created_at, updated_at FROM ping_trees WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching ping tree: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let ping_tree_before = match ping_tree_before {
        Some(pt) => pt,
        None => return Err(StatusCode::NOT_FOUND),
    };

    // Get affected campaigns for audit log
    let affected_campaigns: Vec<serde_json::Value> =
        sqlx::query("SELECT c.id, c.name, c.campaign_token FROM campaigns c INNER JOIN ping_tree_campaigns ptc ON c.id = ptc.campaign_id WHERE ptc.ping_tree_id = $1")
            .bind(id)
            .fetch_all(state.db_pool.as_ref())
            .await
            .ok()
            .unwrap_or_default()
            .iter()
            .filter_map(|row| {
                use sqlx::Row;
                let id: Option<Uuid> = row.try_get("id").ok();
                let name: Option<String> = row.try_get("name").ok();
                let token: Option<String> = row.try_get("campaign_token").ok();
                id.map(|id| {
                    serde_json::json!({
                        "id": id.to_string(),
                        "name": name.unwrap_or_default(),
                        "campaign_token": token.unwrap_or_default()
                    })
                })
            })
            .collect();

    // Soft delete the ping tree
    sqlx::query("UPDATE ping_trees SET deleted_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|e| {
            error!("Database error soft deleting ping tree: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Log audit for ping tree deletion
    let _ = create_audit_log(
        state.db_pool.as_ref(),
        Some(ping_tree_before.instance_id),
        None, // TODO: Extract user_id from request extensions
        "ping_tree_deleted",
        Some("PingTree"),
        Some(id),
        serde_json::json!({
            "ping_tree_name": ping_tree_before.name,
            "ping_tree_vertical": ping_tree_before.vertical,
            "ping_tree_strategy": ping_tree_before.strategy
        }),
        serde_json::json!({
            "campaigns": affected_campaigns
        }),
        None, // TODO: Extract IP address from request
        None, // TODO: Extract user agent from request
    )
    .await;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Ping tree deleted successfully"
    })))
}

#[derive(Deserialize)]
pub struct AddCampaignToPingTreeRequest {
    pub campaign_id: Uuid,
    pub priority: Option<i32>,
}

async fn add_campaign_to_ping_tree(
    State(state): State<AppState>,
    Path(ping_tree_id): Path<Uuid>,
    Json(payload): Json<AddCampaignToPingTreeRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Get ping tree and campaign info for audit log
    let ping_tree = sqlx::query_as::<_, leadsnebula_core::models::ping_tree::PingTree>(
        "SELECT id, instance_id, name, vertical, strategy, status, priority, deleted_at, created_at, updated_at FROM ping_trees WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(ping_tree_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching ping tree: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let ping_tree = match ping_tree {
        Some(pt) => pt,
        None => return Err(StatusCode::NOT_FOUND),
    };

    let campaign = sqlx::query_as::<_, leadsnebula_core::models::campaign::Campaign>(
        "SELECT * FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(payload.campaign_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .ok()
    .flatten();

    sqlx::query(
        r#"
        INSERT INTO ping_tree_campaigns (
            id, ping_tree_id, campaign_id, priority, enabled, created_at, updated_at
        ) VALUES (
            gen_random_uuid(), $1, $2, $3, true, NOW(), NOW()
        )
        ON CONFLICT (ping_tree_id, campaign_id) DO UPDATE
        SET priority = EXCLUDED.priority, enabled = true, updated_at = NOW()
        "#,
    )
    .bind(ping_tree_id)
    .bind(payload.campaign_id)
    .bind(payload.priority)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Do NOT automatically update campaign status when adding to ping tree
    // User must manually activate the campaign via the toggle
    // This allows inactive campaigns to be added to ping trees without auto-activating

    // Create audit log
    let audit_details = serde_json::json!({
        "action": "add_campaign",
        "target_type": "PingTree",
        "target_id": ping_tree_id.to_string(),
        "target_name": ping_tree.name,
        "campaign_id": payload.campaign_id.to_string(),
        "campaign_name": campaign.as_ref().and_then(|c| c.name.clone()),
        "priority": payload.priority,
        "context": {
            "reason": "User added campaign to ping tree via dashboard",
            "method": "POST",
            "endpoint": format!("/api/v1/dashboard/ping_trees/{}/campaigns", ping_tree_id),
            "source": "dashboard_web_ui"
        },
        "outcome": "success",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "compliance": {
            "standard": "ISO_27001_SOC2_NIST",
            "version": "2024"
        }
    });

    let _ = create_audit_log(
        state.db_pool.as_ref(),
        Some(ping_tree.instance_id),
        None, // TODO: Extract user_id from request extensions
        "ping_tree_campaign_added",
        Some("PingTree"),
        Some(ping_tree_id),
        audit_details,
        serde_json::json!({
            "campaign": campaign.as_ref().map(|c| serde_json::json!({
                "id": c.id.to_string(),
                "name": c.name,
                "campaign_token": c.campaign_token
            }))
        }),
        None, // TODO: Extract IP address from request
        None, // TODO: Extract user agent from request
    )
    .await;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Campaign added to ping tree successfully"
    })))
}

async fn remove_campaign_from_ping_tree(
    State(state): State<AppState>,
    Path((ping_tree_id, campaign_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Get ping tree and campaign info for audit log before deletion
    let ping_tree = sqlx::query_as::<_, leadsnebula_core::models::ping_tree::PingTree>(
        "SELECT id, instance_id, name, vertical, strategy, status, priority, deleted_at, created_at, updated_at FROM ping_trees WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(ping_tree_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching ping tree: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let ping_tree = match ping_tree {
        Some(pt) => pt,
        None => return Err(StatusCode::NOT_FOUND),
    };

    let campaign = sqlx::query_as::<_, leadsnebula_core::models::campaign::Campaign>(
        "SELECT * FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(campaign_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .ok()
    .flatten();

    sqlx::query("DELETE FROM ping_tree_campaigns WHERE ping_tree_id = $1 AND campaign_id = $2")
        .bind(ping_tree_id)
        .bind(campaign_id)
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check if campaign is in any other ping trees
    let campaign_in_other_trees: Option<i64> = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ping_tree_campaigns WHERE campaign_id = $1 AND ping_tree_id != $2",
    )
    .bind(campaign_id)
    .bind(ping_tree_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .ok()
    .flatten();

    // If campaign is not in any other ping trees, set to "inactive" (ronin - not in any ping tree)
    // Note: "ronin" is not a database status enum value, so we use "inactive" to represent campaigns not in any ping tree
    // The UI can display "inactive" status as "ronin" for campaigns not in any ping tree
    if campaign_in_other_trees.unwrap_or(0) == 0 {
        sqlx::query(
            "UPDATE campaigns SET status = 'inactive' WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(campaign_id)
        .execute(state.db_pool.as_ref())
        .await
        .ok(); // Don't fail the request if status update fails
    }

    // Create audit log
    let audit_details = serde_json::json!({
        "action": "remove_campaign",
        "target_type": "PingTree",
        "target_id": ping_tree_id.to_string(),
        "target_name": ping_tree.name,
        "campaign_id": campaign_id.to_string(),
        "campaign_name": campaign.as_ref().and_then(|c| c.name.clone()),
        "context": {
            "reason": "User removed campaign from ping tree via dashboard",
            "method": "DELETE",
            "endpoint": format!("/api/v1/dashboard/ping_trees/{}/campaigns/{}", ping_tree_id, campaign_id),
            "source": "dashboard_web_ui"
        },
        "outcome": "success",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "compliance": {
            "standard": "ISO_27001_SOC2_NIST",
            "version": "2024"
        }
    });

    let _ = create_audit_log(
        state.db_pool.as_ref(),
        Some(ping_tree.instance_id),
        None, // TODO: Extract user_id from request extensions
        "ping_tree_campaign_removed",
        Some("PingTree"),
        Some(ping_tree_id),
        audit_details,
        serde_json::json!({
            "campaign": campaign.as_ref().map(|c| serde_json::json!({
                "id": c.id.to_string(),
                "name": c.name,
                "campaign_token": c.campaign_token
            }))
        }),
        None, // TODO: Extract IP address from request
        None, // TODO: Extract user agent from request
    )
    .await;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Campaign removed from ping tree successfully"
    })))
}

// Ping Tree Publisher Assignment API
#[derive(Deserialize)]
pub struct AddPublisherToPingTreeRequest {
    pub publisher_id: Uuid,
    pub revshare_percentage: Option<f64>,
    pub revshare_flat_amount: Option<f64>,
}

#[derive(Deserialize)]
pub struct UpdatePingTreePublisherRevshareRequest {
    pub revshare_percentage: Option<f64>,
    pub revshare_flat_amount: Option<f64>,
}

async fn list_ping_tree_publishers(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Verify ping tree exists
    let ping_tree = sqlx::query_as::<_, leadsnebula_core::models::ping_tree::PingTree>(
        "SELECT id, instance_id, name, vertical, strategy, status, priority, deleted_at, created_at, updated_at FROM ping_trees WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching ping tree: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if ping_tree.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let publishers =
        leadsnebula_core::models::ping_tree_publisher::PingTreePublisher::find_by_ping_tree(
            state.db_pool.as_ref(),
            &id,
        )
        .await
        .map_err(|e| {
            error!("Database error fetching publishers: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Get publisher details
    let mut publisher_details = Vec::new();
    for ptp in &publishers {
        let publisher = sqlx::query_as::<_, leadsnebula_core::models::publisher::Publisher>(
            "SELECT * FROM publishers WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(ptp.publisher_id)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .ok()
        .flatten();

        publisher_details.push(serde_json::json!({
            "id": ptp.id.to_string(),
            "publisher_id": ptp.publisher_id.to_string(),
            "publisher_name": publisher.as_ref().map(|p| p.name.clone()),
            "vertical": ptp.vertical,
            "revshare_percentage": ptp.revshare_percentage.map(|d| d.to_string().parse::<f64>().unwrap_or(0.0)),
            "revshare_flat_amount": ptp.revshare_flat_amount.map(|d| d.to_string().parse::<f64>().unwrap_or(0.0)),
            "created_at": ptp.created_at.to_rfc3339()
        }));
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "publishers": publisher_details
    })))
}

async fn add_publisher_to_ping_tree(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<AddPublisherToPingTreeRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Get ping tree to verify it exists and get vertical
    let ping_tree = sqlx::query_as::<_, leadsnebula_core::models::ping_tree::PingTree>(
        "SELECT id, instance_id, name, vertical, strategy, status, priority, deleted_at, created_at, updated_at FROM ping_trees WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching ping tree: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let ping_tree = match ping_tree {
        Some(pt) => pt,
        None => return Err(StatusCode::NOT_FOUND),
    };

    // Verify publisher exists
    let publisher = sqlx::query_as::<_, leadsnebula_core::models::publisher::Publisher>(
        "SELECT * FROM publishers WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(payload.publisher_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching publisher: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if publisher.is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Convert f64 to Decimal for database
    let revshare_percentage = payload
        .revshare_percentage
        .and_then(|v| Decimal::from_str(&v.to_string()).ok());
    let revshare_flat_amount = payload
        .revshare_flat_amount
        .and_then(|v| Decimal::from_str(&v.to_string()).ok());

    // Create assignment with validation and defaults
    let assignment = leadsnebula_core::models::ping_tree_publisher::PingTreePublisher::create(
        state.db_pool.as_ref(),
        id,
        payload.publisher_id,
        ping_tree.vertical.clone(), // Auto-set from ping_tree
        revshare_percentage,
        revshare_flat_amount,
    )
    .await
    .map_err(|e| {
        error!("Database error creating publisher assignment: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Convert Decimal to f64 for JSON response
    use rust_decimal::Decimal;
    Ok(Json(serde_json::json!({
        "success": true,
        "assignment": {
            "id": assignment.id.to_string(),
            "publisher_id": assignment.publisher_id.to_string(),
            "revshare_percentage": assignment.revshare_percentage.map(|d| d.to_string().parse::<f64>().unwrap_or(0.0)),
            "revshare_flat_amount": assignment.revshare_flat_amount.map(|d| d.to_string().parse::<f64>().unwrap_or(0.0))
        },
        "message": "Publisher added to ping tree successfully"
    })))
}

async fn remove_publisher_from_ping_tree(
    State(state): State<AppState>,
    Path((id, publisher_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Verify ping tree exists
    let ping_tree = sqlx::query_as::<_, leadsnebula_core::models::ping_tree::PingTree>(
        "SELECT id, instance_id, name, vertical, strategy, status, priority, deleted_at, created_at, updated_at FROM ping_trees WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching ping tree: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if ping_tree.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Delete assignment
    leadsnebula_core::models::ping_tree_publisher::PingTreePublisher::delete(
        state.db_pool.as_ref(),
        &id,
        &publisher_id,
    )
    .await
    .map_err(|e| {
        error!("Database error removing publisher assignment: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // TODO: Invalidate cache "routing:{publisher_id}:{vertical}" (Phase 2)

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Publisher removed from ping tree successfully"
    })))
}

async fn update_ping_tree_publisher_revshare(
    State(state): State<AppState>,
    Path((id, publisher_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<UpdatePingTreePublisherRevshareRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Find assignment
    let assignment = leadsnebula_core::models::ping_tree_publisher::PingTreePublisher::find_by_ping_tree_and_publisher(
        state.db_pool.as_ref(),
        &id,
        &publisher_id,
    )
    .await
    .map_err(|e| {
        error!("Database error fetching assignment: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let assignment = match assignment {
        Some(a) => a,
        None => return Err(StatusCode::NOT_FOUND),
    };

    // Convert f64 to Decimal for database
    let revshare_percentage = payload
        .revshare_percentage
        .and_then(|v| Decimal::from_str(&v.to_string()).ok());
    let revshare_flat_amount = payload
        .revshare_flat_amount
        .and_then(|v| Decimal::from_str(&v.to_string()).ok());

    // Update revshare
    let updated =
        leadsnebula_core::models::ping_tree_publisher::PingTreePublisher::update_revshare(
            state.db_pool.as_ref(),
            &assignment.id,
            revshare_percentage,
            revshare_flat_amount,
        )
        .await
        .map_err(|e| {
            error!("Database error updating revshare: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // TODO: Invalidate cache "routing:{publisher_id}:{vertical}" (Phase 2)

    Ok(Json(serde_json::json!({
        "success": true,
        "assignment": {
            "id": updated.id.to_string(),
            "publisher_id": updated.publisher_id.to_string(),
            "revshare_percentage": updated.revshare_percentage,
            "revshare_flat_amount": updated.revshare_flat_amount
        },
        "message": "Revshare updated successfully"
    })))
}

async fn get_publisher_revenue_share(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Verify publisher exists
    let publisher = sqlx::query_as::<_, leadsnebula_core::models::publisher::Publisher>(
        "SELECT * FROM publishers WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching publisher: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if publisher.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Get all assignments for this publisher
    let assignments =
        sqlx::query_as::<_, leadsnebula_core::models::ping_tree_publisher::PingTreePublisher>(
            r#"
        SELECT id, ping_tree_id, publisher_id, vertical, 
               revshare_percentage, revshare_flat_amount,
               created_at, updated_at
        FROM ping_tree_publishers 
        WHERE publisher_id = $1 
        ORDER BY vertical, created_at
        "#,
        )
        .bind(id)
        .fetch_all(state.db_pool.as_ref())
        .await
        .map_err(|e| {
            error!("Database error fetching assignments: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Get ping tree details for each assignment
    let mut revenue_share_configs = Vec::new();
    for assignment in &assignments {
        let ping_tree = sqlx::query_as::<_, leadsnebula_core::models::ping_tree::PingTree>(
            "SELECT id, instance_id, name, vertical, strategy, status, priority, deleted_at, created_at, updated_at FROM ping_trees WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(assignment.ping_tree_id)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .ok()
        .flatten();

        revenue_share_configs.push(serde_json::json!({
            "ping_tree_id": assignment.ping_tree_id.to_string(),
            "ping_tree_name": ping_tree.as_ref().map(|pt| pt.name.clone()),
            "vertical": assignment.vertical,
            "revshare_percentage": assignment.revshare_percentage.map(|d| d.to_string().parse::<f64>().unwrap_or(0.0)),
            "revshare_flat_amount": assignment.revshare_flat_amount.map(|d| d.to_string().parse::<f64>().unwrap_or(0.0)),
            "created_at": assignment.created_at.to_rfc3339()
        }));
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "publisher_id": id.to_string(),
        "revenue_share_configs": revenue_share_configs
    })))
}

// Verticals API
async fn list_verticals(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::{error, info};
    let verticals = sqlx::query_as::<_, leadsnebula_core::models::vertical::Vertical>(
        "SELECT * FROM verticals WHERE is_active = true ORDER BY name",
    )
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing verticals: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    info!("Found {} verticals", verticals.len());

    let response: Vec<serde_json::Value> = verticals
        .iter()
        .map(|v| {
            serde_json::json!({
                "id": v.id.to_string(),
                "name": v.name,
                "slug": v.slug,
                "is_active": v.is_active
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "verticals": response
    })))
}

// Buyer Integrations API
async fn list_buyer_integrations(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::{error, info};

    let vertical_id_param = params.get("vertical_id");

    let integrations = if let Some(vertical_id_str) = vertical_id_param {
        if let Ok(vertical_id) = Uuid::parse_str(vertical_id_str) {
            sqlx::query(
                "SELECT id, name, slug, vertical_id, description, is_internal, status, default_timeout FROM buyer_integrations WHERE status = 'available' AND vertical_id = $1 ORDER BY name"
            )
            .bind(vertical_id)
            .fetch_all(state.db_pool.as_ref())
            .await
        } else {
            sqlx::query(
                "SELECT id, name, slug, vertical_id, description, is_internal, status, default_timeout FROM buyer_integrations WHERE status = 'available' ORDER BY name"
            )
            .fetch_all(state.db_pool.as_ref())
            .await
        }
    } else {
        sqlx::query(
            "SELECT id, name, slug, vertical_id, description, is_internal, status, default_timeout FROM buyer_integrations WHERE status = 'available' ORDER BY name"
        )
        .fetch_all(state.db_pool.as_ref())
        .await
    }
    .map_err(|e| {
        error!("Database error listing buyer integrations: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    info!("Found {} buyer integrations", integrations.len());

    use sqlx::Row;
    let response: Vec<serde_json::Value> = integrations
        .iter()
        .map(|row| {
            let id: Uuid = row.try_get("id").unwrap_or_else(|_| Uuid::nil());
            let name: String = row.try_get("name").unwrap_or_else(|_| String::new());
            let slug: String = row.try_get("slug").unwrap_or_else(|_| String::new());
            let vertical_id: Uuid = row.try_get("vertical_id").unwrap_or_else(|_| Uuid::nil());
            let description: Option<String> = row.try_get("description").ok();
            let is_internal: bool = row.try_get("is_internal").unwrap_or(false);
            let status: String = row.try_get("status").unwrap_or_else(|_| String::new());
            let default_timeout: Option<f64> = row
                .try_get::<Option<String>, _>("default_timeout")
                .ok()
                .flatten()
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| row.try_get::<f64, _>("default_timeout").ok());

            serde_json::json!({
                "id": id.to_string(),
                "name": name,
                "slug": slug,
                "vertical_id": vertical_id.to_string(),
                "description": description,
                "is_internal": is_internal,
                "status": status,
                "default_timeout": default_timeout
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "integrations": response
    })))
}

// Buyer Rule Sets API

#[derive(Deserialize)]
pub struct CreateRuleSetRequest {
    pub rule_set_name: String,
    pub timeout_seconds: Option<f64>,
    pub enabled: Option<bool>,
    pub is_active: Option<bool>,
    pub config: Option<serde_json::Value>,
    pub rules_order: Option<Vec<String>>,
}

async fn list_buyer_rule_sets(
    State(state): State<AppState>,
    Path(buyer_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sqlx::Row;
    use tracing::{error, info};

    let rule_sets = sqlx::query(
        r#"
        SELECT 
            id, buyer_id, vertical_id, buyer_integration_id, rule_set_name,
            config, rules_order, enabled, is_active, timeout_seconds,
            created_at, updated_at
        FROM buyer_qualification_configs
        WHERE buyer_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(buyer_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing buyer rule sets: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    info!("Found {} rule sets for buyer {}", rule_sets.len(), buyer_id);

    // Load ZIP codes from tables once for all rule sets
    let (blacklist_zips, whitelist_zips) =
        load_zip_codes_from_tables(state.db_pool.as_ref(), buyer_id)
            .await
            .unwrap_or_else(|e| {
                error!("Error loading ZIP codes from tables: {}", e);
                (Vec::new(), Vec::new())
            });

    let response: Vec<serde_json::Value> = rule_sets
        .iter()
        .map(|row| {
            let id: Uuid = row.try_get("id").unwrap_or_else(|_| Uuid::nil());
            let buyer_id: Uuid = row.try_get("buyer_id").unwrap_or_else(|_| Uuid::nil());
            let vertical_id: Uuid = row.try_get("vertical_id").unwrap_or_else(|_| Uuid::nil());
            let buyer_integration_id: Option<Uuid> = row.try_get("buyer_integration_id").ok();
            let rule_set_name: String = row.try_get("rule_set_name").unwrap_or_default();
            let mut config: serde_json::Value = row
                .try_get("config")
                .unwrap_or_else(|_| serde_json::json!({}));
            let rules_order: Vec<String> = row.try_get("rules_order").unwrap_or_default();
            let enabled: bool = row.try_get("enabled").unwrap_or(true);
            let is_active: bool = row.try_get("is_active").unwrap_or(false);
            // DECIMAL columns must be retrieved as rust_decimal::Decimal for PostgreSQL NUMERIC
            use rust_decimal::Decimal;
            let timeout_seconds: Option<f64> = row
                .try_get::<Option<Decimal>, _>("timeout_seconds")
                .ok()
                .flatten()
                .map(|d| d.to_string().parse::<f64>().unwrap_or(1.2))
                .or_else(|| row.try_get::<f64, _>("timeout_seconds").ok())
                .or_else(|| {
                    row.try_get::<Option<String>, _>("timeout_seconds")
                        .ok()
                        .flatten()
                        .and_then(|s| s.parse::<f64>().ok())
                });
            let created_at: chrono::DateTime<chrono::Utc> = row
                .try_get("created_at")
                .unwrap_or_else(|_| chrono::Utc::now());
            let updated_at: chrono::DateTime<chrono::Utc> = row
                .try_get("updated_at")
                .unwrap_or_else(|_| chrono::Utc::now());

            // Update config with ZIP codes from tables (tables are source of truth)
            if let Some(config_obj) = config.as_object_mut() {
                if !blacklist_zips.is_empty() {
                    config_obj.insert(
                        "zip_blacklist".to_string(),
                        serde_json::json!(blacklist_zips.clone()),
                    );
                } else {
                    config_obj.remove("zip_blacklist");
                }
                if !whitelist_zips.is_empty() {
                    config_obj.insert(
                        "zip_whitelist".to_string(),
                        serde_json::json!(whitelist_zips.clone()),
                    );
                } else {
                    config_obj.remove("zip_whitelist");
                }
            }

            serde_json::json!({
                "id": id.to_string(),
                "buyer_id": buyer_id.to_string(),
                "vertical_id": vertical_id.to_string(),
                "buyer_integration_id": buyer_integration_id.map(|v| v.to_string()),
                "rule_set_name": rule_set_name,
                "config": config,
                "rules_order": rules_order,
                "enabled": enabled,
                "is_active": is_active,
                "timeout_seconds": timeout_seconds,
                "created_at": created_at.to_rfc3339(),
                "updated_at": updated_at.to_rfc3339()
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "rule_sets": response
    })))
}

async fn create_buyer_rule_set(
    State(state): State<AppState>,
    Path(buyer_id): Path<Uuid>,
    Json(payload): Json<CreateRuleSetRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sqlx::Row;
    use tracing::error;

    // Verify buyer exists
    let buyer =
        sqlx::query("SELECT id, vertical_id FROM buyers WHERE id = $1 AND deleted_at IS NULL")
            .bind(buyer_id)
            .fetch_optional(state.db_pool.as_ref())
            .await
            .map_err(|e| {
                error!("Database error checking buyer: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

    let buyer = buyer.ok_or(StatusCode::NOT_FOUND)?;
    let vertical_id: Uuid = buyer
        .try_get("vertical_id")
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let rule_set_id = Uuid::new_v4();
    let timeout_seconds = payload.timeout_seconds.unwrap_or(1.2);
    let enabled = payload.enabled.unwrap_or(true);
    let is_active = payload.is_active.unwrap_or(false);
    let config = payload.config.unwrap_or_else(|| serde_json::json!({}));
    let rules_order = payload.rules_order.unwrap_or_else(|| {
        vec![
            "zip_blacklist".to_string(),
            "zip_whitelist".to_string(),
            "own_home".to_string(),
            "roof_shade".to_string(),
            "credit_rating".to_string(),
            "monthly_bill".to_string(),
            "property_type".to_string(),
            "roof_type".to_string(),
            "purchase_timeframe".to_string(),
        ]
    });

    // If setting as active, deactivate all other rule sets for this buyer
    if is_active {
        sqlx::query("UPDATE buyer_qualification_configs SET is_active = false WHERE buyer_id = $1")
            .bind(buyer_id)
            .execute(state.db_pool.as_ref())
            .await
            .map_err(|e| {
                error!("Database error deactivating rule sets: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }

    // Extract ZIP codes from config before saving (store in separate tables, remove from JSONB)
    let mut config_for_db = config.clone();
    if let Err(e) =
        extract_and_store_zip_codes(state.db_pool.as_ref(), buyer_id, &mut config_for_db).await
    {
        error!("Error extracting and storing ZIP codes: {}", e);
        // Don't fail the request, just log the error - continue with original config
    }

    let insert_result = sqlx::query(
        r#"
        INSERT INTO buyer_qualification_configs (
            id, buyer_id, vertical_id, rule_set_name, config, rules_order,
            enabled, is_active, timeout_seconds, created_at, updated_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW()
        )
        RETURNING id
        "#,
    )
    .bind(rule_set_id)
    .bind(buyer_id)
    .bind(vertical_id)
    .bind(&payload.rule_set_name)
    .bind(&config_for_db) // Use config without ZIP codes
    .bind(&rules_order)
    .bind(enabled)
    .bind(is_active)
    .bind(timeout_seconds)
    .fetch_one(state.db_pool.as_ref())
    .await;

    match insert_result {
        Ok(_) => {
            // Log audit for rule set creation
            use sqlx::Row;
            let buyer = sqlx::query("SELECT instance_id FROM buyers WHERE id = $1")
                .bind(buyer_id)
                .fetch_optional(state.db_pool.as_ref())
                .await
                .ok()
                .flatten();
            let instance_id =
                buyer.and_then(|row| row.try_get::<Option<Uuid>, _>("instance_id").ok().flatten());

            let _ = create_audit_log(
                state.db_pool.as_ref(),
                instance_id,
                None, // TODO: Extract user_id from request extensions
                "buyer_rule_set_created",
                Some("Buyer"),
                Some(buyer_id),
                serde_json::json!({
                    "rule_set_id": rule_set_id.to_string(),
                    "rule_set_name": payload.rule_set_name,
                    "is_active": is_active,
                    "enabled": enabled,
                    "timeout_seconds": timeout_seconds
                }),
                serde_json::json!({}),
                None, // TODO: Extract IP address from request
                None, // TODO: Extract user agent from request
            )
            .await;

            Ok(Json(serde_json::json!({
                "success": true,
                "rule_set": {
                    "id": rule_set_id.to_string(),
                    "rule_set_name": payload.rule_set_name,
                    "is_active": is_active
                },
                "message": "Rule set created successfully"
            })))
        }
        Err(e) => {
            error!("Database error creating rule set: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_buyer_rule_set(
    State(state): State<AppState>,
    Path((buyer_id, rule_set_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sqlx::Row;
    use tracing::error;

    let rule_set = sqlx::query(
        r#"
        SELECT 
            id, buyer_id, vertical_id, buyer_integration_id, rule_set_name,
            config, rules_order, enabled, is_active, timeout_seconds,
            created_at, updated_at
        FROM buyer_qualification_configs
        WHERE id = $1 AND buyer_id = $2
        "#,
    )
    .bind(rule_set_id)
    .bind(buyer_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error getting rule set: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let rule_set = rule_set.ok_or(StatusCode::NOT_FOUND)?;

    let id: Uuid = rule_set.try_get("id").unwrap_or_else(|_| Uuid::nil());
    let buyer_id: Uuid = rule_set.try_get("buyer_id").unwrap_or_else(|_| Uuid::nil());
    let vertical_id: Uuid = rule_set
        .try_get("vertical_id")
        .unwrap_or_else(|_| Uuid::nil());
    let buyer_integration_id: Option<Uuid> = rule_set.try_get("buyer_integration_id").ok();
    let rule_set_name: String = rule_set.try_get("rule_set_name").unwrap_or_default();
    let mut config: serde_json::Value = rule_set
        .try_get("config")
        .unwrap_or_else(|_| serde_json::json!({}));
    let rules_order: Vec<String> = rule_set.try_get("rules_order").unwrap_or_default();
    let enabled: bool = rule_set.try_get("enabled").unwrap_or(true);
    let is_active: bool = rule_set.try_get("is_active").unwrap_or(false);
    // DECIMAL columns must be retrieved as rust_decimal::Decimal for PostgreSQL NUMERIC
    use rust_decimal::Decimal;
    let timeout_seconds: Option<f64> = rule_set
        .try_get::<Option<Decimal>, _>("timeout_seconds")
        .ok()
        .flatten()
        .map(|d| d.to_string().parse::<f64>().unwrap_or(1.2))
        .or_else(|| rule_set.try_get::<f64, _>("timeout_seconds").ok())
        .or_else(|| {
            rule_set
                .try_get::<Option<String>, _>("timeout_seconds")
                .ok()
                .flatten()
                .and_then(|s| s.parse::<f64>().ok())
        });
    let created_at: chrono::DateTime<chrono::Utc> = rule_set
        .try_get("created_at")
        .unwrap_or_else(|_| chrono::Utc::now());
    let updated_at: chrono::DateTime<chrono::Utc> = rule_set
        .try_get("updated_at")
        .unwrap_or_else(|_| chrono::Utc::now());

    // Load ZIP codes from separate tables and merge into config
    match load_zip_codes_from_tables(state.db_pool.as_ref(), buyer_id).await {
        Ok((blacklist_zips, whitelist_zips)) => {
            // Update config with ZIP codes from tables (tables are source of truth)
            if let Some(config_obj) = config.as_object_mut() {
                if !blacklist_zips.is_empty() {
                    config_obj.insert(
                        "zip_blacklist".to_string(),
                        serde_json::json!(blacklist_zips),
                    );
                } else {
                    config_obj.remove("zip_blacklist");
                }
                if !whitelist_zips.is_empty() {
                    config_obj.insert(
                        "zip_whitelist".to_string(),
                        serde_json::json!(whitelist_zips),
                    );
                } else {
                    config_obj.remove("zip_whitelist");
                }
            }
        }
        Err(e) => {
            error!("Error loading ZIP codes from tables: {}", e);
            // Continue with config as-is if loading fails
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "rule_set": {
            "id": id.to_string(),
            "buyer_id": buyer_id.to_string(),
            "vertical_id": vertical_id.to_string(),
            "buyer_integration_id": buyer_integration_id.map(|v| v.to_string()),
            "rule_set_name": rule_set_name,
            "config": config,
            "rules_order": rules_order,
            "enabled": enabled,
            "is_active": is_active,
            "timeout_seconds": timeout_seconds,
            "created_at": created_at.to_rfc3339(),
            "updated_at": updated_at.to_rfc3339()
        }
    })))
}

async fn update_buyer_rule_set(
    State(state): State<AppState>,
    Path((buyer_id, rule_set_id)): Path<(Uuid, Uuid)>,
    headers: axum::http::HeaderMap,
    axum::extract::Extension(user): axum::extract::Extension<leadsnebula_core::models::user::User>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sqlx::{QueryBuilder, Row};
    use tracing::error;

    // Get rule set before update for audit log - fetch full config JSONB
    let rule_set_before = sqlx::query(
        "SELECT rule_set_name, is_active, enabled, timeout_seconds, config FROM buyer_qualification_configs WHERE id = $1 AND buyer_id = $2"
    )
    .bind(rule_set_id)
    .bind(buyer_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error checking rule set: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let rule_set_before_full = match rule_set_before {
        Some(row) => {
            let config_before: Option<serde_json::Value> = row.try_get("config").ok().flatten();
            Some(serde_json::json!({
                "rule_set_name": row.try_get::<Option<String>, _>("rule_set_name").ok().flatten().unwrap_or_default(),
                "is_active": row.try_get::<Option<bool>, _>("is_active").ok().flatten().unwrap_or(false),
                "enabled": row.try_get::<Option<bool>, _>("enabled").ok().flatten().unwrap_or(false),
                "timeout_seconds": row.try_get::<Option<rust_decimal::Decimal>, _>("timeout_seconds").ok().and_then(|d| d.and_then(|v| v.to_string().parse::<f64>().ok())).unwrap_or(0.0),
                "config": config_before.unwrap_or(serde_json::json!({}))
            }))
        }
        None => return Err(StatusCode::NOT_FOUND),
    };

    // Build dynamic update query
    let mut query_builder = QueryBuilder::new("UPDATE buyer_qualification_configs SET ");
    let mut has_updates = false;

    if let Some(rule_set_name) = payload.get("rule_set_name").and_then(|v| v.as_str()) {
        query_builder.push("rule_set_name = ");
        query_builder.push_bind(rule_set_name);
        query_builder.push(", ");
        has_updates = true;
    }

    if let Some(config) = payload.get("config") {
        // Extract ZIP codes before saving config (store in separate tables, remove from JSONB)
        let mut config_json = config.clone();
        if let Err(e) =
            extract_and_store_zip_codes(state.db_pool.as_ref(), buyer_id, &mut config_json).await
        {
            error!("Error extracting and storing ZIP codes: {}", e);
            // Don't fail the request, just log the error - continue with cleaned config
        }
        query_builder.push("config = ");
        query_builder.push_bind(config_json); // Move ownership, not borrow
        query_builder.push(", ");
        has_updates = true;
    }

    if let Some(rules_order) = payload.get("rules_order").and_then(|v| v.as_array()) {
        let order_vec: Vec<String> = rules_order
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        let order_vec_clone = order_vec.clone();
        query_builder.push("rules_order = ");
        query_builder.push_bind(order_vec_clone);
        query_builder.push(", ");
        has_updates = true;
    }

    if let Some(enabled) = payload.get("enabled").and_then(|v| v.as_bool()) {
        query_builder.push("enabled = ");
        query_builder.push_bind(enabled);
        query_builder.push(", ");
        has_updates = true;
    }

    if let Some(is_active) = payload.get("is_active").and_then(|v| v.as_bool()) {
        // If setting as active, deactivate all other rule sets for this buyer
        if is_active {
            sqlx::query("UPDATE buyer_qualification_configs SET is_active = false WHERE buyer_id = $1 AND id != $2")
                .bind(buyer_id)
                .bind(rule_set_id)
                .execute(state.db_pool.as_ref())
                .await
                .map_err(|e| {
                    error!("Database error deactivating rule sets: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
        }
        query_builder.push("is_active = ");
        query_builder.push_bind(is_active);
        query_builder.push(", ");
        has_updates = true;
    }

    if let Some(timeout_seconds) = payload.get("timeout_seconds").and_then(|v| v.as_f64()) {
        query_builder.push("timeout_seconds = ");
        query_builder.push_bind(timeout_seconds);
        query_builder.push(", ");
        has_updates = true;
    }

    if !has_updates {
        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "No updates provided"
        })));
    }

    query_builder.push(" updated_at = NOW() WHERE id = ");
    query_builder.push_bind(rule_set_id);
    query_builder.push(" AND buyer_id = ");
    query_builder.push_bind(buyer_id);

    let query = query_builder.build();
    let update_result = query.execute(state.db_pool.as_ref()).await;

    match update_result {
        Ok(_) => {
            // ZIP codes were already extracted and stored before the update query

            // Log audit for rule set update
            use sqlx::Row;
            let buyer = sqlx::query("SELECT instance_id FROM buyers WHERE id = $1")
                .bind(buyer_id)
                .fetch_optional(state.db_pool.as_ref())
                .await
                .ok()
                .flatten();
            let instance_id =
                buyer.and_then(|row| row.try_get::<Option<Uuid>, _>("instance_id").ok().flatten());

            // Get rule set after update to capture full config
            let rule_set_after = sqlx::query(
                "SELECT rule_set_name, is_active, enabled, timeout_seconds, config FROM buyer_qualification_configs WHERE id = $1 AND buyer_id = $2"
            )
            .bind(rule_set_id)
            .bind(buyer_id)
            .fetch_optional(state.db_pool.as_ref())
            .await
            .ok()
            .flatten();

            let rule_set_after_full = if let Some(row) = rule_set_after {
                let config_after: Option<serde_json::Value> = row.try_get("config").ok().flatten();
                Some(serde_json::json!({
                    "rule_set_name": row.try_get::<Option<String>, _>("rule_set_name").ok().flatten().unwrap_or_default(),
                    "is_active": row.try_get::<Option<bool>, _>("is_active").ok().flatten().unwrap_or(false),
                    "enabled": row.try_get::<Option<bool>, _>("enabled").ok().flatten().unwrap_or(false),
                    "timeout_seconds": row.try_get::<Option<rust_decimal::Decimal>, _>("timeout_seconds").ok().and_then(|d| d.and_then(|v| v.to_string().parse::<f64>().ok())).unwrap_or(0.0),
                    "config": config_after.unwrap_or(serde_json::json!({}))
                }))
            } else {
                None
            };

            // Build before/after structure with full configs for audit log
            let mut changed_fields = serde_json::Map::new();
            if let Some(ref before) = rule_set_before_full {
                if let Some(ref after) = rule_set_after_full {
                    // Compare all fields and include full configs
                    if let Some(new_name) = after.get("rule_set_name").and_then(|v| v.as_str()) {
                        let before_name = before
                            .get("rule_set_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if before_name != new_name {
                            changed_fields.insert(
                                "rule_set_name".to_string(),
                                serde_json::json!({
                                    "before": before_name,
                                    "after": new_name
                                }),
                            );
                        }
                    }

                    // Always include full configs in before/after
                    let config_before = before
                        .get("config")
                        .cloned()
                        .unwrap_or(serde_json::json!({}));
                    let config_after = after
                        .get("config")
                        .cloned()
                        .unwrap_or(serde_json::json!({}));
                    changed_fields.insert(
                        "config".to_string(),
                        serde_json::json!({
                            "before": config_before,
                            "after": config_after
                        }),
                    );

                    if let Some(new_is_active) = after.get("is_active").and_then(|v| v.as_bool()) {
                        let before_is_active = before
                            .get("is_active")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        if before_is_active != new_is_active {
                            changed_fields.insert(
                                "is_active".to_string(),
                                serde_json::json!({
                                    "before": before_is_active,
                                    "after": new_is_active
                                }),
                            );
                        }
                    }
                    if let Some(new_enabled) = after.get("enabled").and_then(|v| v.as_bool()) {
                        let before_enabled = before
                            .get("enabled")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        if before_enabled != new_enabled {
                            changed_fields.insert(
                                "enabled".to_string(),
                                serde_json::json!({
                                    "before": before_enabled,
                                    "after": new_enabled
                                }),
                            );
                        }
                    }
                    if let Some(new_timeout) = after.get("timeout_seconds").and_then(|v| v.as_f64())
                    {
                        let before_timeout = before
                            .get("timeout_seconds")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        if (before_timeout - new_timeout).abs() > 0.001 {
                            changed_fields.insert(
                                "timeout_seconds".to_string(),
                                serde_json::json!({
                                    "before": before_timeout,
                                    "after": new_timeout
                                }),
                            );
                        }
                    }
                }
            }

            let action_type = if changed_fields.contains_key("is_active") {
                if changed_fields
                    .get("is_active")
                    .and_then(|v| v.get("after").and_then(|a| a.as_bool()))
                    .unwrap_or(false)
                {
                    "buyer_rule_set_activated"
                } else {
                    "buyer_rule_set_deactivated"
                }
            } else if changed_fields.len() == 1 && changed_fields.contains_key("timeout_seconds") {
                // Timeout applies to integration; keep separate from rule set content updates
                "buyer_rule_set_timeout_updated"
            } else {
                "buyer_rule_set_updated"
            };

            // Extract compliance-required information from headers (ISO 27001, SOC 2, NIST)
            let ip_address = headers
                .get("x-forwarded-for")
                .or_else(|| headers.get("x-real-ip"))
                .and_then(|h| h.to_str().ok())
                .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());

            let user_agent = headers
                .get("user-agent")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string());

            // Extract request ID from headers (X-Request-ID, X-Correlation-ID, or generate)
            let request_id = headers
                .get("x-request-id")
                .or_else(|| headers.get("x-correlation-id"))
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            // Extract session ID if available
            let session_id = headers
                .get("x-session-id")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string());

            // Extract referer for context
            let referer = headers
                .get("referer")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string());

            // Build actor information (ISO 27001 A.12.4.1: User identification required)
            let actor_name = format!(
                "{} {}",
                user.first_name.as_deref().unwrap_or(""),
                user.last_name.as_deref().unwrap_or("")
            )
            .trim()
            .to_string();
            let actor_name = if actor_name.is_empty() {
                user.email.clone()
            } else {
                actor_name
            };

            // Determine user role (TODO: Query from database)
            let user_role = "instance_admin"; // TODO: Determine actual role from user_roles table

            // Build full audit log details matching compliance standards (ISO 27001, SOC 2, NIST)
            let timestamp = chrono::Utc::now();
            let audit_details = serde_json::json!({
                "action": "update",
                "target_type": "BuyerRuleSet",
                "target_id": rule_set_id.to_string(),
                "target_name": rule_set_after_full.as_ref().and_then(|r| r.get("rule_set_name").and_then(|n| n.as_str())).unwrap_or(""),
                "actor": {
                    "id": user.id.to_string(),
                    "name": actor_name,
                    "email": user.email,
                    "role": user_role,
                    "instance_id": instance_id.map(|id| id.to_string())
                },
                "changes": changed_fields,
                "before": rule_set_before_full,
                "after": rule_set_after_full,
                "context": {
                    "reason": "User updated rule set via dashboard",
                    "request_id": request_id,
                    "session_id": session_id,
                    "ip_address": ip_address,
                    "user_agent": user_agent,
                    "referer": referer,
                    "method": "POST",
                    "endpoint": format!("/api/v1/dashboard/buyers/{}/rule_sets/{}", buyer_id, rule_set_id),
                    "source": "dashboard_web_ui"
                },
                "outcome": "success",
                "timestamp": timestamp.to_rfc3339(),
                "compliance": {
                    "standard": "ISO_27001_SOC2_NIST",
                    "version": "2024"
                }
            });

            let _ = create_audit_log(
                state.db_pool.as_ref(),
                instance_id,
                Some(user.id),
                action_type,
                Some("Buyer"),
                Some(buyer_id),
                audit_details,
                serde_json::json!({}),
                ip_address.as_deref(),
                user_agent.as_deref(),
            )
            .await;

            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Rule set updated successfully"
            })))
        }
        Err(e) => {
            error!("Database error updating rule set: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn delete_buyer_rule_set(
    State(state): State<AppState>,
    Path((buyer_id, rule_set_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sqlx::Row;
    use tracing::error;

    // Get rule set info before deletion for audit log
    let rule_set_before = sqlx::query(
        "SELECT rule_set_name, is_active, enabled FROM buyer_qualification_configs WHERE id = $1 AND buyer_id = $2"
    )
    .bind(rule_set_id)
    .bind(buyer_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching rule set: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let rule_set_before = rule_set_before.map(|row| {
        serde_json::json!({
            "rule_set_name": row.try_get::<Option<String>, _>("rule_set_name").ok().flatten().unwrap_or_default(),
            "is_active": row.try_get::<Option<bool>, _>("is_active").ok().flatten().unwrap_or(false),
            "enabled": row.try_get::<Option<bool>, _>("enabled").ok().flatten().unwrap_or(false)
        })
    });

    let delete_result =
        sqlx::query("DELETE FROM buyer_qualification_configs WHERE id = $1 AND buyer_id = $2")
            .bind(rule_set_id)
            .bind(buyer_id)
            .execute(state.db_pool.as_ref())
            .await;

    match delete_result {
        Ok(result) => {
            if result.rows_affected() == 0 {
                return Err(StatusCode::NOT_FOUND);
            }

            // Log audit for rule set deletion
            let buyer = sqlx::query("SELECT instance_id FROM buyers WHERE id = $1")
                .bind(buyer_id)
                .fetch_optional(state.db_pool.as_ref())
                .await
                .ok()
                .flatten();
            let instance_id =
                buyer.and_then(|row| row.try_get::<Option<Uuid>, _>("instance_id").ok().flatten());

            let _ = create_audit_log(
                state.db_pool.as_ref(),
                instance_id,
                None, // TODO: Extract user_id from request extensions
                "buyer_rule_set_deleted",
                Some("Buyer"),
                Some(buyer_id),
                serde_json::json!({
                    "rule_set_id": rule_set_id.to_string(),
                    "rule_set_name": rule_set_before.as_ref().and_then(|r| r.get("rule_set_name").and_then(|v| v.as_str())).unwrap_or("Unknown"),
                    "was_active": rule_set_before.as_ref().and_then(|r| r.get("is_active").and_then(|v| v.as_bool())).unwrap_or(false)
                }),
                serde_json::json!({}),
                None, // TODO: Extract IP address from request
                None, // TODO: Extract user agent from request
            ).await;

            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Rule set deleted successfully"
            })))
        }
        Err(e) => {
            error!("Database error deleting rule set: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Helper function to find or create a default ZIP list for a buyer
async fn find_or_create_zip_list(
    db_pool: &sqlx::PgPool,
    buyer_id: Uuid,
    list_type: &str, // "blacklist" or "whitelist"
) -> Result<Uuid, sqlx::Error> {
    use sqlx::Row;

    // Try to find existing default list for this buyer and type
    let existing = sqlx::query(
        "SELECT id FROM public.buyer_zip_lists WHERE buyer_id = $1 AND list_type = $2 ORDER BY created_at ASC LIMIT 1"
    )
    .bind(buyer_id)
    .bind(list_type)
    .fetch_optional(db_pool)
    .await?;

    if let Some(row) = existing {
        return row.try_get::<Uuid, _>("id");
    }

    // Create new default list
    let list_id = Uuid::new_v4();
    let list_name = format!(
        "Default {}",
        if list_type == "blacklist" {
            "Blacklist"
        } else {
            "Whitelist"
        }
    );

    sqlx::query(
        "INSERT INTO public.buyer_zip_lists (id, buyer_id, name, list_type, created_at, updated_at) VALUES ($1, $2, $3, $4, NOW(), NOW()) RETURNING id"
    )
    .bind(list_id)
    .bind(buyer_id)
    .bind(&list_name)
    .bind(list_type)
    .fetch_one(db_pool)
    .await?;

    Ok(list_id)
}

// Helper function to extract and store ZIP codes from config to separate tables
// This removes ZIP codes from the config JSONB and stores them only in tables
async fn extract_and_store_zip_codes(
    db_pool: &sqlx::PgPool,
    buyer_id: Uuid,
    config: &mut serde_json::Value,
) -> Result<(), sqlx::Error> {
    use sqlx::Row;
    use tracing::warn;

    // Extract and store blacklist ZIP codes, then remove from config
    if let Some(blacklist) = config
        .get("zip_blacklist")
        .and_then(|v| v.as_array())
        .cloned()
    {
        let list_id = find_or_create_zip_list(db_pool, buyer_id, "blacklist").await?;

        // Get existing ZIPs in the list
        let existing_zips: Vec<String> =
            sqlx::query("SELECT zip FROM public.buyer_zip_codes WHERE buyer_zip_list_id = $1")
                .bind(list_id)
                .fetch_all(db_pool)
                .await?
                .iter()
                .filter_map(|row| row.try_get::<String, _>("zip").ok())
                .collect();

        // Add new ZIPs that aren't already in the list
        for zip_value in blacklist {
            if let Some(zip) = zip_value.as_str() {
                let zip = zip.trim();
                if zip.len() == 5 && zip.chars().all(|c| c.is_ascii_digit()) {
                    if !existing_zips.contains(&zip.to_string()) {
                        // Insert with ON CONFLICT DO NOTHING to handle race conditions
                        let _ = sqlx::query(
                            "INSERT INTO public.buyer_zip_codes (id, buyer_zip_list_id, zip, created_at, updated_at) VALUES (gen_random_uuid(), $1, $2, NOW(), NOW()) ON CONFLICT (buyer_zip_list_id, zip) DO NOTHING"
                        )
                        .bind(list_id)
                        .bind(zip)
                        .execute(db_pool)
                        .await;
                    }
                } else {
                    warn!("Invalid ZIP code format in blacklist: {}", zip);
                }
            }
        }

        // Remove ZIP codes from config JSONB (tables are now source of truth)
        if let Some(config_obj) = config.as_object_mut() {
            config_obj.remove("zip_blacklist");
        }
    }

    // Extract and store whitelist ZIP codes, then remove from config
    if let Some(whitelist) = config
        .get("zip_whitelist")
        .and_then(|v| v.as_array())
        .cloned()
    {
        let list_id = find_or_create_zip_list(db_pool, buyer_id, "whitelist").await?;

        // Get existing ZIPs in the list
        let existing_zips: Vec<String> =
            sqlx::query("SELECT zip FROM public.buyer_zip_codes WHERE buyer_zip_list_id = $1")
                .bind(list_id)
                .fetch_all(db_pool)
                .await?
                .iter()
                .filter_map(|row| row.try_get::<String, _>("zip").ok())
                .collect();

        // Add new ZIPs that aren't already in the list
        for zip_value in whitelist {
            if let Some(zip) = zip_value.as_str() {
                let zip = zip.trim();
                if zip.len() == 5 && zip.chars().all(|c| c.is_ascii_digit()) {
                    if !existing_zips.contains(&zip.to_string()) {
                        // Insert with ON CONFLICT DO NOTHING to handle race conditions
                        let _ = sqlx::query(
                            "INSERT INTO public.buyer_zip_codes (id, buyer_zip_list_id, zip, created_at, updated_at) VALUES (gen_random_uuid(), $1, $2, NOW(), NOW()) ON CONFLICT (buyer_zip_list_id, zip) DO NOTHING"
                        )
                        .bind(list_id)
                        .bind(zip)
                        .execute(db_pool)
                        .await;
                    }
                } else {
                    warn!("Invalid ZIP code format in whitelist: {}", zip);
                }
            }
        }

        // Remove ZIP codes from config JSONB (tables are now source of truth)
        if let Some(config_obj) = config.as_object_mut() {
            config_obj.remove("zip_whitelist");
        }
    }

    Ok(())
}

// Helper function to load ZIP codes from tables and merge into config
async fn load_zip_codes_from_tables(
    db_pool: &sqlx::PgPool,
    buyer_id: Uuid,
) -> Result<(Vec<String>, Vec<String>), sqlx::Error> {
    use sqlx::Row;

    // Load blacklist ZIPs
    let blacklist_zips: Vec<String> = sqlx::query(
        r#"
        SELECT DISTINCT bzc.zip
        FROM public.buyer_zip_codes bzc
        INNER JOIN public.buyer_zip_lists bzl ON bzc.buyer_zip_list_id = bzl.id
        WHERE bzl.buyer_id = $1 AND bzl.list_type = 'blacklist'
        ORDER BY bzc.zip
        "#,
    )
    .bind(buyer_id)
    .fetch_all(db_pool)
    .await?
    .iter()
    .filter_map(|row| row.try_get::<String, _>("zip").ok())
    .collect();

    // Load whitelist ZIPs
    let whitelist_zips: Vec<String> = sqlx::query(
        r#"
        SELECT DISTINCT bzc.zip
        FROM public.buyer_zip_codes bzc
        INNER JOIN public.buyer_zip_lists bzl ON bzc.buyer_zip_list_id = bzl.id
        WHERE bzl.buyer_id = $1 AND bzl.list_type = 'whitelist'
        ORDER BY bzc.zip
        "#,
    )
    .bind(buyer_id)
    .fetch_all(db_pool)
    .await?
    .iter()
    .filter_map(|row| row.try_get::<String, _>("zip").ok())
    .collect();

    Ok((blacklist_zips, whitelist_zips))
}

// Audit Log API
async fn list_publisher_audit_logs(
    State(state): State<AppState>,
    Path(publisher_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sqlx::Row;
    use tracing::error;

    let logs = sqlx::query(
        r#"
        SELECT 
            al.id,
            al.instance_id,
            al.instance_user_id,
            al.action_type,
            al.resource_type,
            al.resource_id,
            al.details,
            al.affected_resources,
            al.ip_address,
            al.user_agent,
            al.created_at,
            al.updated_at,
            u.email as user_email,
            u.first_name as user_first_name,
            u.last_name as user_last_name
        FROM audit_logs al
        LEFT JOIN instance_users u ON al.instance_user_id = u.id
        WHERE al.resource_type = 'Publisher' AND al.resource_id = $1
        ORDER BY al.created_at DESC
        LIMIT 100
        "#,
    )
    .bind(publisher_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing publisher audit logs: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let audit_logs: Vec<serde_json::Value> = logs
        .iter()
        .map(|row| {
            let user_name = match (
                row.try_get::<Option<String>, _>("user_first_name").ok().flatten(),
                row.try_get::<Option<String>, _>("user_last_name").ok().flatten(),
            ) {
                (Some(first), Some(last)) if !first.is_empty() || !last.is_empty() => {
                    format!("{} {}", first, last).trim().to_string()
                }
                _ => row.try_get::<Option<String>, _>("user_email").ok().flatten().unwrap_or_else(|| "Unknown".to_string()),
            };

            let details_value = row.try_get::<serde_json::Value, _>("details").ok().unwrap_or_else(|| serde_json::json!({}));

            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "action_type": row.try_get::<String, _>("action_type").ok(),
                "user": user_name,
                "details": details_value,
                "affected_resources": row.try_get::<serde_json::Value, _>("affected_resources").ok().unwrap_or_else(|| serde_json::json!({})),
                "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok().map(|dt| dt.to_rfc3339()),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "audit_logs": audit_logs
    })))
}

async fn list_buyer_audit_logs(
    State(state): State<AppState>,
    Path(buyer_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sqlx::Row;
    use tracing::error;

    let logs = sqlx::query(
        r#"
        SELECT 
            al.id,
            al.instance_id,
            al.instance_user_id,
            al.action_type,
            al.resource_type,
            al.resource_id,
            al.details,
            al.affected_resources,
            al.ip_address,
            al.user_agent,
            al.created_at,
            al.updated_at,
            u.email as user_email,
            u.first_name as user_first_name,
            u.last_name as user_last_name
        FROM audit_logs al
        LEFT JOIN instance_users u ON al.instance_user_id = u.id
        WHERE al.resource_type = 'Buyer' AND al.resource_id = $1
        ORDER BY al.created_at DESC
        LIMIT 100
        "#,
    )
    .bind(buyer_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing audit logs: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let audit_logs: Vec<serde_json::Value> = logs
        .iter()
        .map(|row| {
            let user_name = match (
                row.try_get::<Option<String>, _>("user_first_name").ok().flatten(),
                row.try_get::<Option<String>, _>("user_last_name").ok().flatten(),
            ) {
                (Some(first), Some(last)) if !first.is_empty() || !last.is_empty() => {
                    format!("{} {}", first, last).trim().to_string()
                }
                _ => row.try_get::<Option<String>, _>("user_email").ok().flatten().unwrap_or_else(|| "Unknown".to_string()),
            };

            let details_value = row.try_get::<serde_json::Value, _>("details").ok().unwrap_or_else(|| serde_json::json!({}));

            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "action_type": row.try_get::<String, _>("action_type").ok(),
                "user": user_name,
                "details": details_value,
                "affected_resources": row.try_get::<serde_json::Value, _>("affected_resources").ok().unwrap_or_else(|| serde_json::json!({})),
                "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok().map(|dt| dt.to_rfc3339()),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "audit_logs": audit_logs
    })))
}

async fn list_campaign_audit_logs(
    State(state): State<AppState>,
    Path(campaign_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sqlx::Row;
    use tracing::error;

    let logs = sqlx::query(
        r#"
        SELECT 
            al.id,
            al.instance_id,
            al.instance_user_id,
            al.action_type,
            al.resource_type,
            al.resource_id,
            al.details,
            al.affected_resources,
            al.ip_address,
            al.user_agent,
            al.created_at,
            al.updated_at,
            u.email as user_email,
            u.first_name as user_first_name,
            u.last_name as user_last_name
        FROM audit_logs al
        LEFT JOIN instance_users u ON al.instance_user_id = u.id
        WHERE al.resource_type = 'Campaign' AND al.resource_id = $1
        ORDER BY al.created_at DESC
        LIMIT 100
        "#,
    )
    .bind(campaign_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing audit logs: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let audit_logs: Vec<serde_json::Value> = logs
        .iter()
        .map(|row| {
            let user_name = match (
                row.try_get::<Option<String>, _>("user_first_name").ok().flatten(),
                row.try_get::<Option<String>, _>("user_last_name").ok().flatten(),
            ) {
                (Some(first), Some(last)) if !first.is_empty() || !last.is_empty() => {
                    format!("{} {}", first, last).trim().to_string()
                }
                _ => row.try_get::<Option<String>, _>("user_email").ok().flatten().unwrap_or_else(|| "Unknown".to_string()),
            };

            let details_value = row.try_get::<serde_json::Value, _>("details").ok().unwrap_or_else(|| serde_json::json!({}));

            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "action_type": row.try_get::<String, _>("action_type").ok(),
                "user": user_name,
                "details": details_value,
                "affected_resources": row.try_get::<serde_json::Value, _>("affected_resources").ok().unwrap_or_else(|| serde_json::json!({})),
                "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok().map(|dt| dt.to_rfc3339()),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "audit_logs": audit_logs
    })))
}

async fn list_ping_tree_audit_logs(
    State(state): State<AppState>,
    Path(ping_tree_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sqlx::Row;
    use tracing::error;

    let logs = sqlx::query(
        r#"
        SELECT 
            al.id,
            al.instance_id,
            al.instance_user_id,
            al.action_type,
            al.resource_type,
            al.resource_id,
            al.details,
            al.affected_resources,
            al.ip_address,
            al.user_agent,
            al.created_at,
            al.updated_at,
            u.email as user_email,
            u.first_name as user_first_name,
            u.last_name as user_last_name
        FROM audit_logs al
        LEFT JOIN instance_users u ON al.instance_user_id = u.id
        WHERE al.resource_type = 'PingTree' AND al.resource_id = $1
        ORDER BY al.created_at DESC
        LIMIT 100
        "#,
    )
    .bind(ping_tree_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error listing ping tree audit logs: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let audit_logs: Vec<serde_json::Value> = logs
        .iter()
        .map(|row| {
            let user_name = match (
                row.try_get::<Option<String>, _>("user_first_name").ok().flatten(),
                row.try_get::<Option<String>, _>("user_last_name").ok().flatten(),
            ) {
                (Some(first), Some(last)) if !first.is_empty() || !last.is_empty() => {
                    format!("{} {}", first, last).trim().to_string()
                }
                _ => row.try_get::<Option<String>, _>("user_email").ok().flatten().unwrap_or_else(|| "Unknown".to_string()),
            };

            let details_value = row.try_get::<serde_json::Value, _>("details").ok().unwrap_or_else(|| serde_json::json!({}));

            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "action_type": row.try_get::<String, _>("action_type").ok(),
                "user": user_name,
                "details": details_value,
                "affected_resources": row.try_get::<serde_json::Value, _>("affected_resources").ok().unwrap_or_else(|| serde_json::json!({})),
                "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok().map(|dt| dt.to_rfc3339()),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "audit_logs": audit_logs
    })))
}

// Helper function to create audit log entries
#[allow(clippy::too_many_arguments)]
async fn create_audit_log(
    db_pool: &sqlx::PgPool,
    instance_id: Option<Uuid>,
    instance_user_id: Option<Uuid>,
    action_type: &str,
    resource_type: Option<&str>,
    resource_id: Option<Uuid>,
    details: serde_json::Value,
    affected_resources: serde_json::Value,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO audit_logs (
            instance_id, instance_user_id, action_type, resource_type, resource_id,
            details, affected_resources, ip_address, user_agent, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())
        "#,
    )
    .bind(instance_id)
    .bind(instance_user_id)
    .bind(action_type)
    .bind(resource_type)
    .bind(resource_id)
    .bind(details)
    .bind(affected_resources)
    .bind(ip_address)
    .bind(user_agent)
    .execute(db_pool)
    .await?;

    Ok(())
}

async fn list_leads(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::error;

    // Get user from request extensions (set by jwt_auth_middleware)
    let user = get_user_from_request(&request).ok_or(StatusCode::UNAUTHORIZED)?;

    // Get user's instance_id from instances table (where user is the owner)
    let instance_id: Option<Uuid> = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(user.id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching instance_id: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .flatten();

    // Extract query parameters from request URI
    let query_params: std::collections::HashMap<String, String> = request
        .uri()
        .query()
        .map(|q| {
            q.split('&')
                .filter_map(|pair| {
                    let mut parts = pair.splitn(2, '=');
                    let key = parts.next()?;
                    let value = parts.next().unwrap_or("");
                    Some((
                        urlencoding::decode(key)
                            .map(|s| s.into_owned())
                            .unwrap_or_else(|_| key.to_string()),
                        urlencoding::decode(value)
                            .map(|s| s.into_owned())
                            .unwrap_or_else(|_| value.to_string()),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    // Extract pagination parameters
    let page: i64 = query_params
        .get("page")
        .and_then(|p| p.parse().ok())
        .unwrap_or(1)
        .max(1);
    let per_page: i64 = query_params
        .get("per_page")
        .and_then(|p| p.parse().ok())
        .unwrap_or(100)
        .clamp(1, 100);
    let offset = (page - 1) * per_page;

    // Extract search parameter (min 3 characters)
    let search_term = query_params.get("search").and_then(|s| {
        let trimmed = s.trim();
        if trimmed.len() >= 3 {
            Some(trimmed.to_lowercase())
        } else {
            None
        }
    });

    // Build base query with search support
    // OPTIMIZATION: Only load basic fields - no payloads/PII (loaded lazily via /leads/:id/details endpoint)
    let base_query = if let Some(search) = search_term.as_ref() {
        // Search across lead_id, publisher name, buyer name, and encrypted fields (exact match for encrypted)
        // Note: For encrypted fields, we can only do exact match (deterministic encryption)
        // For other fields, we use ILIKE for case-insensitive partial match
        sqlx::query(
            r#"
            SELECT 
                l.uuid,
                l.lead_id,
                l.status::text as status,
                l.submitted_at,
                l.sold_at,
                l.created_at,
                l.updated_at,
                l.ping_id,
                l.post_id,
                l.vertical_id,
                l.vertical_data,
                v.slug as vertical_slug,
                v.name as vertical_name,
                p.name as publisher_name,
                b.name as buyer_name,
                EXTRACT(EPOCH FROM (COALESCE(l.sold_at, l.updated_at) - COALESCE(l.submitted_at, l.created_at))) * 1000 as processing_time_ms,
                (SELECT 
                    CASE 
                        WHEN pp.payload::jsonb ? 'routing_result' THEN (pp.payload::jsonb->'routing_result'->>'price')::float
                        WHEN pp.payload::jsonb ? 'price' THEN (pp.payload::jsonb->>'price')::float
                        ELSE NULL
                    END
                 FROM post_payloads pp
                 WHERE pp.lead_id = l.uuid AND (l.post_id IS NULL OR pp.post_id = l.post_id)
                 ORDER BY pp.created_at DESC
                 LIMIT 1) as price
            FROM leads l
            LEFT JOIN verticals v ON l.vertical_id = v.id
            LEFT JOIN publishers p ON l.publisher_id = p.id AND p.deleted_at IS NULL
            LEFT JOIN buyers b ON l.buyer_id = b.id AND b.deleted_at IS NULL
            WHERE EXISTS (
                SELECT 1 FROM publishers pub 
                WHERE pub.id = l.publisher_id 
                AND pub.instance_id = $1 
                AND pub.deleted_at IS NULL
            )
            AND (
                LOWER(l.lead_id) LIKE $2
                OR LOWER(p.name) LIKE $2
                OR LOWER(b.name) LIKE $2
                OR l.email_encrypted = $3
                OR l.ip_address_encrypted = $3
            )
            ORDER BY l.created_at DESC
            LIMIT $4 OFFSET $5
            "#,
        )
        .bind(instance_id)
        .bind(format!("%{}%", search))
        .bind(search.clone())
        .bind(per_page)
        .bind(offset)
    } else {
        sqlx::query(
            r#"
            SELECT 
                l.uuid,
                l.lead_id,
                l.status::text as status,
                l.submitted_at,
                l.sold_at,
                l.created_at,
                l.updated_at,
                l.ping_id,
                l.post_id,
                l.vertical_id,
                l.vertical_data,
                v.slug as vertical_slug,
                v.name as vertical_name,
                p.name as publisher_name,
                b.name as buyer_name,
                EXTRACT(EPOCH FROM (COALESCE(l.sold_at, l.updated_at) - COALESCE(l.submitted_at, l.created_at))) * 1000 as processing_time_ms,
                (SELECT 
                    CASE 
                        WHEN pp.payload::jsonb ? 'routing_result' THEN (pp.payload::jsonb->'routing_result'->>'price')::float
                        WHEN pp.payload::jsonb ? 'price' THEN (pp.payload::jsonb->>'price')::float
                        ELSE NULL
                    END
                 FROM post_payloads pp
                 WHERE pp.lead_id = l.uuid AND (l.post_id IS NULL OR pp.post_id = l.post_id)
                 ORDER BY pp.created_at DESC
                 LIMIT 1) as price
            FROM leads l
            LEFT JOIN verticals v ON l.vertical_id = v.id
            LEFT JOIN publishers p ON l.publisher_id = p.id AND p.deleted_at IS NULL
            LEFT JOIN buyers b ON l.buyer_id = b.id AND b.deleted_at IS NULL
            WHERE EXISTS (
                SELECT 1 FROM publishers pub 
                WHERE pub.id = l.publisher_id 
                AND pub.instance_id = $1 
                AND pub.deleted_at IS NULL
            )
            ORDER BY l.created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(instance_id)
        .bind(per_page)
        .bind(offset)
    };

    let leads_query = base_query
        .fetch_all(state.db_pool.as_ref())
        .await
        .map_err(|e| {
            error!("Database error fetching leads: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Get total count for pagination (before applying limit/offset)
    let total_count_query = if let Some(search) = search_term.as_ref() {
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(DISTINCT l.uuid)
            FROM leads l
            LEFT JOIN publishers p ON l.publisher_id = p.id AND p.deleted_at IS NULL
            LEFT JOIN buyers b ON l.buyer_id = b.id AND b.deleted_at IS NULL
            WHERE EXISTS (
                SELECT 1 FROM publishers pub 
                WHERE pub.id = l.publisher_id 
                AND pub.instance_id = $1 
                AND pub.deleted_at IS NULL
            )
            AND (
                LOWER(l.lead_id) LIKE $2
                OR LOWER(p.name) LIKE $2
                OR LOWER(b.name) LIKE $2
                OR l.email_encrypted = $3
                OR l.ip_address_encrypted = $3
            )
            "#,
        )
        .bind(instance_id)
        .bind(format!("%{}%", search))
        .bind(search.clone())
    } else {
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(DISTINCT l.uuid)
            FROM leads l
            WHERE EXISTS (
                SELECT 1 FROM publishers pub 
                WHERE pub.id = l.publisher_id 
                AND pub.instance_id = $1 
                AND pub.deleted_at IS NULL
            )
            "#,
        )
        .bind(instance_id)
    };

    let total_count: i64 = total_count_query
        .fetch_one(state.db_pool.as_ref())
        .await
        .map_err(|e| {
            error!("Database error counting leads: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let total_pages = ((total_count as f64) / (per_page as f64)).ceil() as i64;

    // OPTIMIZATION: Only build basic lead information - no payloads/PII (loaded lazily)
    // Group leads by vertical and build response
    let mut leads_by_vertical: std::collections::HashMap<String, Vec<serde_json::Value>> =
        std::collections::HashMap::new();

    for row in leads_query {
        let lead_uuid: Uuid = row.try_get("uuid").map_err(|e| {
            error!("Error getting lead uuid: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        let vertical_slug: String = row
            .try_get("vertical_slug")
            .unwrap_or_else(|_| "unknown".to_string());

        // Calculate processing time
        let processing_time_ms: Option<f64> = row
            .try_get::<Option<f64>, _>("processing_time_ms")
            .ok()
            .flatten();

        // Extract auction_timing from vertical_data (post_ms, total_ms from AtomicAuctionTiming)
        let auction_timing: Option<serde_json::Value> = row
            .try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>("vertical_data")
            .ok()
            .flatten()
            .and_then(|j| j.0.get("auction_timing").cloned());

        let post_ms: Option<f64> = auction_timing
            .as_ref()
            .and_then(|at| at.get("post_ms"))
            .and_then(|v| v.as_f64());
        let total_ms: Option<f64> = auction_timing
            .as_ref()
            .and_then(|at| at.get("total_ms"))
            .and_then(|v| v.as_f64());
        // Cumulative total: use the larger of stored total_ms or DB wall-clock processing_time_ms
        // so we never show a partial (e.g. post-only) time when the real end-to-end time is larger
        let effective_total_ms: Option<f64> = match (total_ms, processing_time_ms) {
            (Some(v), Some(p)) => Some(v.max(p)),
            (Some(v), None) => Some(v),
            (None, Some(p)) => Some(p),
            (None, None) => None,
        };

        // Fetch price from post_payloads
        let price: Option<f64> = row.try_get::<Option<f64>, _>("price").ok().flatten();

        // #region agent log
        let log_entry = serde_json::json!({
            "sessionId": "debug-session",
            "runId": "run1",
            "hypothesisId": "F",
            "location": "dashboard.rs:5970",
            "message": "Lead list - price and processing time",
            "data": {
                "lead_uuid": lead_uuid.to_string(),
                "price": price,
                "processing_time_ms": processing_time_ms,
                "status": row.try_get::<String, _>("status").ok()
            },
            "timestamp": chrono::Utc::now().timestamp_millis()
        });
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/home/badinoff/projects/leadsnebula/.cursor/debug.log")
        {
            use std::io::Write;
            let _ = writeln!(
                file,
                "{}",
                serde_json::to_string(&log_entry).unwrap_or_default()
            );
        }
        // #endregion

        // Build minimal lead JSON (no PII, no payloads - loaded via /leads/:id/details endpoint)
        let lead_json = serde_json::json!({
            "uuid": lead_uuid.to_string(),
            "lead_id": row.try_get::<Option<String>, _>("lead_id").ok().flatten(),
            "status": row.try_get::<String, _>("status").ok(),
            "price": price,
            "submitted_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("submitted_at").ok().flatten().map(|d| d.to_rfc3339()),
            "processing_time_ms": processing_time_ms,
            "auction_timing": if post_ms.is_some() || effective_total_ms.is_some() { serde_json::json!({ "post_ms": post_ms, "total_ms": effective_total_ms }) } else { serde_json::Value::Null },
            "publisher_name": row.try_get::<Option<String>, _>("publisher_name").ok().flatten(),
            "buyer_name": row.try_get::<Option<String>, _>("buyer_name").ok().flatten(),
            // PII and payloads are NOT included - loaded lazily via /leads/:id/details endpoint
        });

        leads_by_vertical
            .entry(vertical_slug.clone())
            .or_default()
            .push(lead_json);
    }

    // Build final response grouped by vertical
    let mut verticals: Vec<serde_json::Value> = Vec::new();
    for (slug, leads) in leads_by_vertical {
        // Capitalize the slug for display name
        let vertical_name = slug
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + &slug[1..])
            .unwrap_or_else(|| slug.to_uppercase());

        verticals.push(serde_json::json!({
            "slug": slug,
            "name": vertical_name,
            "leads": leads,
        }));
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "verticals": verticals,
        "pagination": {
            "page": page,
            "per_page": per_page,
            "total": total_count,
            "total_pages": total_pages,
        },
    })))
}

// Get detailed lead information including PII and payloads (lazy loaded)
async fn get_lead_details(
    State(state): State<AppState>,
    Path(lead_id): Path<String>,
    request: Request,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use leadsnebula_core::encryption::EncryptionService;
    use tracing::error;

    // Get user from request extensions (set by jwt_auth_middleware)
    let user = get_user_from_request(&request).ok_or(StatusCode::UNAUTHORIZED)?;

    // Get user's instance_id from instances table (where user is the owner)
    let instance_id: Option<Uuid> = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT id FROM instances WHERE instance_user_id = $1 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(user.id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching instance_id: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .flatten();

    // Parse lead UUID
    let lead_uuid = Uuid::parse_str(&lead_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Initialize encryption service for PII decryption
    // In local development, SSM may be unavailable - allow viewing leads without decryption
    let env_norm = leadsnebula_core::normalize_env_for_ssm(&state.config.environment).to_string();
    let det_path = format!(
        "/leadsnebula/{}/carina/encryption/deterministic_key_v1",
        env_norm
    );
    let salt_path = format!(
        "/leadsnebula/{}/carina/encryption/key_derivation_salt_v1",
        env_norm
    );

    let pii_decryption_key =
        if let Ok(Some(det_key)) = state.ssm.get_parameter(&det_path, true).await {
            if let Ok(Some(salt)) = state.ssm.get_parameter(&salt_path, true).await {
                Some(
                    leadsnebula_core::encryption::EncryptionService::derive_key_from_secret(
                        &det_key, &salt,
                    ),
                )
            } else {
                None
            }
        } else {
            None
        };

    // If PII decryption key is unavailable (e.g., local dev without SSM), return lead data without decrypted PII
    // This allows viewing lead details even when SSM is unavailable
    let pii_decryption_key_opt = pii_decryption_key;

    // Fetch lead with basic info
    let lead_row = sqlx::query(
        r#"
        SELECT 
            l.uuid,
            l.lead_id,
            l.status::text as status,
            l.submitted_at,
            l.sold_at,
            l.created_at,
            l.updated_at,
            l.vertical_data,
            l.first_name_encrypted,
            l.last_name_encrypted,
            l.email_encrypted,
            l.street_address_encrypted,
            l.zip_encrypted,
            l.ip_address_encrypted,
            l.ping_id,
            l.post_id,
            l.vertical_id,
            p.name as publisher_name,
            b.name as buyer_name,
            EXTRACT(EPOCH FROM (COALESCE(l.sold_at, l.updated_at) - COALESCE(l.submitted_at, l.created_at))) * 1000 as processing_time_ms
        FROM leads l
        LEFT JOIN publishers p ON l.publisher_id = p.id AND p.deleted_at IS NULL
        LEFT JOIN buyers b ON l.buyer_id = b.id AND b.deleted_at IS NULL
        WHERE l.uuid = $1
        AND EXISTS (
            SELECT 1 FROM publishers pub 
            WHERE pub.id = l.publisher_id 
            AND pub.instance_id = $2 
            AND pub.deleted_at IS NULL
        )
        "#,
    )
    .bind(lead_uuid)
    .bind(instance_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        error!("Database error fetching lead: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    // Decrypt PII (if decryption key is available)
    // Return placeholder if decryption fails or key unavailable (instead of None/n/a)
    let decrypt_pii = |enc: Option<String>| -> Option<String> {
        if let Some(ref key) = pii_decryption_key_opt {
            enc.and_then(|e| {
                if e.is_empty() {
                    return None;
                }
                leadsnebula_core::encryption::EncryptionService::decrypt_envelope(key, &e)
                    .ok()
                    .or_else(|| {
                        EncryptionService::new(key)
                            .ok()
                            .and_then(|svc| svc.decrypt(&e).ok())
                    })
            })
        } else {
            // PII decryption key unavailable - return placeholder if encrypted data exists (even if empty string)
            if let Some(e) = enc {
                if !e.is_empty() {
                    Some("[Encrypted - decryption key unavailable]".to_string())
                } else {
                    None
                }
            } else {
                None
            }
        }
    };

    let first_name_enc = lead_row
        .try_get::<Option<String>, _>("first_name_encrypted")
        .ok()
        .flatten();
    let first_name = decrypt_pii(first_name_enc.clone());
    // #region agent log
    let log_entry = serde_json::json!({
        "sessionId": "debug-session",
        "runId": "run1",
        "hypothesisId": "D",
        "location": "dashboard.rs:6134",
        "message": "PII decryption result",
        "data": {
            "lead_uuid": lead_uuid.to_string(),
            "first_name_encrypted_exists": first_name_enc.is_some(),
            "first_name_encrypted_empty": first_name_enc.as_ref().map(|s| s.is_empty()),
            "first_name_result": first_name.clone(),
            "pii_decryption_key_available": pii_decryption_key_opt.is_some()
        },
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/home/badinoff/projects/leadsnebula/.cursor/debug.log")
    {
        use std::io::Write;
        let _ = writeln!(
            file,
            "{}",
            serde_json::to_string(&log_entry).unwrap_or_default()
        );
    }
    // #endregion
    let last_name = decrypt_pii(
        lead_row
            .try_get::<Option<String>, _>("last_name_encrypted")
            .ok()
            .flatten(),
    );
    let email = decrypt_pii(
        lead_row
            .try_get::<Option<String>, _>("email_encrypted")
            .ok()
            .flatten(),
    );
    let street_address = decrypt_pii(
        lead_row
            .try_get::<Option<String>, _>("street_address_encrypted")
            .ok()
            .flatten(),
    );
    let zip = decrypt_pii(
        lead_row
            .try_get::<Option<String>, _>("zip_encrypted")
            .ok()
            .flatten(),
    );
    let ip_address = decrypt_pii(
        lead_row
            .try_get::<Option<String>, _>("ip_address_encrypted")
            .ok()
            .flatten(),
    );

    // Fetch ping payloads (for fallback when buyer_responses don't exist, and for ping request payloads)
    // Also fetch ping_id to match with buyer_responses
    let ping_payloads_rows = sqlx::query(
        r#"
        SELECT 
            pp.id,
            pp.ping_id,
            pp.lead_id,
            pp.payload,
            pp.request_payload_encrypted,
            pp.response_payload_encrypted
        FROM ping_payloads pp
        WHERE pp.lead_id = $1
        ORDER BY pp.created_at ASC
        "#,
    )
    .bind(lead_uuid)
    .fetch_all(state.db_pool.as_ref())
    .await
    .ok()
    .unwrap_or_default();

    // Fetch buyer responses (ping and post responses)
    let buyer_responses = sqlx::query(
        r#"
        SELECT 
            br.id,
            br.ping_id,
            br.post_id,
            br.buyer_id,
            br.campaign_id,
            br.payload,
            br.response_payload_encrypted,
            br.created_at,
            b.name as buyer_name,
            c.name as campaign_name
        FROM buyer_responses br
        LEFT JOIN buyers b ON br.buyer_id = b.id AND b.deleted_at IS NULL
        LEFT JOIN campaigns c ON br.campaign_id = c.id AND c.deleted_at IS NULL
        WHERE br.lead_id = $1
        ORDER BY br.created_at ASC
        "#,
    )
    .bind(lead_uuid)
    .fetch_all(state.db_pool.as_ref())
    .await
    .ok()
    .unwrap_or_default();

    // Build ping payloads with decryption (if decryption key is available)
    let decrypt_payload = |enc: Option<String>| -> Option<serde_json::Value> {
        if let Some(ref key) = pii_decryption_key_opt {
            enc.and_then(|e| {
                leadsnebula_core::encryption::EncryptionService::decrypt_envelope(key, &e)
                    .ok()
                    .or_else(|| {
                        EncryptionService::new(key)
                            .ok()
                            .and_then(|svc| svc.decrypt(&e).ok())
                    })
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            })
        } else {
            // PII decryption key unavailable - return None (payloads will be shown as encrypted)
            None
        }
    };

    let original_request_payload: Option<serde_json::Value> =
        ping_payloads_rows.first().and_then(|row| {
            let request_encrypted: Option<String> = row
                .try_get::<Option<String>, _>("request_payload_encrypted")
                .ok()
                .flatten();
            let payload_json: Option<serde_json::Value> = row
                .try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>("payload")
                .ok()
                .flatten()
                .map(|j| j.0);
            decrypt_payload(request_encrypted).or(payload_json)
        });

    let lead_created_at = lead_row
        .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("created_at")
        .ok()
        .flatten();

    // Build ping payloads from buyer_responses (preferred) or fallback to ping_payloads table (for historical leads)
    // Filter to only ping responses (ping_id IS NOT NULL)
    let ping_buyer_responses: Vec<_> = buyer_responses
        .iter()
        .filter(|row| {
            row.try_get::<Option<String>, _>("ping_id")
                .ok()
                .flatten()
                .is_some()
        })
        .collect();

    // Get winning buyer_id and campaign_id from lead
    let winning_buyer_id: Option<uuid::Uuid> = lead_row
        .try_get::<Option<uuid::Uuid>, _>("buyer_id")
        .ok()
        .flatten();
    let winning_campaign_id: Option<uuid::Uuid> = lead_row
        .try_get::<Option<uuid::Uuid>, _>("campaign_id")
        .ok()
        .flatten();

    // Build map: ping_id (string "FP_...") -> ping request payload by joining ping_payloads to pings.
    // ping_payloads.ping_id can be bigint (pings.id) in DB; pings.ping_id is the "FP_..." string that matches buyer_responses.ping_id (with optional _C{campaign} suffix).
    let ping_request_payloads: std::collections::HashMap<String, Option<serde_json::Value>> = {
        let join_rows = sqlx::query(
            r#"
            SELECT p.ping_id, pp.request_payload_encrypted
            FROM ping_payloads pp
            JOIN pings p ON p.lead_id = pp.lead_id AND p.id::text = pp.ping_id::text
            WHERE pp.lead_id = $1
            "#,
        )
        .bind(lead_uuid)
        .fetch_all(state.db_pool.as_ref())
        .await
        .ok()
        .unwrap_or_default();
        let mut m = std::collections::HashMap::new();
        for row in &join_rows {
            if let Ok(Some(k)) = row.try_get::<Option<String>, _>("ping_id") {
                let enc: Option<String> = row.try_get("request_payload_encrypted").ok().flatten();
                m.insert(k, decrypt_payload(enc));
            }
        }
        m
    };
    let map_keys: Vec<&String> = ping_request_payloads.keys().collect();
    tracing::warn!(
        lead_id = %lead_uuid,
        map_len = ping_request_payloads.len(),
        map_keys = ?map_keys,
        "Dashboard get_lead_details: ping_request_payloads map built (visible in cargo run logs)"
    );

    // Convert buyer_response ping_id ("FP_<b64(lead|ts|accepted)>_C{campaign}") to map key ("FP_<b64(lead|ts|pending)>").
    // Map is keyed by pings.ping_id which uses "pending"; buyer_responses use "accepted" + _C suffix.
    let br_ping_id_to_pending_key = |pid: &str| -> Option<String> {
        let after_fp = pid.strip_prefix("FP_")?;
        let base_b64 = after_fp.rsplit("_C").next()?;
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(base_b64.as_bytes())
            .ok()?;
        let decoded_str = String::from_utf8(decoded).ok()?;
        let pending_str = decoded_str.replace("|accepted", "|pending");
        Some(format!(
            "FP_{}",
            base64::engine::general_purpose::STANDARD.encode(pending_str.as_bytes())
        ))
    };

    let ping_payloads: Vec<serde_json::Value> = if !ping_buyer_responses.is_empty() {
        // Use buyer_responses (normal case) - show ALL pings (winning and losing)
        ping_buyer_responses
            .iter()
            .map(|row| {
                let ping_id: Option<String> =
                    row.try_get::<Option<String>, _>("ping_id").ok().flatten();
                let buyer_id: Option<uuid::Uuid> =
                    row.try_get::<Option<uuid::Uuid>, _>("buyer_id").ok().flatten();
                let campaign_id: Option<uuid::Uuid> =
                    row.try_get::<Option<uuid::Uuid>, _>("campaign_id").ok().flatten();

                // Determine if this is the winning ping
                let is_winner = winning_buyer_id.is_some()
                    && winning_campaign_id.is_some()
                    && buyer_id == winning_buyer_id
                    && campaign_id == winning_campaign_id;

                let payload: Option<serde_json::Value> = row
                    .try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>("payload")
                    .ok()
                    .flatten()
                    .map(|j| j.0);
                let response_encrypted: Option<String> = row
                    .try_get::<Option<String>, _>("response_payload_encrypted")
                    .ok()
                    .flatten();
                let response_payload = decrypt_payload(response_encrypted).or_else(|| payload.clone());
                let bid = response_payload
                    .as_ref()
                    .and_then(|r| r.get("bid"))
                    .and_then(|b| b.as_f64());
                let processing_time_ms = if let (Some(created_at), Some(lead_created)) = (
                    row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("created_at")
                        .ok()
                        .flatten(),
                    lead_created_at,
                ) {
                    Some(
                        created_at
                            .signed_duration_since(lead_created)
                            .num_milliseconds() as f64,
                    )
                } else {
                    None
                };

                // Get ping request payload: map is keyed by pings.ping_id ("FP_...|pending").
                // buyer_responses.ping_id is "FP_...|accepted_C{campaign}". Resolve via pending-key derivation.
                let ping_request_payload = ping_id
                    .as_ref()
                    .and_then(|pid| {
                        let direct = ping_request_payloads.get(pid).cloned();
                        let via_pending = direct.or_else(|| {
                            br_ping_id_to_pending_key(pid)
                                .and_then(|k| ping_request_payloads.get(&k).cloned())
                        });
                        via_pending
                    })
                    .flatten()
                    .or_else(|| original_request_payload.clone());

                // Per-ping timing for verbose display (ping payloads show ping-phase times, not post)
                let auction_timing = processing_time_ms.map(|ms| {
                    serde_json::json!({
                        "request_type": "ping",
                        "total_ms": ms,
                    })
                });

                serde_json::json!({
                    "id": row.try_get::<i64, _>("id").ok(),
                    "ping_id": ping_id,
                    "buyer_name": row.try_get::<Option<String>, _>("buyer_name").ok().flatten(),
                    "campaign_name": row.try_get::<Option<String>, _>("campaign_name").ok().flatten(),
                    "bid": bid,
                    "processing_time_ms": processing_time_ms,
                    "is_winner": is_winner,
                    "status": if is_winner { "W" } else { "L" },
                    "request_payload": ping_request_payload,
                    "response_payload": response_payload,
                    "auction_timing": auction_timing,
                })
            })
            .collect()
    } else if !ping_payloads_rows.is_empty() {
        // Fallback: Use ping_payloads table for historical leads (when buyer_responses don't exist)
        ping_payloads_rows
            .iter()
            .map(|row| {
                let payload_json: Option<serde_json::Value> = row
                    .try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>("payload")
                    .ok()
                    .flatten()
                    .map(|j| j.0);
                let request_encrypted: Option<String> = row
                    .try_get::<Option<String>, _>("request_payload_encrypted")
                    .ok()
                    .flatten();
                let response_encrypted: Option<String> = row
                    .try_get::<Option<String>, _>("response_payload_encrypted")
                    .ok()
                    .flatten();
                let request_payload =
                    decrypt_payload(request_encrypted.clone()).or_else(|| payload_json.clone());
                let response_payload = decrypt_payload(response_encrypted);
                let bid = response_payload
                    .as_ref()
                    .and_then(|r| r.get("bid"))
                    .and_then(|b| b.as_f64());

                serde_json::json!({
                    "id": None::<i64>,
                    "ping_id": None::<String>,
                    "buyer_name": None::<String>,
                    "campaign_name": None::<String>,
                    "bid": bid,
                    "processing_time_ms": None::<f64>,
                    "request_payload": request_payload,
                    "response_payload": response_payload,
                    "auction_timing": None::<serde_json::Value>,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    // Fetch post payload
    // Try to get post_id from lead, or fallback to buyer_responses with post_id
    let post_id: Option<String> = lead_row
        .try_get::<Option<String>, _>("post_id")
        .ok()
        .flatten()
        .or_else(|| {
            // Fallback: find post_id from buyer_responses if lead.post_id is empty
            buyer_responses
                .iter()
                .find_map(|row| row.try_get::<Option<String>, _>("post_id").ok().flatten())
        });

    // Fetch post payload - try with post_id first, then fallback to any post_payloads for this lead
    let post_payload: Option<serde_json::Value> = if let Some(pid) = post_id {
        // Normal case: use post_id from lead
        sqlx::query(
            r#"
            SELECT 
                pp.id,
                pp.post_id,
                pp.payload,
                pp.request_payload_encrypted,
                pp.response_payload_encrypted,
                pp.created_at,
                EXTRACT(EPOCH FROM (pp.updated_at - pp.created_at)) * 1000 as processing_time_ms
            FROM post_payloads pp
            WHERE pp.lead_id = $1 AND pp.post_id = $2
            "#,
        )
        .bind(lead_uuid)
        .bind(&pid)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .ok()
        .flatten()
        .map(|row| {
            let payload: Option<serde_json::Value> = row
                .try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>("payload")
                .ok()
                .flatten()
                .map(|j| j.0);
            let request_payload = decrypt_payload(
                row.try_get::<Option<String>, _>("request_payload_encrypted").ok().flatten()
            ).or_else(|| payload.clone());
            let response_payload = decrypt_payload(
                row.try_get::<Option<String>, _>("response_payload_encrypted").ok().flatten()
            );
            let price = response_payload
                .as_ref()
                .and_then(|r| r.get("routing_result"))
                .and_then(|rr| rr.get("price"))
                .and_then(|p| p.as_f64());

            serde_json::json!({
                "id": row.try_get::<i64, _>("id").ok(),
                "post_id": pid,
                "price": price,
                "processing_time_ms": row.try_get::<Option<f64>, _>("processing_time_ms").ok().flatten(),
                "request_payload": request_payload,
                "response_payload": response_payload,
            })
        })
    } else {
        // Fallback: find any post_payloads for this lead (for historical leads where post_id wasn't set)
        sqlx::query(
            r#"
            SELECT 
                pp.id,
                pp.post_id,
                pp.payload,
                pp.request_payload_encrypted,
                pp.response_payload_encrypted,
                pp.created_at,
                EXTRACT(EPOCH FROM (pp.updated_at - pp.created_at)) * 1000 as processing_time_ms
            FROM post_payloads pp
            WHERE pp.lead_id = $1
            ORDER BY pp.created_at DESC
            LIMIT 1
            "#,
        )
        .bind(lead_uuid)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .ok()
        .flatten()
        .map(|row| {
            let post_id_from_db: Option<String> = row
                .try_get::<Option<String>, _>("post_id")
                .ok()
                .flatten();
            let payload: Option<serde_json::Value> = row
                .try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>("payload")
                .ok()
                .flatten()
                .map(|j| j.0);
            let request_payload = decrypt_payload(
                row.try_get::<Option<String>, _>("request_payload_encrypted").ok().flatten()
            ).or_else(|| payload.clone());
            let response_payload = decrypt_payload(
                row.try_get::<Option<String>, _>("response_payload_encrypted").ok().flatten()
            );
            let price = response_payload
                .as_ref()
                .and_then(|r| r.get("routing_result"))
                .and_then(|rr| rr.get("price"))
                .and_then(|p| p.as_f64());

            serde_json::json!({
                "id": row.try_get::<i64, _>("id").ok(),
                "post_id": post_id_from_db,
                "price": price,
                "processing_time_ms": row.try_get::<Option<f64>, _>("processing_time_ms").ok().flatten(),
                "request_payload": request_payload,
                "response_payload": response_payload,
            })
        })
    };

    let lead_price = post_payload
        .as_ref()
        .and_then(|pp| {
            // #region agent log
            let log_entry = serde_json::json!({
                "sessionId": "debug-session",
                "runId": "run1",
                "hypothesisId": "C",
                "location": "dashboard.rs:6574",
                "message": "Extracting price from post_payload",
                "data": {
                    "lead_uuid": lead_uuid.to_string(),
                    "post_payload_keys": pp.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()),
                    "has_price": pp.get("price").is_some(),
                    "price_value": pp.get("price")
                },
                "timestamp": chrono::Utc::now().timestamp_millis()
            });
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("/home/badinoff/projects/leadsnebula/.cursor/debug.log") {
                use std::io::Write;
                let _ = writeln!(file, "{}", serde_json::to_string(&log_entry).unwrap_or_default());
            }
            // #endregion
            pp.get("price")
        })
        .and_then(|p| p.as_f64());

    let processing_time_ms: Option<f64> = lead_row
        .try_get::<Option<f64>, _>("processing_time_ms")
        .ok()
        .flatten();

    // Extract full vertical_data (auction_timing + verbose for gear modal / compliance)
    let vertical_data: Option<sqlx::types::Json<serde_json::Value>> = lead_row
        .try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>("vertical_data")
        .ok()
        .flatten();
    let vertical_data_value = vertical_data.as_ref().map(|j| j.0.clone());
    let auction_timing: Option<serde_json::Value> = vertical_data
        .as_ref()
        .and_then(|j| j.0.get("auction_timing").cloned());
    let _post_ms: Option<f64> = auction_timing
        .as_ref()
        .and_then(|at| at.get("post_ms"))
        .and_then(|v| v.as_f64());
    let _total_ms: Option<f64> = auction_timing
        .as_ref()
        .and_then(|at| at.get("total_ms"))
        .and_then(|v| v.as_f64());
    // Expose full timing breakdown (pre_checks_ms, ping_auction_ms, qualification_ms, post_ms, total_ms, db_operations_ms) for UI/debugging
    let auction_timing_full = auction_timing.clone().unwrap_or(serde_json::json!({}));

    let response_json = serde_json::json!({
        "success": true,
        "lead": {
            "uuid": lead_uuid.to_string(),
            "lead_id": lead_row.try_get::<Option<String>, _>("lead_id").ok().flatten(),
            "status": lead_row.try_get::<String, _>("status").ok(),
            "price": lead_price,
            "submitted_at": lead_row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("submitted_at").ok().flatten().map(|d| d.to_rfc3339()),
            "sold_at": lead_row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("sold_at").ok().flatten().map(|d| d.to_rfc3339()),
            "created_at": lead_row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("created_at").ok().flatten().map(|d| d.to_rfc3339()),
            "updated_at": lead_row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("updated_at").ok().flatten().map(|d| d.to_rfc3339()),
            "processing_time_ms": processing_time_ms,
            "publisher_name": lead_row.try_get::<Option<String>, _>("publisher_name").ok().flatten(),
            "buyer_name": lead_row.try_get::<Option<String>, _>("buyer_name").ok().flatten(),
            "pii": {
                "first_name": first_name,
                "last_name": last_name,
                "email": email,
                "street_address": street_address,
                "zip": zip,
                "ip_address": ip_address,
            },
            "ping_payloads": ping_payloads,
            "post_payload": post_payload,
            "auction_timing": auction_timing_full,
            "vertical_data": vertical_data_value,
        },
    });

    // #region agent log
    let log_entry = serde_json::json!({
        "sessionId": "debug-session",
        "runId": "run1",
        "hypothesisId": "E",
        "location": "dashboard.rs:6724",
        "message": "Lead details response summary",
        "data": {
            "lead_uuid": lead_uuid.to_string(),
            "ping_payloads_count": ping_payloads.len(),
            "post_payload_exists": post_payload.is_some(),
            "lead_price": lead_price,
            "processing_time_ms": processing_time_ms,
            "first_name": first_name.clone(),
            "last_name": last_name.clone(),
            "email": email.clone()
        },
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/home/badinoff/projects/leadsnebula/.cursor/debug.log")
    {
        use std::io::Write;
        let _ = writeln!(
            file,
            "{}",
            serde_json::to_string(&log_entry).unwrap_or_default()
        );
    }
    // #endregion

    Ok(Json(response_json))
}
