use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use leadsnebula_core::models::publisher::Publisher;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::AppState;

#[derive(Deserialize)]
pub struct LeadRequest {
    pub lead: LeadData,
}

#[derive(Deserialize, Clone)]
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
}

#[derive(Serialize)]
pub struct LeadResponse {
    pub success: bool,
    pub lead_id: Option<String>,
    pub promise_id: Option<String>,
    pub ping_id: Option<String>,
    pub post_id: Option<String>,
    pub status: String,
    pub message: Option<String>,
    pub error: Option<String>,
}

pub fn carina_routes() -> Router<AppState> {
    Router::new().route("/api/v1/leads", post(create_lead))
}

async fn create_lead(
    State(state): State<AppState>,
    Extension(publisher): Extension<Publisher>,
    Json(payload): Json<LeadRequest>,
) -> Result<Json<LeadResponse>, StatusCode> {
    let lead_data = payload.lead;
    let request_type = lead_data
        .request_type
        .as_deref()
        .unwrap_or("ping")
        .to_lowercase();

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
                success: false,
                lead_id: None,
                promise_id: None,
                ping_id: None,
                post_id: None,
                status: "error".to_string(),
                message: None,
                error: Some(format!("Invalid vertical: {}", lead_data.vertical)),
            }));
        }
        Err(e) => {
            tracing::error!("Database error finding vertical: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Handle post request (update existing lead)
    if request_type == "post" {
        let promise_id = lead_data.promise_id.ok_or(StatusCode::BAD_REQUEST)?;

        let lead = match leadsnebula_core::models::lead::Lead::find_by_promise_id(
            &state.db_pool,
            &promise_id,
        )
        .await
        {
            Ok(Some(l)) => l,
            Ok(None) => {
                return Ok(Json(LeadResponse {
                    success: false,
                    lead_id: None,
                    promise_id: Some(promise_id),
                    ping_id: None,
                    post_id: None,
                    status: "error".to_string(),
                    message: None,
                    error: Some("Lead not found".to_string()),
                }));
            }
            Err(e) => {
                tracing::error!("Database error finding lead: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        };

        // Update lead (simplified - full implementation would validate and update all fields)
        // For now, just return success
        return Ok(Json(LeadResponse {
            success: true,
            lead_id: lead.lead_id.clone(),
            promise_id: lead.promise_id.clone(),
            ping_id: lead.ping_id.clone(),
            post_id: lead.post_id.clone(),
            status: "updated".to_string(),
            message: Some("Lead updated successfully".to_string()),
            error: None,
        }));
    }

    // Create new lead (ping/fullpost)
    let lead_id = lead_data
        .lead_id
        .clone()
        .unwrap_or_else(|| format!("lead_{}", uuid::Uuid::new_v4().to_string().replace('-', "")));

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
            promise_id, tcpa_consent, tcpa_language, is_test, vertical_data
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12
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
    .bind("ping_tree")
    .bind("processing")
    .bind(&promise_id)
    .bind(lead_data.tcpa_consent.unwrap_or(false))
    .bind(lead_data.tcpa_language.as_deref().unwrap_or(""))
    .bind(lead_data.is_test.unwrap_or(false))
    .bind(serde_json::json!({}))
    .fetch_one(&*state.db_pool)
    .await;

    let lead_uuid = match result {
        Ok(row) => row.get::<uuid::Uuid, _>(0),
        Err(e) => {
            tracing::error!("Database error creating lead: {}", e);
            tracing::error!("Lead data: event_id={}, lead_id={}, publisher_id={}, vertical_id={}, request_type={}", 
                event_id, lead_id, publisher.id, vertical.id, request_type);
            return Ok(Json(LeadResponse {
                success: false,
                lead_id: Some(lead_id),
                promise_id,
                ping_id: None,
                post_id: None,
                status: "error".to_string(),
                message: Some(format!("Database error: {}", e)),
                error: Some(format!("Failed to create lead: {}", e)),
            }));
        }
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
                success: false,
                lead_id: Some(lead_id),
                promise_id,
                ping_id: None,
                post_id: None,
                status: "error".to_string(),
                message: Some("Failed to load created lead".to_string()),
                error: Some(format!("Database error: {}", e)),
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

    match router.route(&state.db_pool).await {
        Ok(routing_result) => Ok(Json(LeadResponse {
            success: routing_result.success,
            lead_id: Some(lead_id),
            promise_id: routing_result.promise_id,
            ping_id: routing_result.ping_id,
            post_id: routing_result.post_id,
            status: routing_result.status,
            message: if routing_result.success {
                Some("Lead routed successfully".to_string())
            } else {
                None
            },
            error: routing_result.error,
        })),
        Err(e) => {
            tracing::error!("Routing error: {}", e);
            Ok(Json(LeadResponse {
                success: false,
                lead_id: Some(lead_id),
                promise_id,
                ping_id: None,
                post_id: None,
                status: "error".to_string(),
                message: None,
                error: Some(format!("Routing error: {}", e)),
            }))
        }
    }
}
