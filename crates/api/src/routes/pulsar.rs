use axum::{
    extract::{Extension, State},
    http::{header::HeaderMap, StatusCode},
    response::Json,
    routing::post,
    Router,
};
use leadsnebula_core::models::publisher::Publisher;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;

#[derive(Deserialize)]
pub struct PulsarLeadRequest {
    pub lead: PulsarLeadData,
}

#[derive(Deserialize, Clone)]
#[allow(dead_code)] // Fields will be used as implementation expands
pub struct PulsarLeadData {
    pub request_type: Option<String>,
    pub publisher_id: Option<String>,
    pub vertical: Option<String>,
    pub campaign_token: Option<String>,
    pub lead_id: Option<String>,
    pub promise_id: Option<String>,
    pub is_test: Option<bool>,
    pub zip: Option<String>,
    pub ip_address: Option<String>,
    pub monthly_bill: Option<f64>,
    pub own_home: Option<bool>,
    pub purchase_timeframe: Option<String>,
    pub credit_rating: Option<String>,
    pub property_type: Option<String>,
    pub roof_shade: Option<String>,
    pub roof_type: Option<String>,
    pub utility_provider: Option<String>,
    pub trusted_form_url: Option<String>,
    pub jornaya_lead_id: Option<String>,
    pub tcpa_consent: Option<bool>,
    pub tcpa_language: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub mobile_phone: Option<String>,
    pub cell_phone: Option<String>,
    pub state: Option<String>,
    pub city: Option<String>,
    pub street_address: Option<String>,
}

#[derive(Serialize)]
pub struct PulsarResponse {
    pub success: bool,
    pub status: String,
    pub ping_id: Option<String>,
    pub post_id: Option<String>,
    pub promise_id: Option<String>,
    pub price: Option<u32>,
    pub message: Option<String>,
    pub error: Option<String>,
    pub reason: Option<String>,
}

pub fn pulsar_routes() -> Router<AppState> {
    Router::new().route("/api/v1/pulsar/leads", post(handle_pulsar_lead))
}

async fn handle_pulsar_lead(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(_publisher): Extension<Publisher>,
    Json(payload): Json<PulsarLeadRequest>,
) -> Result<Json<PulsarResponse>, StatusCode> {
    // Extract buyer_id from internal header
    let buyer_id = headers
        .get("X-Internal-Buyer-ID")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| {
            tracing::warn!("Missing X-Internal-Buyer-ID header");
            StatusCode::BAD_REQUEST
        })?;

    let lead_data = payload.lead;
    let request_type = lead_data
        .request_type
        .as_deref()
        .unwrap_or("ping")
        .to_lowercase();

    match request_type.as_str() {
        "ping" => handle_ping(state, buyer_id, lead_data).await,
        "post" => handle_post(state, buyer_id, lead_data).await,
        "fullpost" => handle_fullpost(state, buyer_id, lead_data).await,
        _ => Ok(Json(PulsarResponse {
            success: false,
            status: "rejected".to_string(),
            ping_id: None,
            post_id: None,
            promise_id: None,
            price: None,
            message: None,
            error: Some("Invalid request_type. Must be 'ping', 'post', or 'fullpost'".to_string()),
            reason: None,
        })),
    }
}

async fn handle_ping(
    state: AppState,
    buyer_id: Uuid,
    lead_data: PulsarLeadData,
) -> Result<Json<PulsarResponse>, StatusCode> {
    let _lead_id = lead_data
        .lead_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Generate ping_id and promise_id
    let ping_id = format!(
        "PING_{}",
        hex::encode(rand::random::<[u8; 8]>()).to_uppercase()
    );
    let promise_id = format!(
        "PROMISE_{}",
        hex::encode(rand::random::<[u8; 6]>()).to_uppercase()
    );

    // Simplified qualification check (full implementation would use qualification engine)
    let accepted = true; // Default accept for now

    if !accepted {
        let rejected_ping_id = format!(
            "PING_REJECTED_{}",
            hex::encode(rand::random::<[u8; 8]>()).to_uppercase()
        );
        return Ok(Json(PulsarResponse {
            success: false,
            status: "rejected".to_string(),
            ping_id: Some(rejected_ping_id),
            post_id: None,
            promise_id: None,
            price: None,
            message: Some("Lead did not meet qualification requirements".to_string()),
            error: Some("Lead rejected by qualification rules".to_string()),
            reason: Some("Qualification check failed".to_string()),
        }));
    }

    // Log decision
    let _ = sqlx::query(
        r#"
        INSERT INTO pulsar_decision_logs (lead_id, ping_id, buyer_id, accepted, final_bid_price, evaluated_at)
        VALUES ($1, $2, $3, $4, $5, NOW())
        "#,
    )
    .bind(lead_data.lead_id.as_ref())
    .bind(&ping_id)
    .bind(buyer_id)
    .bind(accepted)
    .bind(Some((rand::random::<u32>() % 200 + 100) as i32)) // Random price 100-300
    .execute(&*state.db_pool)
    .await;

    Ok(Json(PulsarResponse {
        success: true,
        status: "accepted".to_string(),
        ping_id: Some(ping_id),
        post_id: None,
        promise_id: Some(promise_id),
        price: Some(rand::random::<u32>() % 200 + 100),
        message: Some("Lead accepted for ping".to_string()),
        error: None,
        reason: None,
    }))
}

async fn handle_post(
    state: AppState,
    buyer_id: Uuid,
    lead_data: PulsarLeadData,
) -> Result<Json<PulsarResponse>, StatusCode> {
    let promise_id = lead_data.promise_id.ok_or(StatusCode::BAD_REQUEST)?;
    let promise_id_for_message = promise_id.clone();
    let lead_id = lead_data
        .lead_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Check for duplicate promise_id
    let existing =
        sqlx::query("SELECT post_id FROM leads WHERE promise_id = $1 AND status = 'sold' LIMIT 1")
            .bind(&promise_id)
            .fetch_optional(&*state.db_pool)
            .await
            .map_err(|e| {
                tracing::error!("Database error checking duplicate promise_id: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

    if existing.is_some() {
        return Ok(Json(PulsarResponse {
            success: false,
            status: "rejected".to_string(),
            ping_id: None,
            post_id: None,
            promise_id: Some(promise_id),
            price: None,
            message: Some(format!(
                "The promise_id '{}' was already used for a sold lead and cannot be reused.",
                promise_id_for_message
            )),
            error: Some("This promise_id has already been used".to_string()),
            reason: None,
        }));
    }

    // Simplified qualification check
    let accepted = true;

    if !accepted {
        return Ok(Json(PulsarResponse {
            success: false,
            status: "rejected".to_string(),
            ping_id: None,
            post_id: None,
            promise_id: Some(promise_id),
            price: None,
            message: Some("Lead did not meet qualification requirements".to_string()),
            error: Some("Lead rejected by qualification rules".to_string()),
            reason: Some("Qualification check failed".to_string()),
        }));
    }

    let post_id = format!(
        "POST_{}",
        hex::encode(rand::random::<[u8; 8]>()).to_uppercase()
    );

    // Log decision
    let _ = sqlx::query(
        r#"
        INSERT INTO pulsar_decision_logs (lead_id, buyer_id, accepted, final_bid_price, evaluated_at)
        VALUES ($1, $2, $3, $4, NOW())
        "#,
    )
    .bind(&lead_id)
    .bind(buyer_id)
    .bind(accepted)
    .bind(Some((rand::random::<u32>() % 200 + 100) as i32))
    .execute(&*state.db_pool)
    .await;

    Ok(Json(PulsarResponse {
        success: true,
        status: "sold".to_string(),
        ping_id: None,
        post_id: Some(post_id),
        promise_id: Some(promise_id),
        price: Some(rand::random::<u32>() % 200 + 100),
        message: Some("Lead accepted and sold".to_string()),
        error: None,
        reason: None,
    }))
}

async fn handle_fullpost(
    _state: AppState,
    _buyer_id: Uuid,
    lead_data: PulsarLeadData,
) -> Result<Json<PulsarResponse>, StatusCode> {
    let _lead_id = lead_data
        .lead_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let ping_id = format!(
        "PING_{}",
        hex::encode(rand::random::<[u8; 8]>()).to_uppercase()
    );
    let post_id = format!(
        "POST_{}",
        hex::encode(rand::random::<[u8; 8]>()).to_uppercase()
    );
    let promise_id = format!(
        "PROMISE_{}",
        hex::encode(rand::random::<[u8; 6]>()).to_uppercase()
    );

    Ok(Json(PulsarResponse {
        success: true,
        status: "sold".to_string(),
        ping_id: Some(ping_id),
        post_id: Some(post_id),
        promise_id: Some(promise_id),
        price: Some(rand::random::<u32>() % 200 + 100),
        message: Some("Lead accepted and sold via fullpost".to_string()),
        error: None,
        reason: None,
    }))
}
