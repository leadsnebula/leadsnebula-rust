use axum::{
    async_trait,
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode},
    response::Json,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use leadsnebula_core::models::publisher::Publisher;

// Custom extractor for Publisher from request extensions
struct PublisherExtractor(pub Publisher);

#[async_trait]
impl<S> FromRequestParts<S> for PublisherExtractor
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Publisher>()
            .cloned()
            .map(PublisherExtractor)
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

#[derive(Deserialize)]
pub struct LeadRequest {
    pub lead: LeadData,
    #[serde(default)]
    #[allow(dead_code)] // Will be used when full implementation is added
    pub verbose: bool,
}

#[derive(Deserialize, Clone)]
#[allow(dead_code)] // Fields will be used when full implementation is added
pub struct LeadData {
    pub publisher_id: Option<String>,
    pub vertical: String,
    pub request_type: String,
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
    pub status: String,
    pub ping_id: Option<String>,
    pub post_id: Option<String>,
    pub promise_id: Option<String>,
    pub price: Option<u32>,
    pub bid: Option<u32>,
    pub message: Option<String>,
    pub error: Option<String>,
    pub reason: Option<String>,
}

pub fn leads_routes() -> Router<AppState> {
    Router::new().route("/api/v1/leads", post(handle_lead_submission))
}

async fn handle_lead_submission(
    State(_state): State<AppState>,
    PublisherExtractor(publisher): PublisherExtractor,
    Json(payload): Json<LeadRequest>,
) -> Result<Json<LeadResponse>, StatusCode> {

    // For now, return a "not implemented" response
    // TODO: Implement full lead processing logic similar to Ruby LeadsProcessing::Ingester
    tracing::warn!(
        "Lead submission endpoint called but not fully implemented yet. Publisher: {}, Vertical: {}, Request Type: {}",
        publisher.id,
        payload.lead.vertical,
        payload.lead.request_type
    );

    // Return a basic response indicating the endpoint exists but needs implementation
    Ok(Json(LeadResponse {
        success: false,
        status: "error".to_string(),
        ping_id: None,
        post_id: None,
        promise_id: None,
        price: None,
        bid: None,
        message: Some("Lead submission endpoint is not yet fully implemented in Rust API".to_string()),
        error: Some("NotImplemented".to_string()),
        reason: Some("This endpoint is being migrated from Ruby to Rust".to_string()),
    }))
}
