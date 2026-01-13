// Note: This module is currently unused because we only serve minimal app with /live endpoint
// It will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use chrono::Utc;
use leadsnebula_core::models::enums::LeadStatus;
use leadsnebula_core::models::publisher::Publisher;
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::AppState;

fn map_error_to_user(err_text: &str) -> (String, String) {
    let lower = err_text.to_lowercase();
    // Build a list of friendly problem lines when we can detect multiple issues
    let mut problems: Vec<String> = Vec::new();

    if lower.contains("submitted_at") {
        problems.push("Server misconfiguration: required lead timestamp is missing. Contact the site administrator.".to_string());
    }
    if lower.contains("buyer_id") {
        problems.push("No buyer configured for this publisher/vertical".to_string());
    }
    if lower.contains("campaign_id") {
        problems.push("No campaign configured for this publisher/vertical".to_string());
    }
    if lower.contains("post_id") {
        problems.push("Post could not be created: required post identifier missing".to_string());
    }
    if lower.contains("permission denied") || lower.contains("permission") {
        problems.push("Server permission error. Contact the administrator.".to_string());
    }
    if lower.contains("violates not-null constraint") || lower.contains("null value") {
        // Generic not-null / null detection - add an explanatory line if nothing more specific matched
        if problems.is_empty() {
            problems.push(
                "Required data missing for this operation. Contact support or try again later."
                    .to_string(),
            );
        }
    }

    // Fallback message when no specific hints were found
    if problems.is_empty() {
        problems.push(
            "An internal server error occurred. Contact support if the problem persists."
                .to_string(),
        );
    }

    // Join each problem on its own line
    let friendly = problems.join("\n");

    // Technical message: keep original but prefix with source for clarity
    let technical = format!("error returned from database: {}", err_text);

    (friendly, technical)
}

#[derive(Deserialize, Serialize)]
pub struct LeadRequest {
    pub verbose: Option<bool>,
    pub lead: LeadData,
}

#[derive(Deserialize, Serialize, Clone)]
#[allow(dead_code)] // Fields will be used as implementation expands
pub struct LeadData {
    pub publisher_id: Option<String>,
    pub vertical: String,
    pub request_type: Option<String>,
    pub campaign_token: Option<String>,
    pub promise_id: Option<String>,
    pub lead_id: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub cell_phone: Option<String>,
    pub mobile_phone: Option<String>,
    pub street_address: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub zip: Option<String>,
    pub monthly_bill: Option<f64>,
    pub credit_rating: Option<String>,
    pub own_home: Option<bool>,
    pub property_type: Option<String>,
    pub roof_shade: Option<String>,
    pub roof_type: Option<String>,
    pub utility_provider: Option<String>,
    pub purchase_timeframe: Option<String>,
    pub ip_address: Option<String>,
    pub tcpa_consent: Option<bool>,
    pub tcpa_language: Option<String>,
    pub jornaya_lead_id: Option<String>,
    pub trusted_form_url: Option<String>,
    pub is_test: Option<bool>,
    pub verbose: Option<bool>,
}

#[derive(Serialize)]
pub struct StatusNode {
    pub success: bool,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct LeadNode {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promise_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ping_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bid: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
}

#[derive(Serialize)]
pub struct LeadResponse {
    // Preserve order: status, lead, verbose
    pub status: StatusNode,
    pub lead: LeadNode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbose: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
}

pub fn carina_routes() -> Router<AppState> {
    Router::new().route("/api/v1/leads", post(create_lead))
}

async fn create_lead(
    State(state): State<AppState>,
    Extension(publisher): Extension<Publisher>,
    Json(payload): Json<LeadRequest>,
) -> Result<Json<LeadResponse>, StatusCode> {
    let request_level_verbose = payload.verbose.unwrap_or(false);
    let lead_data = payload.lead;
    let request_type = lead_data
        .request_type
        .as_deref()
        .unwrap_or("ping")
        .to_lowercase();
    let verbose_requested = lead_data.verbose.unwrap_or(request_level_verbose);

    // Validate vertical
    let vertical = match leadsnebula_core::models::vertical::Vertical::find_by_slug(
        &state.db_pool,
        &lead_data.vertical,
    )
    .await
    {
        Ok(Some(v)) => v,
        Ok(None) => {
            return Ok(Json(LeadResponse {
                status: StatusNode {
                    success: false,
                    status: "error".to_string(),
                    message: Some(format!("Invalid vertical: {}", lead_data.vertical)),
                    error: Some(format!("Invalid vertical slug: {}", lead_data.vertical)),
                },
                lead: LeadNode {
                    promise_id: None,
                    lead_id: None,
                    lead_uuid: None,
                    ping_id: None,
                    bid: None,
                    post_id: None,
                    price: None,
                },
                verbose: if verbose_requested {
                    Some(serde_json::json!({
                        "error_code": "ERR_400",
                        "timestamp": Utc::now().to_rfc3339(),
                        "endpoint": "POST /api/v1/leads",
                        "status_code": 400
                    }))
                } else {
                    None
                },
                http_status: Some(400),
            }));
        }
        Err(e) => {
            tracing::error!("Database error finding vertical: {}", e);
            let (message, technical) = map_error_to_user(&e.to_string());
            return Ok(Json(LeadResponse {
                status: StatusNode {
                    success: false,
                    status: "error".to_string(),
                    message: Some(message),
                    error: Some(technical),
                },
                lead: LeadNode {
                    promise_id: None,
                    lead_id: None,
                    lead_uuid: None,
                    ping_id: None,
                    bid: None,
                    post_id: None,
                    price: None,
                },
                verbose: if verbose_requested {
                    Some(serde_json::json!({
                        "error_code": "ERR_500",
                        "timestamp": Utc::now().to_rfc3339(),
                        "endpoint": "POST /api/v1/leads",
                        "status_code": 500
                    }))
                } else {
                    None
                },
                http_status: Some(500),
            }));
        }
    };

    // Handle post request (update existing lead)
    if request_type == "post" {
        let promise_id = match lead_data.promise_id {
            Some(ref p) => p.clone(),
            None => {
                return Ok(Json(LeadResponse {
                    status: StatusNode {
                        success: false,
                        status: "error".to_string(),
                        message: None,
                        error: Some("Missing promise_id for post request".to_string()),
                    },
                    lead: LeadNode {
                        promise_id: None,
                        lead_id: None,
                        lead_uuid: None,
                        ping_id: None,
                        bid: None,
                        post_id: None,
                        price: None,
                    },
                    verbose: if verbose_requested {
                        Some(serde_json::json!({
                            "error_code": "ERR_400",
                            "timestamp": Utc::now().to_rfc3339(),
                            "endpoint": "POST /api/v1/leads",
                            "status_code": 400,
                            "note": "Post requests must include a promise_id"
                        }))
                    } else {
                        None
                    },
                    http_status: Some(400),
                }));
            }
        };
        let lead = match leadsnebula_core::models::lead::Lead::find_by_promise_id(
            &state.db_pool,
            &promise_id,
        )
        .await
        {
            Ok(Some(l)) => l,
            Ok(None) => {
                return Ok(Json(LeadResponse {
                    status: StatusNode {
                        success: false,
                        status: "error".to_string(),
                        message: None,
                        error: Some("Lead not found".to_string()),
                    },
                    lead: LeadNode {
                        promise_id: None,
                        lead_id: None,
                        lead_uuid: None,
                        ping_id: None,
                        bid: None,
                        post_id: None,
                        price: None,
                    },
                    verbose: if verbose_requested {
                        Some(serde_json::json!({
                            "error_code": format!("ERR_{}", 404),
                            "timestamp": Utc::now().to_rfc3339(),
                            "endpoint": "POST /api/v1/leads",
                            "status_code": 404
                        }))
                    } else {
                        None
                    },
                    http_status: Some(404),
                }));
            }
            Err(e) => {
                tracing::error!("Database error finding lead: {}", e);
                let (message, technical) = map_error_to_user(&e.to_string());
                return Ok(Json(LeadResponse {
                    status: StatusNode {
                        success: false,
                        status: "error".to_string(),
                        message: Some(message),
                        error: Some(technical),
                    },
                    lead: LeadNode {
                        promise_id: None,
                        lead_id: None,
                        lead_uuid: None,
                        ping_id: None,
                        bid: None,
                        post_id: None,
                        price: None,
                    },
                    verbose: if verbose_requested {
                        Some(serde_json::json!({
                            "error_code": "ERR_500",
                            "timestamp": Utc::now().to_rfc3339(),
                            "endpoint": "POST /api/v1/leads",
                            "status_code": 500
                        }))
                    } else {
                        None
                    },
                    http_status: Some(500),
                }));
            }
        };

        // Attempt an atomic conditional claim to prevent double-sell.
        // We set a temporary in-progress token into `post_id` only if it's empty and the promise is not expired.
        let inprog_token = format!("INPROG_{}", uuid::Uuid::new_v4());
        let claim_result = sqlx::query_scalar::<_, uuid::Uuid>(
            "UPDATE leads SET post_id = $1 WHERE uuid = $2 AND (post_id IS NULL OR post_id = '') AND promise_id = $3 AND created_at >= NOW() - INTERVAL '10 minutes' RETURNING uuid",
        )
        .bind(inprog_token.clone())
        .bind(lead.uuid)
        .bind(&promise_id)
        .fetch_optional(&*state.db_pool)
        .await;

        match claim_result {
            Ok(Some(_)) => {
                // We have claimed this promise for this process. Proceed to route the post.
            }
            Ok(None) => {
                return Ok(Json(LeadResponse {
                    status: StatusNode {
                        success: false,
                        status: "error".to_string(),
                        message: Some(
                            "Lead has already been posted/sold or promise expired".to_string(),
                        ),
                        error: Some("duplicate_or_expired_promise".to_string()),
                    },
                    lead: LeadNode {
                        promise_id: None,
                        lead_id: lead.lead_id.clone(),
                        lead_uuid: Some(lead.uuid.to_string()),
                        ping_id: lead.ping_id.clone(),
                        bid: None,
                        post_id: lead.post_id.clone(),
                        price: None,
                    },
                    verbose: if verbose_requested {
                        Some(serde_json::json!({
                            "error_code": "ERR_400",
                            "timestamp": Utc::now().to_rfc3339(),
                            "endpoint": "POST /api/v1/leads",
                            "status_code": 400,
                            "note": "Duplicate post attempt or promise expired"
                        }))
                    } else {
                        None
                    },
                    http_status: Some(400),
                }));
            }
            Err(e) => {
                tracing::error!("Database error claiming promise: {}", e);
                let (message, technical) = map_error_to_user(&e.to_string());
                return Ok(Json(LeadResponse {
                    status: StatusNode {
                        success: false,
                        status: "error".to_string(),
                        message: Some(message),
                        error: Some(technical),
                    },
                    lead: LeadNode {
                        promise_id: None,
                        lead_id: None,
                        lead_uuid: None,
                        ping_id: None,
                        bid: None,
                        post_id: None,
                        price: None,
                    },
                    verbose: if verbose_requested {
                        Some(serde_json::json!({
                            "error_code": "ERR_500",
                            "timestamp": Utc::now().to_rfc3339(),
                            "endpoint": "POST /api/v1/leads",
                            "status_code": 500
                        }))
                    } else {
                        None
                    },
                    http_status: Some(500),
                }));
            }
        }

        // Route the post through the ping-tree router to perform buyer post handling
        let router = leadsnebula_core::services::ping_tree_router::PingTreeRouter::new(
            lead.clone(),
            publisher.id,
            vertical.slug.clone(),
            request_type.clone(),
        );
        let start_processing = std::time::Instant::now();
        match router
            .route(
                state.db_pool.clone(),
                std::sync::Arc::new(state.config.encryption_key.clone()),
            )
            .await
        {
            Ok(routing_result) => {
                // Batch load buyer and campaign names in parallel (performance optimization)
                let (buyer_name, campaign_name) = tokio::join!(
                    async {
                        if let Some(bid) = routing_result.buyer_id {
                            sqlx::query_scalar::<_, String>(
                                "SELECT name FROM buyers WHERE id = $1 AND deleted_at IS NULL",
                            )
                            .bind(bid)
                            .fetch_optional(&*state.db_pool)
                            .await
                            .unwrap_or_default()
                        } else {
                            None
                        }
                    },
                    async {
                        if let Some(cid) = routing_result.campaign_id {
                            sqlx::query_scalar::<_, String>(
                                "SELECT name FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
                            )
                            .bind(cid)
                            .fetch_optional(&*state.db_pool)
                            .await
                            .unwrap_or_default()
                        } else {
                            None
                        }
                    }
                );

                // Round price to 2 decimals for response
                let rounded_price = routing_result.price.map(|p| (p * 100.0).round() / 100.0);
                let processing_time_ms = start_processing.elapsed().as_millis() as u64;

                // Build status node/message
                let success = routing_result.success;
                let status = routing_result.status.clone();
                let message = if status == "sold" {
                    if let Some(name) = buyer_name.clone() {
                        if let Some(p) = rounded_price {
                            Some(format!("Lead sold to {} for {}", name, p))
                        } else {
                            Some(format!("Lead sold to {}", name))
                        }
                    } else if let Some(p) = rounded_price {
                        Some(format!("Lead sold for {}", p))
                    } else {
                        Some("Lead sold".to_string())
                    }
                } else {
                    routing_result
                        .error
                        .clone()
                        .or_else(|| routing_result.status.clone().into())
                };

                // Persist post payload (request + response) into post_payloads with encryption when possible
                let post_request_json =
                    serde_json::to_value(&lead_data).unwrap_or_else(|_| serde_json::json!({}));
                let post_response_json = serde_json::json!({
                    "routing_result": {
                        "status": routing_result.status,
                        "success": routing_result.success,
                        "error": routing_result.error,
                        "price": routing_result.price,
                        "buyer_id": routing_result.buyer_id.map(|b| b.to_string()),
                        "campaign_id": routing_result.campaign_id.map(|c| c.to_string()),
                        "ping_id": routing_result.ping_id,
                        "post_id": routing_result.post_id,
                        "promise_id": routing_result.promise_id,
                    }
                });

                // Try to encrypt using SSM deterministic key
                let env_norm2 =
                    leadsnebula_core::normalize_env_for_ssm(&state.config.environment).to_string();
                let det_path2 = format!(
                    "/leadsnebula/{}/carina/encryption/deterministic_key_v1",
                    env_norm2
                );
                let salt_path2 = format!(
                    "/leadsnebula/{}/carina/encryption/key_derivation_salt_v1",
                    env_norm2
                );
                let mut enc_req_opt: Option<String> = None;
                let mut enc_resp_opt: Option<String> = None;
                if let Ok(Some(det_key)) = state.ssm.get_parameter(&det_path2, true).await {
                    if let Ok(Some(salt)) = state.ssm.get_parameter(&salt_path2, true).await {
                        let derived =
                            leadsnebula_core::encryption::EncryptionService::derive_key_from_secret(
                                &det_key, &salt,
                            );
                        if let Ok(req_str) = serde_json::to_string(&post_request_json) {
                            if let Ok(env) =
                                leadsnebula_core::encryption::EncryptionService::encrypt_envelope(
                                    &derived, &req_str, true,
                                )
                            {
                                enc_req_opt = Some(env);
                            }
                        }
                        if let Ok(resp_str) = serde_json::to_string(&post_response_json) {
                            if let Ok(env2) =
                                leadsnebula_core::encryption::EncryptionService::encrypt_envelope(
                                    &derived, &resp_str, true,
                                )
                            {
                                enc_resp_opt = Some(env2);
                            }
                        }
                    }
                }

                // Insert into post_payloads
                // Clone post_id once for both branches
                let post_id_for_insert = routing_result.post_id.clone();
                let _ = if let (Some(er), Some(epr)) = (enc_req_opt, enc_resp_opt) {
                    sqlx::query("INSERT INTO post_payloads (lead_id, post_id, payload, request_payload_encrypted, response_payload_encrypted, created_at) VALUES ($1, $2, $3, $4, $5, now())")
                        .bind(lead.uuid)
                        .bind(&post_id_for_insert)
                        .bind(sqlx::types::Json(&post_request_json))
                        .bind(er)
                        .bind(epr)
                        .execute(&*state.db_pool)
                        .await
                } else {
                    sqlx::query("INSERT INTO post_payloads (lead_id, post_id, payload, created_at) VALUES ($1, $2, $3, now())")
                        .bind(lead.uuid)
                        .bind(&post_id_for_insert)
                        .bind(sqlx::types::Json(&post_request_json))
                        .execute(&*state.db_pool)
                        .await
                };

                // Encrypt any buyer_responses rows for this lead/post_id using SSM deterministic key (best-effort)
                // Move to async background task to avoid blocking response
                if let Some(post_id_val) = routing_result.post_id.clone() {
                    let pool_clone = state.db_pool.clone();
                    let ssm_clone = state.ssm.clone();
                    let env_clone = state.config.environment.clone();
                    let lead_uuid_clone = lead.uuid;

                    tokio::spawn(async move {
                        if let Ok(rows) = sqlx::query("SELECT id, payload FROM buyer_responses WHERE lead_id = $1 AND post_id = $2 AND response_payload_encrypted IS NULL")
                                .bind(lead_uuid_clone)
                                .bind(post_id_val)
                                .fetch_all(&*pool_clone)
                                .await
                            {
                                let env_norm = leadsnebula_core::normalize_env_for_ssm(&env_clone).to_string();
                                let det_path = format!("/leadsnebula/{}/carina/encryption/deterministic_key_v1", env_norm);
                                let salt_path = format!("/leadsnebula/{}/carina/encryption/key_derivation_salt_v1", env_norm);
                                if let Ok(Some(det_key)) = ssm_clone.get_parameter(&det_path, true).await {
                                    if let Ok(Some(salt)) = ssm_clone.get_parameter(&salt_path, true).await {
                                        let derived = leadsnebula_core::encryption::EncryptionService::derive_key_from_secret(&det_key, &salt);
                                        for r in rows {
                                            let id: i64 = r.get("id");
                                            let payload_val: serde_json::Value = r.get("payload");
                                            if let Ok(payload_str) = serde_json::to_string(&payload_val) {
                                                if let Ok(envelope) = leadsnebula_core::encryption::EncryptionService::encrypt_envelope(&derived, &payload_str, true) {
                                                    let _ = sqlx::query("UPDATE buyer_responses SET response_payload_encrypted = $1 WHERE id = $2")
                                                        .bind(envelope)
                                                        .bind(id)
                                                        .execute(&*pool_clone)
                                                        .await;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                    });
                }

                // Update lead final state: if sold, set post_id and status sold; if not, clear in-progress placeholder
                if routing_result.success && routing_result.status == "sold" {
                    // Set final post_id and mark sold only if our in-progress token is still present
                    let _ = sqlx::query("UPDATE leads SET post_id = $1, status = $2, sold_at = NOW(), updated_at = NOW() WHERE uuid = $3 AND post_id = $4")
                        .bind(routing_result.post_id.clone())
                        .bind(leadsnebula_core::models::enums::LeadStatus::Sold)
                        .bind(lead.uuid)
                        .bind(inprog_token.clone())
                        .execute(&*state.db_pool)
                        .await;
                } else {
                    // Reset placeholder so another post attempt may try
                    let _ = sqlx::query(
                        "UPDATE leads SET post_id = '' WHERE uuid = $1 AND post_id = $2",
                    )
                    .bind(lead.uuid)
                    .bind(inprog_token.clone())
                    .execute(&*state.db_pool)
                    .await;
                }
                return Ok(Json(LeadResponse {
                    status: StatusNode {
                        success,
                        status,
                        message,
                        error: routing_result.error.clone(),
                    },
                    lead: LeadNode {
                        promise_id: None,
                        lead_id: lead.lead_id.clone(),
                        lead_uuid: Some(lead.uuid.to_string()),
                        ping_id: None,
                        bid: None,
                        post_id: routing_result.post_id.clone(),
                        price: rounded_price,
                    },
                    verbose: if verbose_requested {
                        Some(serde_json::json!({
                            "error_code": format!("ERR_{}", if success {200} else {500}),
                            "timestamp": Utc::now().to_rfc3339(),
                            "endpoint": "POST /api/v1/leads",
                            "status_code": if success {200} else {500},
                            "routing": {
                                "processing_time_ms": processing_time_ms,
                                "buyer_name": buyer_name,
                                "buyer_id": routing_result.buyer_id.map(|b| b.to_string()),
                                "campaign_name": campaign_name,
                                "campaign_id": routing_result.campaign_id.map(|c| c.to_string())
                            }
                        }))
                    } else {
                        None
                    },
                    http_status: Some(if success { 200 } else { 500 }),
                }));
            }
            Err(e) => {
                tracing::error!("Routing error during post: {}", e);
                let (message, technical) = map_error_to_user(&e.to_string());
                return Ok(Json(LeadResponse {
                    status: StatusNode {
                        success: false,
                        status: "error".to_string(),
                        message: Some(message),
                        error: Some(technical),
                    },
                    lead: LeadNode {
                        promise_id: None,
                        lead_id: None,
                        lead_uuid: None,
                        ping_id: None,
                        bid: None,
                        post_id: None,
                        price: None,
                    },
                    verbose: if verbose_requested {
                        Some(serde_json::json!({
                            "error_code": "ERR_500",
                            "timestamp": Utc::now().to_rfc3339(),
                            "endpoint": "POST /api/v1/leads",
                            "status_code": 500
                        }))
                    } else {
                        None
                    },
                    http_status: Some(500),
                }));
            }
        }
    }

    // Create new lead (ping/fullpost)
    // Determine strategy to satisfy DB check constraint
    let strategy = match request_type.as_str() {
        "ping" | "post" => "pingPost",
        "fullpost" => "fullPost",
        _ => "pingPost",
    };

    // Pre-insert checks: determine buyer_id and campaign_id to satisfy NOT NULL constraints
    let mut preproblems: Vec<String> = Vec::new();
    let mut buyer_id_opt: Option<uuid::Uuid> = None;
    let mut campaign_id_opt: Option<uuid::Uuid> = None;

    // If a campaign_token was provided, prefer that campaign
    if let Some(ref token) = lead_data.campaign_token {
        match sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT id FROM campaigns WHERE campaign_token = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(token)
        .fetch_optional(&*state.db_pool)
        .await
        {
            Ok(Some(cid)) => campaign_id_opt = Some(cid),
            Ok(None) => {
                preproblems.push("No campaign configured for this publisher/vertical".to_string())
            }
            Err(e) => {
                tracing::error!("Error checking campaigns by token: {}", e);
                preproblems.push("Failed to verify campaigns due to server error".to_string());
            }
        }
    }

    // If campaign not provided/found, try to find any campaign linked to buyers for this vertical
    if campaign_id_opt.is_none() {
        match sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT id FROM campaigns WHERE buyer_id IN (SELECT id FROM buyers WHERE vertical_id = $1 AND deleted_at IS NULL) AND deleted_at IS NULL LIMIT 1",
        )
        .bind(vertical.id)
        .fetch_optional(&*state.db_pool)
        .await
        {
            Ok(Some(cid)) => campaign_id_opt = Some(cid),
            Ok(None) => {
                // don't add problem yet; we'll surface buyer/campaign problems after buyer check
            }
            Err(e) => {
                tracing::error!("Error checking campaigns fallback: {}", e);
                preproblems.push("Failed to verify campaigns due to server error".to_string());
            }
        }
    }

    // Determine buyer: prefer campaign's buyer if we have a campaign, otherwise find any buyer for the vertical
    if let Some(cid) = campaign_id_opt {
        match sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT buyer_id FROM campaigns WHERE id = $1 LIMIT 1",
        )
        .bind(cid)
        .fetch_one(&*state.db_pool)
        .await
        {
            Ok(bid) => buyer_id_opt = Some(bid),
            Err(e) => {
                tracing::error!("Error resolving buyer from campaign: {}", e);
                preproblems.push("Failed to resolve buyer for campaign".to_string());
            }
        }
    } else {
        match sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT id FROM buyers WHERE vertical_id = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(vertical.id)
        .fetch_optional(&*state.db_pool)
        .await
        {
            Ok(Some(bid)) => buyer_id_opt = Some(bid),
            Ok(None) => {
                preproblems.push("No buyer configured for this publisher/vertical".to_string())
            }
            Err(e) => {
                tracing::error!("Error checking buyers: {}", e);
                preproblems.push("Failed to verify buyers due to server error".to_string());
            }
        }
    }

    // Attempt to check ping tree presence (best-effort; table may not exist in all installs)
    // Ping tree presence is helpful but not mandatory here; routing will return a clear error if absent
    match sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM ping_trees WHERE publisher_id = $1 AND vertical = $2 AND deleted_at IS NULL LIMIT 1")
        .bind(publisher.id)
        .bind(vertical.slug.clone())
        .fetch_optional(&*state.db_pool)
        .await
    {
        Ok(Some(_)) => {}
        Ok(None) => tracing::info!("No ping tree configured for this publisher/vertical; routing may fail"),
        Err(e) => tracing::debug!("Ping tree check skipped or failed: {}", e),
    }

    if !preproblems.is_empty() {
        let message = preproblems.join("\n");
        return Ok(Json(LeadResponse {
            status: StatusNode {
                success: false,
                status: "error".to_string(),
                message: Some(message.clone()),
                error: Some(format!(
                    "pre-check failures: {}",
                    message.replace("\n", ", ")
                )),
            },
            lead: LeadNode {
                promise_id: None,
                lead_id: None,
                lead_uuid: None,
                ping_id: None,
                bid: None,
                post_id: None,
                price: None,
            },
            verbose: if verbose_requested {
                Some(serde_json::json!({
                    "error_code": "ERR_400",
                    "timestamp": Utc::now().to_rfc3339(),
                    "endpoint": "POST /api/v1/leads",
                    "status_code": 400
                }))
            } else {
                None
            },
            http_status: Some(400),
        }));
    }

    // Generate identifiers only after pre-checks pass
    let lead_id = lead_data.lead_id.clone().unwrap_or_else(|| {
        let prefix = vertical.slug.to_uppercase();
        let rand_str: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(8)
            .map(char::from)
            .collect::<String>()
            .to_uppercase();
        format!("{}-{}", prefix, rand_str)
    });

    let event_id = format!("evt_{}", uuid::Uuid::new_v4());

    // Generate promise_id for ping requests
    let promise_id = if request_type == "ping" || request_type == "fullpost" {
        Some(format!(
            "PROMISE_{}",
            hex::encode(rand::random::<[u8; 6]>()).to_uppercase()
        ))
    } else {
        None
    };

    // Insert lead into database
    tracing::info!(
        "Creating lead: event_id={}, lead_id={}, publisher_id={}, vertical_id={}, request_type={}",
        event_id,
        lead_id,
        publisher.id,
        vertical.id,
        request_type
    );

    let result = sqlx::query(
        r#"
        INSERT INTO leads (
            event_id, lead_id, publisher_id, vertical_id, request_type, strategy, status,
            promise_id, tcpa_consent, tcpa_language, is_test, session_id, vertical_data,
                buyer_id, campaign_id, post_id, submitted_at, created_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
            $14, $15, $16, NOW(), NOW()
        ) RETURNING uuid
        "#,
    )
    .bind(&event_id)
    .bind(if lead_id.is_empty() {
        None
    } else {
        Some(&lead_id)
    })
    .bind(publisher.id)
    .bind(vertical.id)
    .bind(&request_type)
    .bind(strategy)
    .bind(LeadStatus::Processing)
    .bind(&promise_id)
    .bind(lead_data.tcpa_consent.unwrap_or(false))
    .bind(lead_data.tcpa_language.as_deref().unwrap_or(""))
    .bind(lead_data.is_test.unwrap_or(false))
    // Provide a session_id to satisfy DB NOT NULL constraints; prefer incoming header if available
    .bind(format!("sess_{}", uuid::Uuid::new_v4()).as_str())
    .bind(serde_json::json!({}))
    // Bind resolved buyer_id and campaign_id (pre-check ensures these exist)
    .bind(buyer_id_opt.expect("buyer_id must be present after pre-checks"))
    .bind(campaign_id_opt.expect("campaign_id must be present after pre-checks"))
    // Provide a placeholder post_id to satisfy NOT NULL constraint for legacy installs
    .bind("")
    .fetch_one(&*state.db_pool)
    .await;

    let lead_uuid = match result {
        Ok(row) => row.get::<uuid::Uuid, _>(0),
        Err(e) => {
            tracing::error!("Database error creating lead: {}", e);
            tracing::error!("Lead data: event_id={}, lead_id={}, publisher_id={}, vertical_id={}, request_type={}", 
                event_id, lead_id, publisher.id, vertical.id, request_type);
            let (message, technical) = map_error_to_user(&e.to_string());
            return Ok(Json(LeadResponse {
                status: StatusNode {
                    success: false,
                    status: "error".to_string(),
                    message: Some(message),
                    error: Some(technical),
                },
                // Do not expose generated identifiers when creation failed due to configuration
                lead: LeadNode {
                    promise_id: None,
                    lead_id: None,
                    lead_uuid: None,
                    ping_id: None,
                    bid: None,
                    post_id: None,
                    price: None,
                },
                verbose: if verbose_requested {
                    Some(serde_json::json!({
                        "error_code": "ERR_500",
                        "timestamp": Utc::now().to_rfc3339(),
                        "endpoint": "POST /api/v1/leads",
                        "status_code": 500
                    }))
                } else {
                    None
                },
                http_status: Some(500),
            }));
        }
    };

    // Persist incoming request payload into ping_payloads for later inspection.
    let request_payload_json =
        serde_json::to_value(&lead_data).unwrap_or_else(|_| serde_json::json!({}));
    // Try to encrypt the request payload using Rails-compatible keys from SSM (deterministic)
    let mut encrypted_request_opt: Option<String> = None;
    let env_norm = leadsnebula_core::normalize_env_for_ssm(&state.config.environment).to_string();
    let det_path = format!(
        "/leadsnebula/{}/carina/encryption/deterministic_key_v1",
        env_norm
    );
    let salt_path = format!(
        "/leadsnebula/{}/carina/encryption/key_derivation_salt_v1",
        env_norm
    );
    if let Ok(Some(det_key)) = state.ssm.get_parameter(&det_path, true).await {
        if let Ok(Some(salt)) = state.ssm.get_parameter(&salt_path, true).await {
            if let Ok(derived) = std::panic::catch_unwind(|| {
                leadsnebula_core::encryption::EncryptionService::derive_key_from_secret(
                    &det_key, &salt,
                )
            }) {
                let key_bytes: [u8; 32] = derived;
                if let Ok(req_str) = serde_json::to_string(&request_payload_json) {
                    if let Ok(envelope) =
                        leadsnebula_core::encryption::EncryptionService::encrypt_envelope(
                            &key_bytes, &req_str, true,
                        )
                    {
                        encrypted_request_opt = Some(envelope);
                    }
                }
            }
        }
    }

    let inserted_payload = if let Some(encrypted_request) = encrypted_request_opt {
        sqlx::query("INSERT INTO ping_payloads (lead_id, payload, request_payload_encrypted, created_at) VALUES ($1, $2, $3, now()) RETURNING id")
            .bind(lead_uuid)
            .bind(sqlx::types::Json(&request_payload_json))
            .bind(encrypted_request)
            .fetch_one(&*state.db_pool)
            .await
    } else {
        sqlx::query("INSERT INTO ping_payloads (lead_id, payload, created_at) VALUES ($1, $2, now()) RETURNING id")
            .bind(lead_uuid)
            .bind(sqlx::types::Json(&request_payload_json))
            .fetch_one(&*state.db_pool)
            .await
    };

    // Capture payload row id if insert succeeded (best-effort)
    let payload_row_id: Option<uuid::Uuid> = match inserted_payload {
        Ok(r) => r.get::<uuid::Uuid, _>("id").into(),
        Err(_) => None,
    };

    // Load the created lead
    let lead = match sqlx::query_as::<_, leadsnebula_core::models::lead::Lead>(
        "SELECT * FROM leads WHERE uuid = $1",
    )
    .bind(lead_uuid)
    .fetch_one(&*state.db_pool)
    .await
    {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to load created lead: {}", e);
            return Ok(Json(LeadResponse {
                status: StatusNode {
                    success: false,
                    status: "error".to_string(),
                    message: Some("Failed to load created lead".to_string()),
                    error: Some(format!("Database error: {}", e)),
                },
                lead: LeadNode {
                    promise_id,
                    lead_id: Some(lead_id),
                    lead_uuid: Some(lead_uuid.to_string()),
                    ping_id: None,
                    bid: None,
                    post_id: None,
                    price: None,
                },
                verbose: if verbose_requested {
                    Some(serde_json::json!({
                        "error_code": format!("ERR_{}", 500),
                        "timestamp": Utc::now().to_rfc3339(),
                        "endpoint": "POST /api/v1/leads",
                        "status_code": 500
                    }))
                } else {
                    None
                },
                http_status: Some(500),
            }));
        }
    };

    // Route the lead through ping tree
    let router = leadsnebula_core::services::ping_tree_router::PingTreeRouter::new(
        lead,
        publisher.id,
        vertical.slug.clone(),
        request_type.clone(),
    );

    let start_processing = std::time::Instant::now();
    match router
        .route(
            state.db_pool.clone(),
            std::sync::Arc::new(state.config.encryption_key.clone()),
        )
        .await
    {
        Ok(routing_result) => {
            let processing_time_ms = start_processing.elapsed().as_millis() as u64;
            // Determine a clearer message and include price when available
            // Helper to round to 2 decimals
            fn round2(v: Option<f64>) -> Option<f64> {
                v.map(|p| (p * 100.0).round() / 100.0)
            }

            // Prepare bid/price rounded values
            let mut bid: Option<f64> = None;
            let mut price: Option<f64> = None;
            if routing_result.status == "accepted" {
                bid = round2(routing_result.price);
            } else if routing_result.status == "sold" {
                price = round2(routing_result.price);
            }

            // Build a clearer message. For accepted pings include winning bid when available.
            // Note: Buyer name lookup removed to avoid blocking response (performance optimization)
            // Buyer name can be included in verbose response if needed
            let message = if routing_result.status == "accepted" {
                if let Some(b) = bid {
                    Some(format!("Ping Accepted with a bid of ${:.2}", b))
                } else {
                    Some("Ping Accepted".to_string())
                }
            } else if routing_result.status == "sold" {
                if let Some(p) = price {
                    Some(format!("Lead Sold for {}", p))
                } else {
                    Some("Lead Sold".to_string())
                }
            } else if routing_result.success {
                Some("Lead routed successfully".to_string())
            } else {
                None
            };

            // Resolve buyer and campaign names if available (best-effort; ignore failures)
            let buyer_name = if let Some(bid) = routing_result.buyer_id {
                (sqlx::query_scalar::<_, String>(
                    "SELECT name FROM buyers WHERE id = $1 AND deleted_at IS NULL",
                )
                .bind(bid)
                .fetch_optional(&*state.db_pool)
                .await)
                    .unwrap_or_default()
            } else {
                None
            };

            let campaign_name = if let Some(cid) = routing_result.campaign_id {
                (sqlx::query_scalar::<_, String>(
                    "SELECT name FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
                )
                .bind(cid)
                .fetch_optional(&*state.db_pool)
                .await)
                    .unwrap_or_default()
            } else {
                None
            };

            let verbose_json = if verbose_requested {
                Some(serde_json::json!({
                    "error_code": format!("ERR_{}", 200),
                    "timestamp": Utc::now().to_rfc3339(),
                    "endpoint": "POST /api/v1/leads",
                    "status_code": 200,
                    "routing": {
                        "processing_time_ms": processing_time_ms,
                        "buyer_name": buyer_name,
                        "buyer_id": routing_result.buyer_id.map(|b| b.to_string()),
                        "campaign_name": campaign_name,
                        "campaign_id": routing_result.campaign_id.map(|c| c.to_string())
                    }
                }))
            } else {
                None
            };

            // Update the ping_payloads row with the routing result as response_payload and external id
            if let Some(row_id) = payload_row_id {
                let response_json = serde_json::json!({
                    "routing_result": {
                        "status": routing_result.status,
                        "success": routing_result.success,
                        "error": routing_result.error,
                        "price": routing_result.price,
                        "buyer_id": routing_result.buyer_id.map(|b| b.to_string()),
                        "campaign_id": routing_result.campaign_id.map(|c| c.to_string()),
                        "ping_id": routing_result.ping_id,
                        "post_id": routing_result.post_id,
                        "promise_id": routing_result.promise_id,
                    }
                });

                // Try to encrypt the response as well
                let mut encrypted_response_opt: Option<String> = None;
                if let Ok(Some(det_key)) = state.ssm.get_parameter(&det_path, true).await {
                    if let Ok(Some(salt)) = state.ssm.get_parameter(&salt_path, true).await {
                        let derived =
                            leadsnebula_core::encryption::EncryptionService::derive_key_from_secret(
                                &det_key, &salt,
                            );
                        if let Ok(resp_str) = serde_json::to_string(&response_json) {
                            if let Ok(envelope) =
                                leadsnebula_core::encryption::EncryptionService::encrypt_envelope(
                                    &derived, &resp_str, true,
                                )
                            {
                                encrypted_response_opt = Some(envelope);
                            }
                        }
                    }
                }

                // Clone ping_id once for both branches
                let ping_id_for_update = routing_result.ping_id.clone();
                if let Some(encrypted_response) = encrypted_response_opt {
                    let _ = sqlx::query("UPDATE ping_payloads SET payload = COALESCE(payload, 'null'::jsonb), response_payload_encrypted = $1, external_ping_id = $2, updated_at = now() WHERE id = $3")
                            .bind(encrypted_response)
                            .bind(&ping_id_for_update)
                            .bind(row_id)
                            .execute(&*state.db_pool)
                            .await;
                } else {
                    let _ = sqlx::query("UPDATE ping_payloads SET payload = COALESCE(payload, 'null'::jsonb), response_payload = $1, external_ping_id = $2, updated_at = now() WHERE id = $3")
                            .bind(sqlx::types::Json(&response_json))
                            .bind(&ping_id_for_update)
                            .bind(row_id)
                            .execute(&*state.db_pool)
                            .await;
                }

                // Encrypt any buyer_responses rows for this lead/ping_id using SSM deterministic key (best-effort)
                // Move to async background task to avoid blocking response
                if let Some(ping_id_val) = routing_result.ping_id.clone() {
                    let pool_clone = state.db_pool.clone();
                    let ssm_clone = state.ssm.clone();
                    let env_clone = state.config.environment.clone();
                    let lead_uuid_clone = lead_uuid;

                    let ping_id_for_log = ping_id_val.clone();
                    let lead_uuid_for_log = lead_uuid_clone;
                    let handle = tokio::spawn(async move {
                        if let Ok(rows) = sqlx::query("SELECT id, payload FROM buyer_responses WHERE lead_id = $1 AND ping_id = $2 AND response_payload_encrypted IS NULL")
                                .bind(lead_uuid_clone)
                                .bind(ping_id_val)
                                .fetch_all(&*pool_clone)
                                .await
                            {
                                // Try to load deterministic key/salt from SSM
                                let env_norm = leadsnebula_core::normalize_env_for_ssm(&env_clone).to_string();
                                let det_path = format!("/leadsnebula/{}/carina/encryption/deterministic_key_v1", env_norm);
                                let salt_path = format!("/leadsnebula/{}/carina/encryption/key_derivation_salt_v1", env_norm);
                                if let Ok(Some(det_key)) = ssm_clone.get_parameter(&det_path, true).await {
                                    if let Ok(Some(salt)) = ssm_clone.get_parameter(&salt_path, true).await {
                                        let derived = leadsnebula_core::encryption::EncryptionService::derive_key_from_secret(&det_key, &salt);
                                        for r in rows {
                                            let id: i64 = r.get("id");
                                            let payload_val: serde_json::Value = r.get("payload");
                                            if let Ok(payload_str) = serde_json::to_string(&payload_val) {
                                                if let Ok(envelope) = leadsnebula_core::encryption::EncryptionService::encrypt_envelope(&derived, &payload_str, true) {
                                                    let _ = sqlx::query("UPDATE buyer_responses SET response_payload_encrypted = $1 WHERE id = $2")
                                                        .bind(envelope)
                                                        .bind(id)
                                                        .execute(&*pool_clone)
                                                        .await;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                    });
                    // Log errors if task panics (fire-and-forget, but log for observability)
                    tokio::spawn(async move {
                        if let Err(e) = handle.await {
                            tracing::error!(
                                "Payload encryption task panicked for lead {} ping_id {}: {:?}",
                                lead_uuid_for_log,
                                ping_id_for_log,
                                e
                            );
                            #[cfg(feature = "sentry")]
                            {
                                sentry::capture_message(
                                    &format!("Payload encryption task panicked: {:?}", e),
                                    sentry::Level::Error,
                                );
                            }
                        }
                    });
                }
            }

            // For fullpost requests, also save post payloads if post_id is present
            if request_type == "fullpost" && routing_result.post_id.is_some() {
                let post_request_json =
                    serde_json::to_value(&lead_data).unwrap_or_else(|_| serde_json::json!({}));
                let post_response_json = serde_json::json!({
                    "routing_result": {
                        "status": routing_result.status.clone(),
                        "success": routing_result.success,
                        "error": routing_result.error.clone(),
                        "price": routing_result.price,
                        "buyer_id": routing_result.buyer_id.map(|b| b.to_string()),
                        "campaign_id": routing_result.campaign_id.map(|c| c.to_string()),
                        "ping_id": routing_result.ping_id.clone(),
                        "post_id": routing_result.post_id.clone(),
                        "promise_id": routing_result.promise_id.clone(),
                    }
                });

                // Try to encrypt using SSM deterministic key
                let env_norm_fp =
                    leadsnebula_core::normalize_env_for_ssm(&state.config.environment).to_string();
                let det_path_fp = format!(
                    "/leadsnebula/{}/carina/encryption/deterministic_key_v1",
                    env_norm_fp
                );
                let salt_path_fp = format!(
                    "/leadsnebula/{}/carina/encryption/key_derivation_salt_v1",
                    env_norm_fp
                );
                let mut enc_req_opt_fp: Option<String> = None;
                let mut enc_resp_opt_fp: Option<String> = None;
                if let Ok(Some(det_key)) = state.ssm.get_parameter(&det_path_fp, true).await {
                    if let Ok(Some(salt)) = state.ssm.get_parameter(&salt_path_fp, true).await {
                        let derived =
                            leadsnebula_core::encryption::EncryptionService::derive_key_from_secret(
                                &det_key, &salt,
                            );
                        if let Ok(req_str) = serde_json::to_string(&post_request_json) {
                            if let Ok(env) =
                                leadsnebula_core::encryption::EncryptionService::encrypt_envelope(
                                    &derived, &req_str, true,
                                )
                            {
                                enc_req_opt_fp = Some(env);
                            }
                        }
                        if let Ok(resp_str) = serde_json::to_string(&post_response_json) {
                            if let Ok(env2) =
                                leadsnebula_core::encryption::EncryptionService::encrypt_envelope(
                                    &derived, &resp_str, true,
                                )
                            {
                                enc_resp_opt_fp = Some(env2);
                            }
                        }
                    }
                }

                // Insert into post_payloads for fullpost
                // Clone post_id once for both branches
                let post_id_for_insert_fp = routing_result.post_id.clone();
                let _ = if let (Some(er), Some(epr)) = (enc_req_opt_fp, enc_resp_opt_fp) {
                    sqlx::query("INSERT INTO post_payloads (lead_id, post_id, payload, request_payload_encrypted, response_payload_encrypted, created_at) VALUES ($1, $2, $3, $4, $5, now())")
                        .bind(lead_uuid)
                        .bind(&post_id_for_insert_fp)
                        .bind(sqlx::types::Json(&post_request_json))
                        .bind(er)
                        .bind(epr)
                        .execute(&*state.db_pool)
                        .await
                } else {
                    sqlx::query("INSERT INTO post_payloads (lead_id, post_id, payload, created_at) VALUES ($1, $2, $3, now())")
                        .bind(lead_uuid)
                        .bind(&post_id_for_insert_fp)
                        .bind(sqlx::types::Json(&post_request_json))
                        .execute(&*state.db_pool)
                        .await
                };
            }

            Ok(Json(LeadResponse {
                status: StatusNode {
                    success: routing_result.success,
                    status: routing_result.status.clone(),
                    message,
                    error: routing_result.error.clone(),
                },
                lead: LeadNode {
                    promise_id: routing_result.promise_id.clone(),
                    lead_id: Some(lead_id),
                    lead_uuid: Some(lead_uuid.to_string()),
                    ping_id: None,
                    bid,
                    post_id: routing_result.post_id.clone(),
                    price,
                },
                verbose: verbose_json,
                http_status: Some(200),
            }))
        }
        Err(e) => {
            tracing::error!("Routing error: {}", e);
            let (message, technical) = map_error_to_user(&e.to_string());
            Ok(Json(LeadResponse {
                status: StatusNode {
                    success: false,
                    status: "error".to_string(),
                    message: Some(message),
                    error: Some(technical),
                },
                // Do not expose identifiers when routing failed due to configuration/routing problems
                lead: LeadNode {
                    promise_id: None,
                    lead_id: None,
                    lead_uuid: None,
                    ping_id: None,
                    bid: None,
                    post_id: None,
                    price: None,
                },
                verbose: if verbose_requested {
                    Some(serde_json::json!({
                        "error_code": "ERR_500",
                        "timestamp": Utc::now().to_rfc3339(),
                        "endpoint": "POST /api/v1/leads",
                        "status_code": 500
                    }))
                } else {
                    None
                },
                http_status: Some(500),
            }))
        }
    }
}
