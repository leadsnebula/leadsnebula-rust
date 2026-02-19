// Lead submission endpoint - handles ping, post, and fullpost request types
// This is the main entry point for lead routing through ping trees

use axum::{
    extract::{Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::post,
    Router,
};
use chrono::Utc;
use leadsnebula_core::models::enums::LeadStatus;
use leadsnebula_core::models::publisher::Publisher;
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};

use crate::AppState;
use leadsnebula_core::services::auction_timing::AtomicAuctionTiming;
use leadsnebula_core::services::diagnostic_metrics::DiagnosticMetrics;
use leadsnebula_core::services::ssm_key_cache::get_ssm_parameter_cached;
use simd_json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use std::time::Instant;
use tokio_retry::{strategy::ExponentialBackoff, Retry};

const RATE_LIMIT_PER_HOUR: u32 = 360;
const RATE_LIMIT_WINDOW_SECONDS: u64 = 3600;

#[derive(Clone, Debug)]
struct RateBucket {
    window_start: Instant,
    count: u32,
}

static LEAD_RATE_BUCKETS: OnceLock<Mutex<HashMap<String, RateBucket>>> = OnceLock::new();

fn rate_buckets() -> &'static Mutex<HashMap<String, RateBucket>> {
    LEAD_RATE_BUCKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

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
    if lower.contains("column")
        && lower.contains("publisher_id")
        && lower.contains("does not exist")
    {
        problems.push(
            "Server configuration error: database schema mismatch. Contact the administrator."
                .to_string(),
        );
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

/// Returns the first missing required field name for the given request type, or None if all required fields are present.
/// campaign_token is optional (required only for ronin campaigns; see post-prechecks validation).
/// purchase_timeframe is optional for all request types. roof_shade is required for all.
fn missing_required_lead_field(lead: &LeadData, request_type: &str) -> Option<&'static str> {
    let check = |name: &'static str, present: bool| -> Option<&'static str> {
        if present {
            None
        } else {
            Some(name)
        }
    };
    match request_type {
        "post" => {
            let r = [
                ("promise_id", lead.promise_id.is_some()),
                ("publisher_id", lead.publisher_id.is_some()),
                ("request_type", lead.request_type.is_some()),
                ("first_name", lead.first_name.is_some()),
                ("last_name", lead.last_name.is_some()),
                ("email", lead.email.is_some()),
                ("cell_phone", lead.cell_phone.is_some()),
                ("street_address", lead.street_address.is_some()),
                ("city", lead.city.is_some()),
                ("state", lead.state.is_some()),
                ("zip", lead.zip.is_some()),
                ("ip_address", lead.ip_address.is_some()),
                ("monthly_bill", lead.monthly_bill.is_some()),
                ("own_home", lead.own_home.is_some()),
                ("roof_shade", lead.roof_shade.is_some()),
                ("utility_provider", lead.utility_provider.is_some()),
                ("property_type", lead.property_type.is_some()),
                ("tcpa_consent", lead.tcpa_consent.is_some()),
                ("tcpa_language", lead.tcpa_language.is_some()),
                ("credit_rating", lead.credit_rating.is_some()),
                ("jornaya_lead_id", lead.jornaya_lead_id.is_some()),
                ("trusted_form_url", lead.trusted_form_url.is_some()),
            ];
            for (name, present) in r {
                if let Some(m) = check(name, present) {
                    return Some(m);
                }
            }
            None
        }
        "fullpost" => {
            let r = [
                ("publisher_id", lead.publisher_id.is_some()),
                ("request_type", lead.request_type.is_some()),
                ("first_name", lead.first_name.is_some()),
                ("last_name", lead.last_name.is_some()),
                ("email", lead.email.is_some()),
                ("cell_phone", lead.cell_phone.is_some()),
                ("street_address", lead.street_address.is_some()),
                ("city", lead.city.is_some()),
                ("state", lead.state.is_some()),
                ("zip", lead.zip.is_some()),
                ("ip_address", lead.ip_address.is_some()),
                ("monthly_bill", lead.monthly_bill.is_some()),
                ("own_home", lead.own_home.is_some()),
                ("roof_shade", lead.roof_shade.is_some()),
                ("utility_provider", lead.utility_provider.is_some()),
                ("credit_rating", lead.credit_rating.is_some()),
                ("tcpa_consent", lead.tcpa_consent.is_some()),
                ("tcpa_language", lead.tcpa_language.is_some()),
                ("jornaya_lead_id", lead.jornaya_lead_id.is_some()),
                ("trusted_form_url", lead.trusted_form_url.is_some()),
            ];
            for (name, present) in r {
                if let Some(m) = check(name, present) {
                    return Some(m);
                }
            }
            None
        }
        _ => {
            // ping (order: check each required field)
            let r = [
                ("publisher_id", lead.publisher_id.is_some()),
                ("request_type", lead.request_type.is_some()),
                ("zip", lead.zip.is_some()),
                ("ip_address", lead.ip_address.is_some()),
                ("monthly_bill", lead.monthly_bill.is_some()),
                ("own_home", lead.own_home.is_some()),
                ("roof_shade", lead.roof_shade.is_some()),
                ("credit_rating", lead.credit_rating.is_some()),
                ("tcpa_consent", lead.tcpa_consent.is_some()),
                ("tcpa_language", lead.tcpa_language.is_some()),
                ("jornaya_lead_id", lead.jornaya_lead_id.is_some()),
                ("trusted_form_url", lead.trusted_form_url.is_some()),
            ];
            for (name, present) in r {
                if !present {
                    return Some(name);
                }
            }
            None
        }
    }
}

/// Human-readable message for validation error (for lead record and dashboard).
fn validation_error_human_message(field: &str) -> String {
    match field {
        "ip_address" => "IP Address Missing".to_string(),
        "email" => "Email Missing".to_string(),
        "first_name" => "First Name Missing".to_string(),
        "last_name" => "Last Name Missing".to_string(),
        "cell_phone" => "Cell Phone Missing".to_string(),
        "street_address" => "Street Address Missing".to_string(),
        "city" => "City Missing".to_string(),
        "state" => "State Missing".to_string(),
        "zip" => "Zip Missing".to_string(),
        "monthly_bill" => "Monthly Bill Missing".to_string(),
        "own_home" => "Own Home Missing".to_string(),
        "roof_shade" => "Roof Shade Missing".to_string(),
        "utility_provider" => "Utility Provider Missing".to_string(),
        "credit_rating" => "Credit Rating Missing".to_string(),
        "tcpa_consent" => "TCPA Consent Missing".to_string(),
        "tcpa_language" => "TCPA Language Missing".to_string(),
        "jornaya_lead_id" => "Jornaya Lead ID Missing".to_string(),
        "trusted_form_url" => "Trusted Form URL Missing".to_string(),
        "property_type" => "Property Type Missing".to_string(),
        "promise_id" => "Promise ID Missing".to_string(),
        "publisher_id" => "Publisher ID Missing".to_string(),
        "request_type" => "Request Type Missing".to_string(),
        _ => format!("{} missing", field.replace('_', " ")),
    }
}

/// Persist a failed lead (any non-sold status) so it appears in the leads report with audit trail.
/// Used for validation_error, publisher_mismatch, invalid_format, and any early failure where we have a vertical.
/// Resolves buyer_id/campaign_id from any campaign for this vertical (required by NOT NULL constraints).
#[allow(clippy::too_many_arguments)]
async fn persist_failed_lead(
    pool: &sqlx::PgPool,
    publisher_id: uuid::Uuid,
    vertical: &leadsnebula_core::models::vertical::Vertical,
    lead_data: &LeadData,
    request_type: &str,
    request_body: &LeadRequest,
    response_body: &LeadResponse,
    status: LeadStatus,
    vertical_data: serde_json::Value,
    ping_message: &str,
) -> Result<uuid::Uuid, sqlx::Error> {
    use rand::Rng;

    // Resolve a campaign for this publisher's instance so we can set buyer_id/campaign_id when present.
    // If the instance has no campaign, we persist the error lead with NULL buyer_id/campaign_id so it still appears in the leads report.
    let instance_id: uuid::Uuid = sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT instance_id FROM publishers WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(publisher_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| sqlx::Error::ColumnNotFound("publisher instance".into()))?;

    let (buyer_id, campaign_id): (Option<uuid::Uuid>, Option<uuid::Uuid>) =
        match sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid)>(
            "SELECT buyer_id, id FROM campaigns WHERE instance_id = $1 AND vertical = $2 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(instance_id)
        .bind(&vertical.slug)
        .fetch_optional(pool)
        .await?
        {
            Some(row) => (Some(row.0), Some(row.1)),
            None => sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid)>(
                "SELECT buyer_id, id FROM campaigns WHERE instance_id = $1 AND deleted_at IS NULL LIMIT 1",
            )
            .bind(instance_id)
            .fetch_optional(pool)
            .await?
            .map(|row| (Some(row.0), Some(row.1)))
            .unwrap_or((None, None)),
        };

    let strategy = match request_type {
        "fullpost" => "fullPost",
        _ => "pingPost",
    };
    let lead_uuid = uuid::Uuid::new_v4();
    let event_uuid = uuid::Uuid::new_v4();
    let event_id = format!("evt_{}", event_uuid);
    let lead_id = {
        let mut lead_id = String::with_capacity(vertical.slug.len() + 9);
        lead_id.push_str(&vertical.slug.to_uppercase());
        lead_id.push('-');
        let mut rng = rand::thread_rng();
        for _ in 0..8 {
            let c = rng.sample(Alphanumeric);
            lead_id.push(char::from(c).to_ascii_uppercase());
        }
        lead_id
    };
    let session_uuid = uuid::Uuid::new_v4();
    let session_id = format!("sess_{}", session_uuid);
    let request_json = serde_json::to_value(request_body).unwrap_or_else(|_| serde_json::json!({}));
    let response_json =
        serde_json::to_value(response_body).unwrap_or_else(|_| serde_json::json!({}));

    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO leads (
            uuid, event_id, lead_id, publisher_id, vertical_id, request_type, strategy, status,
            promise_id, tcpa_consent, tcpa_language, is_test, session_id, vertical_data,
            buyer_id, campaign_id, post_id, submitted_at, created_at, updated_at,
            first_name_encrypted, last_name_encrypted, email_encrypted, cell_phone_encrypted,
            street_address_encrypted, city_encrypted, state_encrypted, zip_encrypted, ip_address_encrypted
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, NULL, $9, $10, $11, $12, $13,
            $14, $15, '', NOW(), NOW(), NOW(),
            NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL
        )
        "#,
    )
    .bind(lead_uuid)
    .bind(&event_id)
    .bind(&lead_id)
    .bind(publisher_id)
    .bind(vertical.id)
    .bind(request_type)
    .bind(strategy)
    .bind(&status)
    .bind(lead_data.tcpa_consent.unwrap_or(false))
    .bind(lead_data.tcpa_language.as_deref().unwrap_or(""))
    .bind(lead_data.is_test.unwrap_or(false))
    .bind(&session_id)
    .bind(sqlx::types::Json(vertical_data))
    .bind(buyer_id)
    .bind(campaign_id)
    .execute(&mut *tx)
    .await?;

    let status_str = status.as_str();
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let ping_id_text = format!("ERR_{}_{}", lead_uuid, timestamp);
    let ping_row: (i64,) = sqlx::query_as(
        "INSERT INTO pings (ping_id, lead_id, promise_id, state, sent_at, created_at) VALUES ($1, $2, NULL, $3, now(), now()) RETURNING id",
    )
    .bind(&ping_id_text)
    .bind(lead_uuid)
    .bind(status_str)
    .fetch_one(&mut *tx)
    .await?;
    let ping_db_id = ping_row.0;

    let payload = serde_json::json!({
        "request": request_json,
        "response": response_json,
        "validation_error": ping_message,
    });
    sqlx::query(
        "INSERT INTO ping_payloads (ping_id, lead_id, payload, created_at, updated_at) VALUES ($1::bigint, $2, $3, now(), now())",
    )
    .bind(ping_db_id)
    .bind(lead_uuid)
    .bind(sqlx::types::Json(payload))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(lead_uuid)
}

/// Persist a lead that failed validation (missing required field) so it appears in the leads report.
#[allow(clippy::too_many_arguments)]
async fn persist_validation_error_lead(
    pool: &sqlx::PgPool,
    publisher_id: uuid::Uuid,
    vertical: &leadsnebula_core::models::vertical::Vertical,
    lead_data: &LeadData,
    request_type: &str,
    missing_field: &str,
    request_body: &LeadRequest,
    response_body: &LeadResponse,
) -> Result<uuid::Uuid, sqlx::Error> {
    let human_message = validation_error_human_message(missing_field);
    let vertical_data = serde_json::json!({
        "validation_error": human_message,
        "missing_field": missing_field,
    });
    persist_failed_lead(
        pool,
        publisher_id,
        vertical,
        lead_data,
        request_type,
        request_body,
        response_body,
        LeadStatus::Error,
        vertical_data,
        &human_message,
    )
    .await
}

/// Transform per_buyer_timings for verbose response: align status with main outcome, strip bid for fullpost.
/// Ensures verbose is consistent (e.g. "sold" not "accepted") and fullpost never exposes bid.
pub(crate) fn transform_per_buyer_timings_for_verbose(
    timings: &[serde_json::Value],
    main_status: &str,
    is_fullpost: bool,
) -> Vec<serde_json::Value> {
    timings
        .iter()
        .map(|t| {
            let mut obj = t.clone();
            if let Some(o) = obj.as_object_mut() {
                if is_fullpost {
                    o.remove("bid");
                }
                o.insert(
                    "status".to_string(),
                    serde_json::Value::String(main_status.to_string()),
                );
            }
            obj
        })
        .collect()
}

#[derive(Deserialize, Serialize, Clone)]
pub struct LeadRequest {
    pub verbose: Option<bool>,
    pub lead: LeadData,
}

/// Incoming request body: lead can be nested (grouped nodes) or flat. Parsed then normalized to LeadData.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct LeadRequestIncoming {
    pub verbose: Option<bool>,
    #[serde(deserialize_with = "deserialize_lead_body")]
    pub lead: LeadData,
}

/// Deserialize lead as either nested (request_properties, publisher_data, ...) or flat (all fields at top level).
fn deserialize_lead_body<'de, D>(deserializer: D) -> Result<LeadData, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.get("request_properties").is_some()
        && value.get("publisher_data").is_some()
        && value.get("consumer_data").is_some()
        && value.get("property_data").is_some()
        && value.get("compliance").is_some()
    {
        let nested: LeadNested = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        Ok(nested.into())
    } else {
        let flat: LeadData = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        Ok(flat)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct LeadNested {
    request_properties: Option<NestedRequestProperties>,
    publisher_data: Option<NestedPublisherData>,
    consumer_data: Option<NestedConsumerData>,
    property_data: Option<NestedPropertyData>,
    compliance: Option<NestedCompliance>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct NestedRequestProperties {
    vertical: Option<String>,
    is_test: Option<bool>,
    request_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct NestedPublisherData {
    publisher_id: Option<String>,
    campaign_token: Option<String>,
    lead_id: Option<String>,
    promise_id: Option<String>,
    source: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct NestedConsumerData {
    first_name: Option<String>,
    last_name: Option<String>,
    email: Option<String>,
    cell_phone: Option<String>,
    street_address: Option<String>,
    city: Option<String>,
    state: Option<String>,
    zip: Option<String>,
    credit_rating: Option<String>,
    ip_address: Option<String>,
    user_agent: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct NestedPropertyData {
    monthly_bill: Option<f64>,
    own_home: Option<bool>,
    property_type: Option<String>,
    purchase_timeframe: Option<String>,
    roof_shade: Option<String>,
    roof_type: Option<String>,
    utility_provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct NestedCompliance {
    tcpa_consent: Option<bool>,
    tcpa_language: Option<String>,
    jornaya_lead_id: Option<String>,
    trusted_form_url: Option<String>,
}

impl From<LeadNested> for LeadData {
    fn from(n: LeadNested) -> Self {
        let r = n.request_properties.as_ref();
        let p = n.publisher_data.as_ref();
        let c = n.consumer_data.as_ref();
        let prop = n.property_data.as_ref();
        let comp = n.compliance.as_ref();
        LeadData {
            publisher_id: p.and_then(|x| x.publisher_id.clone()),
            vertical: r.and_then(|x| x.vertical.clone()).unwrap_or_default(),
            request_type: r.and_then(|x| x.request_type.clone()),
            campaign_token: p.and_then(|x| x.campaign_token.clone()),
            promise_id: p.and_then(|x| x.promise_id.clone()),
            lead_id: p.and_then(|x| x.lead_id.clone()),
            source: p.and_then(|x| x.source.clone()), // publisher website / traffic source
            first_name: c.and_then(|x| x.first_name.clone()),
            last_name: c.and_then(|x| x.last_name.clone()),
            email: c.and_then(|x| x.email.clone()),
            cell_phone: c.and_then(|x| x.cell_phone.clone()),
            street_address: c.and_then(|x| x.street_address.clone()),
            city: c.and_then(|x| x.city.clone()),
            state: c.and_then(|x| x.state.clone()),
            zip: c.and_then(|x| x.zip.clone()),
            monthly_bill: prop.and_then(|x| x.monthly_bill),
            credit_rating: c.and_then(|x| x.credit_rating.clone()),
            own_home: prop.and_then(|x| x.own_home),
            property_type: prop.and_then(|x| x.property_type.clone()),
            roof_shade: prop.and_then(|x| x.roof_shade.clone()),
            roof_type: prop.and_then(|x| x.roof_type.clone()),
            utility_provider: prop.and_then(|x| x.utility_provider.clone()),
            purchase_timeframe: prop.and_then(|x| x.purchase_timeframe.clone()),
            ip_address: c.and_then(|x| x.ip_address.clone()),
            tcpa_consent: comp.and_then(|x| x.tcpa_consent),
            tcpa_language: comp.and_then(|x| x.tcpa_language.clone()),
            jornaya_lead_id: comp.and_then(|x| x.jornaya_lead_id.clone()),
            trusted_form_url: comp.and_then(|x| x.trusted_form_url.clone()),
            is_test: r.and_then(|x| x.is_test),
            verbose: None,
            user_agent: c.and_then(|x| x.user_agent.clone()),
        }
    }
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
    pub source: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub cell_phone: Option<String>,
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
    /// Ignored; use top-level `verbose` only. Kept for backward compatibility.
    pub verbose: Option<bool>,
    pub user_agent: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct StatusNode {
    pub success: bool,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
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

#[derive(Serialize, Clone)]
pub struct LeadResponse {
    // Preserve order: status, lead, verbose
    pub status: StatusNode,
    pub lead: LeadNode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbose: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
}

pub fn leads_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/leads", post(create_lead))
        .route("/api/v1/leads/ping", post(create_lead_ping))
        .route("/api/v1/leads/post", post(create_lead_post))
        .route("/api/v1/leads/fullpost", post(create_lead_fullpost))
}

async fn create_lead_ping(
    State(state): State<AppState>,
    Extension(publisher): Extension<Publisher>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    Json(payload_in): Json<LeadRequestIncoming>,
) -> Result<(StatusCode, Json<LeadResponse>), StatusCode> {
    let mut lead = payload_in.lead;
    lead.request_type = Some("ping".to_string());
    let payload = LeadRequest {
        verbose: payload_in.verbose,
        lead,
    };
    create_lead(
        State(state),
        Extension(publisher),
        Query(params),
        headers,
        Json(payload),
    )
    .await
}

async fn create_lead_post(
    State(state): State<AppState>,
    Extension(publisher): Extension<Publisher>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    Json(payload_in): Json<LeadRequestIncoming>,
) -> Result<(StatusCode, Json<LeadResponse>), StatusCode> {
    let mut lead = payload_in.lead;
    lead.request_type = Some("post".to_string());
    let payload = LeadRequest {
        verbose: payload_in.verbose,
        lead,
    };
    create_lead(
        State(state),
        Extension(publisher),
        Query(params),
        headers,
        Json(payload),
    )
    .await
}

async fn create_lead_fullpost(
    State(state): State<AppState>,
    Extension(publisher): Extension<Publisher>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    Json(payload_in): Json<LeadRequestIncoming>,
) -> Result<(StatusCode, Json<LeadResponse>), StatusCode> {
    let mut lead = payload_in.lead;
    lead.request_type = Some("fullpost".to_string());
    let payload = LeadRequest {
        verbose: payload_in.verbose,
        lead,
    };
    create_lead(
        State(state),
        Extension(publisher),
        Query(params),
        headers,
        Json(payload),
    )
    .await
}

/// Extract request context for compliance/verbose (ISO 27001, SOC 2, NIST).
/// Caller can pass ip_address from lead PII if already decrypted, or None.
#[allow(clippy::too_many_arguments)]
fn request_context_from_headers(
    headers: &HeaderMap,
    endpoint: &str,
    method: &str,
    reason: &str,
    source: &str,
    outcome: &str,
    target_type: &str,
    target_id: &str,
    action: &str,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> serde_json::Value {
    let ip_address = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
    let ip_address_v4 = headers
        .get("x-client-ipv4")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_string());
    let ip_address_v6 = headers
        .get("x-client-ipv6")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_string());
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
    let session_id = headers
        .get("x-session-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    let referer = headers
        .get("referer")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    let mut context = serde_json::Map::new();
    context.insert("endpoint".to_string(), serde_json::json!(endpoint));
    context.insert("method".to_string(), serde_json::json!(method));
    context.insert("reason".to_string(), serde_json::json!(reason));
    context.insert("source".to_string(), serde_json::json!(source));
    context.insert("request_id".to_string(), serde_json::json!(request_id));
    context.insert("ip_address".to_string(), serde_json::json!(ip_address));
    if ip_address_v4.is_some() || ip_address_v6.is_some() {
        context.insert(
            "ip_address_v4".to_string(),
            serde_json::json!(ip_address_v4),
        );
        context.insert(
            "ip_address_v6".to_string(),
            serde_json::json!(ip_address_v6),
        );
    }
    context.insert("user_agent".to_string(), serde_json::json!(user_agent));
    context.insert("referer".to_string(), serde_json::json!(referer));
    context.insert("session_id".to_string(), serde_json::json!(session_id));
    serde_json::json!({
        "context": serde_json::Value::Object(context),
        "compliance": { "standard": "ISO_27001_SOC2_NIST", "version": "2024" },
        "outcome": outcome,
        "timestamp": timestamp.to_rfc3339(),
        "target_type": target_type,
        "target_id": target_id,
        "target_name": serde_json::Value::Null,
        "action": action
    })
}

async fn create_lead(
    State(state): State<AppState>,
    Extension(publisher): Extension<Publisher>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    Json(payload): Json<LeadRequest>,
) -> Result<(StatusCode, Json<LeadResponse>), StatusCode> {
    // Enforce per-publisher lead rate limit (360 requests/hour).
    let rate_key = format!("publisher:{}", publisher.id);
    let now = Instant::now();
    if let Ok(mut map) = rate_buckets().lock() {
        let bucket = map.entry(rate_key).or_insert(RateBucket {
            window_start: now,
            count: 0,
        });

        if now.duration_since(bucket.window_start).as_secs() >= RATE_LIMIT_WINDOW_SECONDS {
            bucket.window_start = now;
            bucket.count = 0;
        }

        if bucket.count >= RATE_LIMIT_PER_HOUR {
            let elapsed = now.duration_since(bucket.window_start).as_secs();
            let retry_after = RATE_LIMIT_WINDOW_SECONDS.saturating_sub(elapsed);
            return Ok((
                StatusCode::TOO_MANY_REQUESTS,
                Json(LeadResponse {
                    status: StatusNode {
                        success: false,
                        status: "error".to_string(),
                        message: Some(format!(
                            "Rate limit exceeded: max {} requests per hour",
                            RATE_LIMIT_PER_HOUR
                        )),
                        error: Some("Too many requests".to_string()),
                    },
                    lead: LeadNode {
                        promise_id: Some(String::new()),
                        lead_id: Some(String::new()),
                        lead_uuid: Some(String::new()),
                        ping_id: Some(String::new()),
                        bid: Some(0.0),
                        post_id: Some(String::new()),
                        price: Some(0.0),
                    },
                    verbose: Some(serde_json::json!({
                        "status_code": 429,
                        "rate_limit_per_hour": RATE_LIMIT_PER_HOUR,
                        "remaining": 0,
                        "retry_after_seconds": retry_after
                    })),
                    http_status: Some(429),
                }),
            ));
        }

        bucket.count += 1;
    }

    // BLAME-SHIFTING: Track total wall-clock time from request start
    let request_start = std::time::Instant::now();

    // Initialize timing and metrics
    let timing = Arc::new(AtomicAuctionTiming::new());
    let metrics = Arc::new(DiagnosticMetrics::new());

    // Check for minimal mode and async mode
    let minimal_mode = params.get("minimal").map(|v| v == "true").unwrap_or(false);
    let async_mode = params.get("async").map(|v| v == "true").unwrap_or(false);

    let request_level_verbose = if minimal_mode {
        false // Skip verbose in minimal mode
    } else {
        payload.verbose.unwrap_or(false)
    };
    let lead_data = payload.lead;
    let request_type = lead_data
        .request_type
        .as_deref()
        .unwrap_or("ping")
        .to_lowercase();

    tracing::warn!(
        vertical = %lead_data.vertical,
        request_type = %request_type,
        publisher_id = %publisher.id,
        "POST /api/v1/leads request received"
    );

    // Top-level verbose takes precedence; only use lead-level verbose if top-level is not set
    let verbose_requested = if payload.verbose.is_some() {
        request_level_verbose
    } else {
        lead_data.verbose.unwrap_or(false)
    };

    // Validate vertical (CACHED - 24h TTL, verticals rarely change)
    // Note: Pre-checks query uses vertical.slug in subquery, so it could theoretically run in parallel
    // However, vertical lookup is cached and fast (<5ms), so parallelization benefit is minimal
    let vertical_start = std::time::Instant::now();
    let cache_key = format!("vertical:slug:{}", lead_data.vertical);

    let (vertical_result, _cache_hit) = if let Some(cache) = &state.cache {
        // Use cached lookup with 24h TTL
        // Cache stores Option<Vertical> - serialize None as "null"
        let cache_check_start = std::time::Instant::now();
        let result = match cache
            .get_or_insert_with(
                &cache_key,
                86400, // 24 hours
                || async {
                    let result = leadsnebula_core::models::vertical::Vertical::find_by_slug(
                        &state.db_pool,
                        &lead_data.vertical,
                    )
                    .await?;
                    Ok::<Option<leadsnebula_core::models::vertical::Vertical>, anyhow::Error>(
                        result,
                    )
                },
            )
            .await
        {
            Ok(v) => {
                let cache_hit = cache_check_start.elapsed().as_millis() < 10; // Very fast = cache hit
                if cache_hit {
                    metrics.record_cache_hit();
                } else {
                    metrics.record_cache_miss();
                }
                (Ok(v), cache_hit)
            }
            Err(e) => {
                tracing::warn!("Cache lookup failed, falling back to DB: {}", e);
                metrics.record_cache_miss();
                // Fallback to direct DB query
                let db_start = std::time::Instant::now();
                let result = leadsnebula_core::models::vertical::Vertical::find_by_slug(
                    &state.db_pool,
                    &lead_data.vertical,
                )
                .await;
                metrics.record_query(db_start.elapsed().as_millis() as u64);
                (result, false)
            }
        };
        result
    } else {
        // No cache available, use direct DB query
        let db_start = std::time::Instant::now();
        let result = leadsnebula_core::models::vertical::Vertical::find_by_slug(
            &state.db_pool,
            &lead_data.vertical,
        )
        .await;
        metrics.record_query(db_start.elapsed().as_millis() as u64);
        (result, false)
    };
    let vertical_duration = vertical_start.elapsed().as_millis() as u64;
    timing.record_pre_checks(vertical_duration);

    let vertical = match vertical_result {
        Ok(Some(v)) => v,
        Ok(None) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(LeadResponse {
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
                }),
            ));
        }
        Err(e) => {
            tracing::error!("Database error finding vertical: {}", e);
            let (message, technical) = map_error_to_user(&e.to_string());
            return Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LeadResponse {
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
                }),
            ));
        }
    };

    // Validate publisher_id if provided in request body
    if let Some(ref provided_publisher_id) = lead_data.publisher_id {
        let provided_uuid = uuid::Uuid::parse_str(provided_publisher_id).ok();
        if let Some(provided_uuid) = provided_uuid {
            if provided_uuid != publisher.id {
                let response_body = LeadResponse {
                    status: StatusNode {
                        success: false,
                        status: "error".to_string(),
                        message: Some("Publisher ID mismatch. Your API key is associated with a different publisher.".to_string()),
                        error: Some(format!("Publisher ID mismatch: provided={}, authenticated={}", provided_publisher_id, publisher.id)),
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
                            "error_code": "ERR_401",
                            "timestamp": Utc::now().to_rfc3339(),
                            "endpoint": "POST /api/v1/leads",
                            "status_code": 401,
                            "provided_publisher_id": provided_publisher_id,
                            "authenticated_publisher_id": publisher.id.to_string()
                        }))
                    } else {
                        None
                    },
                    http_status: Some(401),
                };
                let request_body = LeadRequest {
                    verbose: payload.verbose,
                    lead: lead_data.clone(),
                };
                let pool = state.db_pool.clone();
                let publisher_id = publisher.id;
                let vertical_clone = vertical.clone();
                let lead_data_clone = lead_data.clone();
                let request_type_clone = request_type.clone();
                let request_body_clone = request_body.clone();
                let response_body_clone = response_body.clone();
                let msg = "Publisher ID mismatch".to_string();
                tokio::spawn(async move {
                    let vertical_data = serde_json::json!({
                        "error_kind": "publisher_mismatch",
                        "message": &msg,
                    });
                    if let Err(e) = persist_failed_lead(
                        pool.as_ref(),
                        publisher_id,
                        &vertical_clone,
                        &lead_data_clone,
                        &request_type_clone,
                        &request_body_clone,
                        &response_body_clone,
                        LeadStatus::Error,
                        vertical_data,
                        msg.as_str(),
                    )
                    .await
                    {
                        tracing::warn!(
                            "Failed to persist publisher_mismatch lead for audit: {}",
                            e
                        );
                    }
                });
                return Ok((StatusCode::BAD_REQUEST, Json(response_body)));
            }
        } else {
            // Invalid UUID format
            let response_body = LeadResponse {
                status: StatusNode {
                    success: false,
                    status: "error".to_string(),
                    message: Some(format!(
                        "Invalid publisher_id format: {}",
                        provided_publisher_id
                    )),
                    error: Some(format!(
                        "Invalid UUID format for publisher_id: {}",
                        provided_publisher_id
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
            };
            let request_body = LeadRequest {
                verbose: payload.verbose,
                lead: lead_data.clone(),
            };
            let pool = state.db_pool.clone();
            let publisher_id = publisher.id;
            let vertical_clone = vertical.clone();
            let lead_data_clone = lead_data.clone();
            let request_type_clone = request_type.clone();
            let request_body_clone = request_body.clone();
            let response_body_clone = response_body.clone();
            let msg = format!("Invalid publisher_id format: {}", provided_publisher_id);
            tokio::spawn(async move {
                let vertical_data = serde_json::json!({
                    "error_kind": "invalid_publisher_id_format",
                    "message": &msg,
                });
                if let Err(e) = persist_failed_lead(
                    pool.as_ref(),
                    publisher_id,
                    &vertical_clone,
                    &lead_data_clone,
                    &request_type_clone,
                    &request_body_clone,
                    &response_body_clone,
                    LeadStatus::Error,
                    vertical_data,
                    msg.as_str(),
                )
                .await
                {
                    tracing::warn!(
                        "Failed to persist invalid_publisher_id_format lead for audit: {}",
                        e
                    );
                }
            });
            return Ok((StatusCode::BAD_REQUEST, Json(response_body)));
        }
    }

    // Validate required fields for this request type (API is source of truth)
    if let Some(field) = missing_required_lead_field(&lead_data, &request_type) {
        let error_message = format!("Missing required field: {}", field);
        let response_body = LeadResponse {
            status: StatusNode {
                success: false,
                status: "error".to_string(),
                message: Some(error_message.clone()),
                error: Some(error_message),
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
        };
        let request_body = LeadRequest {
            verbose: payload.verbose,
            lead: lead_data.clone(),
        };
        let pool = state.db_pool.clone();
        let publisher_id = publisher.id;
        let vertical_clone = vertical.clone();
        let lead_data_clone = lead_data.clone();
        let request_type_clone = request_type.clone();
        let field_owned = field.to_string();
        let request_body_clone = request_body.clone();
        let response_body_clone = response_body.clone();
        tokio::spawn(async move {
            if let Err(e) = persist_validation_error_lead(
                pool.as_ref(),
                publisher_id,
                &vertical_clone,
                &lead_data_clone,
                &request_type_clone,
                &field_owned,
                &request_body_clone,
                &response_body_clone,
            )
            .await
            {
                tracing::error!(
                    publisher_id = %publisher_id,
                    missing_field = %field_owned,
                    error = %e,
                    "Failed to persist validation-error lead: lead will not appear in dashboard. Check DB constraints and that publisher.instance_id is correct for this API's DATABASE_URL."
                );
            }
        });
        return Ok((StatusCode::BAD_REQUEST, Json(response_body)));
    }

    // Handle post request (update existing lead)
    if request_type == "post" {
        let promise_id = lead_data
            .promise_id
            .as_ref()
            .expect("promise_id validated above")
            .clone();
        let lead_id_str = lead_data.lead_id.as_deref().filter(|s| !s.is_empty());
        // CACHE: Lead lookup by promise_id (5m TTL). Use skip_none so we do NOT cache None.
        let lead_cache_key = format!("lead:promise_id:{}", promise_id);
        let by_promise: anyhow::Result<Option<leadsnebula_core::models::lead::Lead>> =
            if let Some(cache) = &state.cache {
                cache
                    .get_or_insert_with_skip_none(&lead_cache_key, 300, || async {
                        leadsnebula_core::models::lead::Lead::find_by_promise_id(
                            &state.db_pool,
                            &promise_id,
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("Database error: {}", e))
                    })
                    .await
            } else {
                leadsnebula_core::models::lead::Lead::find_by_promise_id(
                    &state.db_pool,
                    &promise_id,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Database error: {}", e))
            };
        let lead = match by_promise {
            Ok(Some(l)) => l,
            Ok(None) => {
                if let Some(lid) = lead_id_str {
                    // Fallback: find by lead_id (row is keyed by lead_id; ON CONFLICT keeps same row)
                    match leadsnebula_core::models::lead::Lead::find_by_lead_id(&state.db_pool, lid)
                        .await
                    {
                        Ok(Some(l)) => l,
                        _ => {
                            return Ok((
                                StatusCode::NOT_FOUND,
                                Json(LeadResponse {
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
                                }),
                            ));
                        }
                    }
                } else {
                    return Ok((
                        StatusCode::NOT_FOUND,
                        Json(LeadResponse {
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
                        }),
                    ));
                }
            }
            Err(e) => {
                tracing::error!("Database error finding lead: {}", e);
                let (message, technical) = map_error_to_user(&e.to_string());
                return Ok((
                    StatusCode::BAD_REQUEST,
                    Json(LeadResponse {
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
                    }),
                ));
            }
        };

        // Attempt an atomic conditional claim to prevent double-sell.
        // We set a temporary in-progress token into `post_id` only if it's empty and the promise is not expired.
        let inprog_token = format!("INPROG_{}", uuid::Uuid::new_v4());
        // Wire tokio-retry for transient DB errors
        let retry_strategy = ExponentialBackoff::from_millis(50)
            .max_delay(Duration::from_millis(200))
            .take(3);
        let claim_result = Retry::spawn(retry_strategy, || async {
            sqlx::query_scalar::<_, uuid::Uuid>(
                "UPDATE leads SET post_id = $1 WHERE uuid = $2 AND (post_id IS NULL OR post_id = '') AND promise_id = $3 AND created_at >= NOW() - INTERVAL '10 minutes' RETURNING uuid",
            )
            .bind(inprog_token.clone())
            .bind(lead.uuid)
            .bind(&promise_id)
            .fetch_optional(&*state.db_pool)
            .await
        })
        .await;

        match claim_result {
            Ok(Some(_)) => {
                // We have claimed this promise for this process. Proceed to route the post.
            }
            Ok(None) => {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    Json(LeadResponse {
                        status: StatusNode {
                            success: false,
                            status: "error".to_string(),
                            message: Some(
                                "The lead_id or promise_id have already been posted, or do not match your ping response. Use the lead_id and promise_id from your ping response."
                                    .to_string(),
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
                                "note": "lead_id or promise_id already posted or mismatch; use lead_id and promise_id from ping response"
                            }))
                        } else {
                            None
                        },
                        http_status: Some(400),
                    }),
                ));
            }
            Err(e) => {
                tracing::error!("Database error claiming promise: {}", e);
                let (message, technical) = map_error_to_user(&e.to_string());
                return Ok((
                    StatusCode::BAD_REQUEST,
                    Json(LeadResponse {
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
                    }),
                ));
            }
        }

        // Route the post through the ping-tree router to perform buyer post handling
        let timing_arc = timing.clone();
        let metrics_arc = metrics.clone();
        let router = leadsnebula_core::services::ping_tree_router::PingTreeRouter::new(
            lead.clone(),
            publisher.id,
            vertical.slug.clone(),
            request_type.clone(),
            state.cache.clone(),
            Some(state.write_behind_queue.clone()),
        )
        .with_timing_and_metrics(timing_arc, metrics_arc);
        match router
            .route(
                state.db_pool.clone(),
                std::sync::Arc::new(state.config.encryption_key.clone()),
            )
            .await
        {
            Ok(routing_result) => {
                // Batch load buyer and campaign names in parallel (CACHED - 1h TTL, names rarely change)
                let (buyer_name, campaign_name) = tokio::join!(
                    async {
                        if let Some(bid) = routing_result.buyer_id {
                            let cache_key = format!("buyer:name:{}", bid);
                            if let Some(cache) = &state.cache {
                                cache
                                    .get_or_insert_with(&cache_key, 3600, || async {
                                        sqlx::query_scalar::<_, String>(
                                            "SELECT name FROM buyers WHERE id = $1 AND deleted_at IS NULL",
                                        )
                                        .bind(bid)
                                        .fetch_optional(&*state.db_pool)
                                        .await
                                        .map_err(|e| anyhow::anyhow!("Database error: {}", e))
                                    })
                                    .await
                                    .ok()
                                    .flatten()
                            } else {
                                // Fallback if cache not available
                                sqlx::query_scalar::<_, String>(
                                    "SELECT name FROM buyers WHERE id = $1 AND deleted_at IS NULL",
                                )
                                .bind(bid)
                                .fetch_optional(&*state.db_pool)
                                .await
                                .unwrap_or_default()
                            }
                        } else {
                            None
                        }
                    },
                    async {
                        if let Some(cid) = routing_result.campaign_id {
                            let cache_key = format!("campaign:name:{}", cid);
                            if let Some(cache) = &state.cache {
                                cache
                                    .get_or_insert_with(&cache_key, 3600, || async {
                                        sqlx::query_scalar::<_, String>(
                                            "SELECT name FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
                                        )
                                        .bind(cid)
                                        .fetch_optional(&*state.db_pool)
                                        .await
                                        .map_err(|e| anyhow::anyhow!("Database error: {}", e))
                                    })
                                    .await
                                    .ok()
                                    .flatten()
                            } else {
                                // Fallback if cache not available
                                sqlx::query_scalar::<_, String>(
                                    "SELECT name FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
                                )
                                .bind(cid)
                                .fetch_optional(&*state.db_pool)
                                .await
                                .unwrap_or_default()
                            }
                        } else {
                            None
                        }
                    }
                );

                // Round price to 2 decimals for response
                let rounded_price = routing_result.price.map(|p| (p * 100.0).round() / 100.0);

                // Build status node/message
                // OPTIMIZED: Avoid unnecessary clones, use references where possible
                let success = routing_result.success;
                let status = routing_result.status.clone(); // Need to clone for StatusNode
                let status_ref = &status; // Use reference for comparisons
                let message = if *status_ref == "sold" {
                    if let Some(ref name) = buyer_name {
                        // Use reference
                        if let Some(p) = rounded_price {
                            // OPTIMIZED: Pre-allocate string with estimated capacity
                            let mut msg = String::with_capacity(name.len() + 20);
                            msg.push_str("Lead sold to ");
                            msg.push_str(name);
                            msg.push_str(" for $");
                            msg.push_str(&p.to_string());
                            Some(msg)
                        } else {
                            // OPTIMIZED: Pre-allocate string
                            let mut msg = String::with_capacity(name.len() + 12);
                            msg.push_str("Lead sold to ");
                            msg.push_str(name);
                            Some(msg)
                        }
                    } else if let Some(p) = rounded_price {
                        // OPTIMIZED: Pre-allocate string
                        let mut msg = String::with_capacity(20);
                        msg.push_str("Lead sold for $");
                        msg.push_str(&p.to_string());
                        Some(msg)
                    } else {
                        Some("Lead sold".to_string())
                    }
                } else {
                    routing_result
                        .error
                        .as_ref()
                        .cloned()
                        .or_else(|| Some(routing_result.status.clone()))
                };

                // Persist post payload (request + response) into post_payloads with encryption when possible
                // OPTIMIZED: Defer JSON serialization - only serialize when needed (for encryption/queue)
                // Build JSON manually using serde_json::Value::Object to avoid macro overhead
                // OPTIMIZED: Pre-allocate Map with estimated capacity (8 fields)
                use serde_json::Map;
                let mut routing_result_map = Map::with_capacity(8);
                // OPTIMIZED: Use string literals directly instead of .to_string() for keys
                routing_result_map.insert(
                    "status".to_string(),
                    serde_json::Value::String(routing_result.status.clone()),
                );
                routing_result_map.insert(
                    "success".to_string(),
                    serde_json::Value::Bool(routing_result.success),
                );
                if let Some(ref error) = routing_result.error {
                    routing_result_map.insert(
                        "error".to_string(),
                        serde_json::Value::String(error.clone()),
                    );
                } else {
                    routing_result_map.insert("error".to_string(), serde_json::Value::Null);
                }
                if let Some(price) = routing_result.price {
                    routing_result_map.insert(
                        "price".to_string(),
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(price)
                                .unwrap_or_else(|| serde_json::Number::from(0)),
                        ),
                    );
                } else {
                    routing_result_map.insert("price".to_string(), serde_json::Value::Null);
                }
                if let Some(buyer_id) = routing_result.buyer_id {
                    // OPTIMIZED: Pre-allocate UUID string (36 chars)
                    let mut buyer_id_str = String::with_capacity(36);
                    buyer_id_str.push_str(&buyer_id.to_string());
                    routing_result_map.insert(
                        "buyer_id".to_string(),
                        serde_json::Value::String(buyer_id_str),
                    );
                } else {
                    routing_result_map.insert("buyer_id".to_string(), serde_json::Value::Null);
                }
                if let Some(campaign_id) = routing_result.campaign_id {
                    // OPTIMIZED: Pre-allocate UUID string (36 chars)
                    let mut campaign_id_str = String::with_capacity(36);
                    campaign_id_str.push_str(&campaign_id.to_string());
                    routing_result_map.insert(
                        "campaign_id".to_string(),
                        serde_json::Value::String(campaign_id_str),
                    );
                } else {
                    routing_result_map.insert("campaign_id".to_string(), serde_json::Value::Null);
                }
                if let Some(ref ping_id) = routing_result.ping_id {
                    routing_result_map.insert(
                        "ping_id".to_string(),
                        serde_json::Value::String(ping_id.clone()),
                    );
                } else {
                    routing_result_map.insert("ping_id".to_string(), serde_json::Value::Null);
                }
                if let Some(ref post_id) = routing_result.post_id {
                    routing_result_map.insert(
                        "post_id".to_string(),
                        serde_json::Value::String(post_id.clone()),
                    );
                } else {
                    routing_result_map.insert("post_id".to_string(), serde_json::Value::Null);
                }
                if let Some(ref promise_id) = routing_result.promise_id {
                    routing_result_map.insert(
                        "promise_id".to_string(),
                        serde_json::Value::String(promise_id.clone()),
                    );
                } else {
                    routing_result_map.insert("promise_id".to_string(), serde_json::Value::Null);
                }

                // OPTIMIZED: Only serialize post_request_json when actually needed (for encryption/queue)
                let post_request_json =
                    serde_json::to_value(&lead_data).unwrap_or_else(|_| serde_json::json!({}));
                let mut post_response_json_map = Map::new();
                post_response_json_map.insert(
                    "routing_result".to_string(),
                    serde_json::Value::Object(routing_result_map),
                );
                let post_response_json = serde_json::Value::Object(post_response_json_map);

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
                if let Ok(Some(det_key)) =
                    get_ssm_parameter_cached(&state.ssm, &det_path2, true).await
                {
                    if let Ok(Some(salt)) =
                        get_ssm_parameter_cached(&state.ssm, &salt_path2, true).await
                    {
                        let derived =
                            leadsnebula_core::encryption::EncryptionService::derive_key_from_secret(
                                &det_key, &salt,
                            );
                        if let Ok(bytes) = simd_json::to_vec(&post_request_json) {
                            if let Ok(req_str) = String::from_utf8(bytes) {
                                if let Ok(env) =
                                    leadsnebula_core::encryption::EncryptionService::encrypt_envelope(
                                        &derived, &req_str, true,
                                    )
                                {
                                    enc_req_opt = Some(env);
                                }
                            }
                        }
                        if let Ok(bytes) = simd_json::to_vec(&post_response_json) {
                            if let Ok(resp_str) = String::from_utf8(bytes) {
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
                }

                // Insert into post_payloads via write-behind queue (decoupled from critical path)
                state.write_behind_queue.enqueue(
                    leadsnebula_core::services::write_behind_queue::BackgroundTask::PayloadUpdate {
                        lead_id: lead.uuid,
                        payload_type: "post".to_string(),
                        payload: post_request_json.clone(),
                        post_id: routing_result.post_id.clone(),
                        request_payload_encrypted: enc_req_opt.clone(),
                        response_payload_encrypted: enc_resp_opt.clone(),
                        ping_payloads_row_id: None,
                        external_ping_id: None,
                    },
                );

                // Encryption of buyer_responses moved to background job/cron - spawn overhead eliminated
                // If encryption is needed, it should be handled by a separate background task or write-behind queue

                // Build auction_timing with TRUE total wall time (lead arrival → final result)
                // For POST: timing only covers this request; use (now - lead.created_at) for real total
                let pre_checks_ms = timing.get_pre_checks_ms();
                let post_ms = timing.get_post_sent_ms();
                let db_operations_ms = metrics.get_total_query_time_ms();
                let total_wall_ms = chrono::Utc::now()
                    .signed_duration_since(lead.created_at)
                    .num_milliseconds()
                    .clamp(0, i64::MAX) as u64;
                let mut auction_timing_base = serde_json::Map::new();
                auction_timing_base.insert("request_type".to_string(), serde_json::json!("post"));
                auction_timing_base.insert(
                    "pre_checks_ms".to_string(),
                    serde_json::json!(pre_checks_ms),
                );
                auction_timing_base.insert("post_ms".to_string(), serde_json::json!(post_ms));
                auction_timing_base.insert(
                    "db_operations_ms".to_string(),
                    serde_json::json!(db_operations_ms),
                );
                auction_timing_base
                    .insert("total_ms".to_string(), serde_json::json!(total_wall_ms));
                let outcome_str = if routing_result.success && routing_result.status == "sold" {
                    "success"
                } else {
                    "failure"
                };
                let verbose_obj = request_context_from_headers(
                    &headers,
                    "/api/v1/leads",
                    "POST",
                    "Lead post (buyer post)",
                    "carina_api",
                    outcome_str,
                    "Lead",
                    &lead.uuid.to_string(),
                    "post",
                    chrono::Utc::now(),
                );
                let mut verbose_map = verbose_obj.as_object().cloned().unwrap_or_default();
                let mut processing_times = serde_json::Map::new();
                processing_times.insert(
                    "pre_checks_ms".to_string(),
                    serde_json::json!(pre_checks_ms),
                );
                processing_times.insert("post_ms".to_string(), serde_json::json!(post_ms));
                processing_times.insert(
                    "db_operations_ms".to_string(),
                    serde_json::json!(db_operations_ms),
                );
                processing_times.insert("total_ms".to_string(), serde_json::json!(total_wall_ms));
                verbose_map.insert(
                    "processing_times".to_string(),
                    serde_json::Value::Object(processing_times),
                );
                let auction_timing_data = serde_json::json!({
                    "auction_timing": serde_json::Value::Object(auction_timing_base),
                    "verbose": serde_json::Value::Object(verbose_map)
                });
                // Merge auction_timing + verbose into existing vertical_data if present
                let vertical_data_to_store = if let Some(obj) = lead.vertical_data.as_object() {
                    let mut merged = obj.clone();
                    merged.insert(
                        "auction_timing".to_string(),
                        auction_timing_data["auction_timing"].clone(),
                    );
                    merged.insert(
                        "verbose".to_string(),
                        auction_timing_data["verbose"].clone(),
                    );
                    serde_json::Value::Object(merged)
                } else {
                    auction_timing_data
                };

                // Update lead final state via write-behind queue (decoupled from critical path)
                if routing_result.success && routing_result.status == "sold" {
                    // Set final post_id and mark sold only if our in-progress token is still present
                    state.write_behind_queue.enqueue(
                        leadsnebula_core::services::write_behind_queue::BackgroundTask::LeadUpdate {
                            lead_id: lead.uuid,
                            status: leadsnebula_core::models::enums::LeadStatus::Sold,
                            campaign_id: routing_result.campaign_id,
                            buyer_id: routing_result.buyer_id,
                            promise_id: None,
                            ping_id: None,
                            post_id: routing_result.post_id.clone(),
                            sold_at: true,
                            inprog_token: Some(inprog_token.clone()),
                            vertical_data: Some(vertical_data_to_store),
                        },
                    );
                } else {
                    // Reset placeholder so another post attempt may try
                    state.write_behind_queue.enqueue(
                        leadsnebula_core::services::write_behind_queue::BackgroundTask::LeadUpdate {
                            lead_id: lead.uuid,
                            status: lead.status.clone(),
                            campaign_id: None,
                            buyer_id: None,
                            promise_id: None,
                            ping_id: None,
                            post_id: Some("".to_string()),
                            sold_at: false,
                            inprog_token: Some(inprog_token.clone()),
                            vertical_data: Some(vertical_data_to_store),
                        },
                    );
                }
                return Ok((
                    StatusCode::BAD_REQUEST,
                    Json(LeadResponse {
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
                    }),
                ));
            }
            Err(e) => {
                tracing::error!("Routing error during post: {}", e);
                let (message, technical) = map_error_to_user(&e.to_string());
                return Ok((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(LeadResponse {
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
                    }),
                ));
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
    // OPTIMIZED: Single query combining all pre-checks (reduces 4-6 queries to 1)
    // CACHED: Short TTL (300s) since campaigns can change
    let prechecks_start = std::time::Instant::now();
    let mut preproblems: Vec<String> = Vec::new();
    let mut buyer_id_opt: Option<uuid::Uuid> = None;
    let mut campaign_id_opt: Option<uuid::Uuid> = None;

    let campaign_token = lead_data.campaign_token.as_deref().unwrap_or("");
    let prechecks_cache_key = format!(
        "prechecks:publisher:{}:vertical:{}:token:{}",
        publisher.id, vertical.slug, campaign_token
    );

    // Prepare SSM paths for parallel fetching with pre-checks query
    // Use normalize_env_for_ssm to ensure consistency with cache_warmup (converts "development" -> "dev")
    let env_norm = leadsnebula_core::normalize_env_for_ssm(&state.config.environment);
    let det_path = format!(
        "/leadsnebula/{}/carina/encryption/deterministic_key_v1",
        env_norm
    );
    let salt_path = format!(
        "/leadsnebula/{}/carina/encryption/key_derivation_salt_v1",
        env_norm
    );

    // Parallelize pre-checks query with SSM key fetching (they're independent)
    // Enhanced single query combining all pre-checks (avoids fallback query)
    // CACHED: Short TTL (300s) since campaigns can change
    let (prechecks_result, ssm_results) = tokio::join!(
        async {
            if let Some(cache) = &state.cache {
                // Use cached lookup with 300s TTL
                match cache
                    .get_or_insert_with(
                        &prechecks_cache_key,
                        300, // 5 minutes
                        || async {
                            sqlx::query_as::<_, (Option<uuid::Uuid>, Option<uuid::Uuid>, bool)>(
                                r#"
                        SELECT 
                            c.id AS campaign_id,
                            COALESCE(
                                c.buyer_id,
                                b_ping_tree.buyer_id,
                                b_vertical.id
                            ) AS effective_buyer_id,
                            EXISTS(
                                SELECT 1 FROM ping_tree_publishers ptp
                                INNER JOIN ping_trees pt ON pt.id = ptp.ping_tree_id
                                WHERE ptp.publisher_id = $1 
                                  AND ptp.vertical = $2
                                  AND pt.status = 'active'
                                  AND pt.deleted_at IS NULL
                            ) AS has_ping_tree
                        FROM (VALUES (true)) AS dummy
                        LEFT JOIN campaigns c ON (
                            (c.campaign_token = $3 AND $3 != '' AND c.publisher_id = $1) OR 
                            (c.vertical = $2 AND c.publisher_id = $1 AND c.buyer_id IN (
                                SELECT b.id FROM buyers b 
                                WHERE b.vertical_id = (
                                    SELECT v.id FROM verticals v 
                                    WHERE v.slug = $2 AND v.is_active = true
                                ) AND b.deleted_at IS NULL
                            ))
                        ) AND c.deleted_at IS NULL
                        LEFT JOIN LATERAL (
                            SELECT c_pt.buyer_id
                            FROM ping_tree_publishers ptp_pt
                            INNER JOIN ping_trees pt_pt ON pt_pt.id = ptp_pt.ping_tree_id
                            INNER JOIN ping_tree_campaigns ptc ON ptc.ping_tree_id = pt_pt.id
                            INNER JOIN campaigns c_pt ON c_pt.id = ptc.campaign_id
                            WHERE ptp_pt.publisher_id = $1
                              AND ptp_pt.vertical = $2
                              AND pt_pt.status = 'active'
                              AND pt_pt.deleted_at IS NULL
                              AND ptc.enabled = true
                              AND c_pt.status = 'active'
                              AND c_pt.deleted_at IS NULL
                              AND c_pt.buyer_id IS NOT NULL
                            LIMIT 1
                        ) b_ping_tree ON TRUE
                        LEFT JOIN buyers b_vertical ON 
                            b_vertical.vertical_id = (
                                SELECT v2.id FROM verticals v2 
                                WHERE v2.slug = $2 AND v2.is_active = true
                            ) 
                            AND (c.id IS NULL OR c.buyer_id IS NULL)
                            AND b_ping_tree.buyer_id IS NULL
                            AND b_vertical.deleted_at IS NULL
                        LIMIT 1
                        "#,
                            )
                            .bind(publisher.id)
                            .bind(vertical.slug.clone())
                            .bind(campaign_token)
                            .fetch_optional(&*state.db_pool)
                            .await
                            .map_err(|e| anyhow::anyhow!("Database error: {}", e))
                        },
                    )
                    .await
                {
                    Ok(r) => {
                        let cache_hit = prechecks_start.elapsed().as_millis() < 10; // Very fast = cache hit
                        if cache_hit {
                            metrics.record_cache_hit();
                        } else {
                            metrics.record_cache_miss();
                        }
                        (Ok(r), cache_hit)
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Cache lookup failed for prechecks, falling back to DB: {}",
                            e
                        );
                        metrics.record_cache_miss();
                        // Fallback to direct DB query
                        let db_start = std::time::Instant::now();
                        let result =
                            sqlx::query_as::<_, (Option<uuid::Uuid>, Option<uuid::Uuid>, bool)>(
                                r#"
                    SELECT 
                        c.id AS campaign_id,
                        COALESCE(
                            c.buyer_id,
                            b_ping_tree.buyer_id,
                            b_vertical.id
                        ) AS effective_buyer_id,
                        EXISTS(
                            SELECT 1 FROM ping_trees pt
                            INNER JOIN ping_tree_publishers ptp ON pt.id = ptp.ping_tree_id
                            WHERE ptp.publisher_id = $1 
                              AND ptp.vertical = $2
                              AND pt.status = 'active'
                              AND pt.deleted_at IS NULL
                        ) AS has_ping_tree
                    FROM (VALUES (true)) AS dummy
                    LEFT JOIN campaigns c ON (
                        (c.campaign_token = $3 AND $3 != '' AND c.publisher_id = $1) OR 
                        (c.vertical = $2 AND c.publisher_id = $1 AND c.buyer_id IN (
                            SELECT b.id FROM buyers b 
                            WHERE b.vertical_id = (
                                SELECT v.id FROM verticals v 
                                WHERE v.slug = $2 AND v.is_active = true
                            ) AND b.deleted_at IS NULL
                        ))
                    ) AND c.deleted_at IS NULL
                    LEFT JOIN LATERAL (
                        SELECT c_pt.buyer_id
                        FROM ping_tree_publishers ptp_pt
                        INNER JOIN ping_trees pt_pt ON pt_pt.id = ptp_pt.ping_tree_id
                        INNER JOIN ping_tree_campaigns ptc ON ptc.ping_tree_id = pt_pt.id
                        INNER JOIN campaigns c_pt ON c_pt.id = ptc.campaign_id
                        WHERE ptp_pt.publisher_id = $1
                          AND ptp_pt.vertical = $2
                          AND pt_pt.status = 'active'
                          AND pt_pt.deleted_at IS NULL
                          AND ptc.enabled = true
                          AND c_pt.status = 'active'
                          AND c_pt.deleted_at IS NULL
                          AND c_pt.buyer_id IS NOT NULL
                        LIMIT 1
                    ) b_ping_tree ON TRUE
                    LEFT JOIN buyers b_vertical ON 
                        b_vertical.vertical_id = (
                                SELECT v2.id FROM verticals v2 
                                    WHERE v2.slug = $2 AND v2.is_active = true
                        ) 
                        AND (c.id IS NULL OR c.buyer_id IS NULL)
                        AND b_ping_tree.buyer_id IS NULL
                        AND b_vertical.deleted_at IS NULL
                    LIMIT 1
                    "#,
                            )
                            .bind(publisher.id)
                            .bind(vertical.slug.clone())
                            .bind(campaign_token)
                            .fetch_optional(&*state.db_pool)
                            .await;
                        metrics.record_query(db_start.elapsed().as_millis() as u64);
                        (result, false)
                    }
                }
            } else {
                // No cache available, use direct DB query
                let db_start = std::time::Instant::now();
                let result = sqlx::query_as::<_, (Option<uuid::Uuid>, Option<uuid::Uuid>, bool)>(
                    r#"
            SELECT 
                c.id AS campaign_id,
                COALESCE(
                    c.buyer_id,
                    b_ping_tree.buyer_id,
                    b_vertical.id
                ) AS effective_buyer_id,
                EXISTS(
                    SELECT 1 FROM ping_trees pt
                    INNER JOIN ping_tree_publishers ptp ON pt.id = ptp.ping_tree_id
                    WHERE ptp.publisher_id = $1 
                      AND ptp.vertical = $2
                      AND pt.status = 'active'
                      AND pt.deleted_at IS NULL
                ) AS has_ping_tree
            FROM (VALUES (true)) AS dummy
            LEFT JOIN campaigns c ON (
                (c.campaign_token = $3 AND $3 != '' AND c.publisher_id = $1) OR 
                (c.vertical = $2 AND c.publisher_id = $1 AND c.buyer_id IN (
                    SELECT b.id FROM buyers b 
                    WHERE b.vertical_id = (
                        SELECT v.id FROM verticals v 
                        WHERE v.slug = $2 AND v.is_active = true
                    ) AND b.deleted_at IS NULL
                ))
            ) AND c.deleted_at IS NULL
            LEFT JOIN LATERAL (
                SELECT c_pt.buyer_id
                FROM ping_tree_publishers ptp_pt
                INNER JOIN ping_trees pt_pt ON pt_pt.id = ptp_pt.ping_tree_id
                INNER JOIN ping_tree_campaigns ptc ON ptc.ping_tree_id = pt_pt.id
                INNER JOIN campaigns c_pt ON c_pt.id = ptc.campaign_id
                WHERE ptp_pt.publisher_id = $1
                  AND ptp_pt.vertical = $2
                  AND pt_pt.status = 'active'
                  AND pt_pt.deleted_at IS NULL
                  AND ptc.enabled = true
                  AND c_pt.status = 'active'
                  AND c_pt.deleted_at IS NULL
                  AND c_pt.buyer_id IS NOT NULL
                LIMIT 1
            ) b_ping_tree ON TRUE
            LEFT JOIN buyers b_vertical ON 
                b_vertical.vertical_id = (
                                SELECT v2.id FROM verticals v2 
                                    WHERE v2.slug = $2 AND v2.is_active = true
                ) 
                AND (c.id IS NULL OR c.buyer_id IS NULL)
                AND b_ping_tree.buyer_id IS NULL
                AND b_vertical.deleted_at IS NULL
            LIMIT 1
            "#,
                )
                .bind(publisher.id)
                .bind(vertical.slug.clone())
                .bind(campaign_token)
                .fetch_optional(&*state.db_pool)
                .await;
                metrics.record_query(db_start.elapsed().as_millis() as u64);
                (result, false)
            }
        },
        async {
            // Fetch SSM keys in parallel with pre-checks query
            (
                get_ssm_parameter_cached(&state.ssm, &det_path, true).await,
                get_ssm_parameter_cached(&state.ssm, &salt_path, true).await,
            )
        }
    );

    // Extract results
    let (result, _prechecks_cache_hit) = prechecks_result;
    let (mut det_key_result, mut salt_result) = ssm_results;

    // If SSM keys are missing, force pre-warm as fallback and retry once
    // OPTIMIZED: Skip pre-warm retry in local dev to avoid SSM timeout delays
    let is_local_dev = std::path::Path::new(".env.local").exists();
    if !is_local_dev
        && (det_key_result.is_err()
            || det_key_result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref())
                .is_none()
            || salt_result.is_err()
            || salt_result.as_ref().ok().and_then(|r| r.as_ref()).is_none())
    {
        tracing::warn!("SSM keys missing, forcing immediate pre-warm as fallback...");
        use crate::cache_warmup::pre_warm_ssm_keys;
        let _ = pre_warm_ssm_keys(&state.ssm, &state.config.environment).await;

        // Retry once after pre-warm
        if det_key_result.is_err()
            || det_key_result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref())
                .is_none()
        {
            det_key_result = get_ssm_parameter_cached(&state.ssm, &det_path, true).await;
        }
        if salt_result.is_err() || salt_result.as_ref().ok().and_then(|r| r.as_ref()).is_none() {
            salt_result = get_ssm_parameter_cached(&state.ssm, &salt_path, true).await;
        }
    } else if is_local_dev
        && (det_key_result.is_err()
            || det_key_result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref())
                .is_none()
            || salt_result.is_err()
            || salt_result.as_ref().ok().and_then(|r| r.as_ref()).is_none())
    {
        tracing::debug!(
            "SSM keys missing in local dev - skipping pre-warm retry to avoid timeout delays"
        );
    }

    let prechecks_duration = prechecks_start.elapsed().as_millis() as u64;
    timing.record_pre_checks(prechecks_duration);

    match result {
        Ok(Some((campaign_id, effective_buyer_id, has_ping_tree))) => {
            campaign_id_opt = campaign_id;
            buyer_id_opt = effective_buyer_id;

            // Log ping tree status
            if !has_ping_tree {
                tracing::info!(
                    "No ping tree configured for this publisher/vertical; routing may fail"
                );
            }

            // Ronin campaign: campaign is active but not in any ping tree; campaign_token is required so we know which campaign to attribute the lead to
            if !has_ping_tree
                && campaign_id_opt.is_some()
                && lead_data
                    .campaign_token
                    .as_deref()
                    .is_none_or(|s| s.trim().is_empty())
            {
                preproblems.push(
                    "This campaign is not in a ping tree (ronin). campaign_token is required when submitting leads to a ronin campaign.".to_string(),
                );
            }
            if campaign_id_opt.is_none() && lead_data.campaign_token.is_some() {
                preproblems.push("No campaign configured for this publisher/vertical".to_string());
            }
            if buyer_id_opt.is_none() {
                // Log detailed debug info when buyer is not found
                tracing::warn!(
                    publisher_id = %publisher.id,
                    vertical = %vertical.slug,
                    campaign_id = ?campaign_id,
                    has_ping_tree = has_ping_tree,
                    "Buyer not found in prechecks - query returned NULL buyer_id"
                );
                preproblems.push("No buyer configured for this publisher/vertical".to_string());
            }
        }
        Ok(None) => {
            // No results - try fallback buyer lookup
            match sqlx::query_scalar::<_, uuid::Uuid>(
                "SELECT b.id FROM buyers b WHERE b.vertical_id = (SELECT v.id FROM verticals v WHERE v.slug = $1 AND v.is_active = true) AND b.deleted_at IS NULL LIMIT 1",
            )
            .bind(vertical.slug.clone())
            .fetch_optional(&*state.db_pool)
            .await
            {
                Ok(Some(bid)) => {
                    buyer_id_opt = Some(bid);
                }
                Ok(None) => {
                    preproblems.push("No buyer configured for this publisher/vertical".to_string());
                }
                Err(e) => {
                    tracing::error!("Error checking buyers: {}", e);
                    preproblems.push("Failed to verify buyers due to server error".to_string());
                }
            }

            if campaign_id_opt.is_none() && lead_data.campaign_token.is_some() {
                preproblems.push("No campaign configured for this publisher/vertical".to_string());
            }
        }
        Err(e) => {
            tracing::error!("Error in combined pre-check query: {}", e);
            preproblems.push("Failed to verify configuration due to server error".to_string());
        }
    }

    if !preproblems.is_empty() {
        let message = preproblems.join("\n");
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(LeadResponse {
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
            }),
        ));
    }

    // Critical path timing: start AFTER pre-checks and DB operations
    // This measures only the non-DB critical path (routing, response building)
    // Always available (not feature-gated) for production performance monitoring
    let critical_path_start = std::time::Instant::now();

    // Generate identifiers only after pre-checks pass.
    // Ping/fullpost: lead_id does not exist before the request; server generates it on arrival and returns it in the response.
    // Post must use the same lead_id and promise_id from the ping response (no new lead_id on post).
    let id_generation_start = std::time::Instant::now();
    let lead_id = {
        let prefix = vertical.slug.to_uppercase();
        let mut result = String::with_capacity(prefix.len() + 9);
        result.push_str(&prefix);
        result.push('-');
        let mut rng = rand::thread_rng();
        for _ in 0..8 {
            let c = rng.sample(Alphanumeric);
            result.push(char::from(c).to_ascii_uppercase());
        }
        result
    };
    let lead_uuid = uuid::Uuid::new_v4();

    tracing::debug!(
        lead_uuid = %lead_uuid,
        stage = "critical_path_start",
        "Starting critical path timing"
    );

    // OPTIMIZED: Use String::with_capacity instead of format!
    let event_uuid = uuid::Uuid::new_v4();
    let mut event_id = String::with_capacity(41); // "evt_" + 36 chars for UUID
    event_id.push_str("evt_");
    event_id.push_str(&event_uuid.to_string());

    // Generate promise_id for ping requests (immediately, no DB needed)
    // OPTIMIZED: Use String::with_capacity + direct hex encoding
    let promise_id = if request_type == "ping" || request_type == "fullpost" {
        let rand_bytes = rand::random::<[u8; 6]>();
        let mut promise = String::with_capacity(20); // "PROMISE_" + 12 hex chars
        promise.push_str("PROMISE_");
        // Encode hex directly to uppercase
        for byte in rand_bytes.iter() {
            promise.push_str(&format!("{:02X}", byte));
        }
        Some(promise)
    } else {
        None
    };

    // OPTIMIZED: Use String::with_capacity instead of format!
    let session_uuid = uuid::Uuid::new_v4();
    let mut session_id = String::with_capacity(41); // "sess_" + 36 chars for UUID
    session_id.push_str("sess_");
    session_id.push_str(&session_uuid.to_string());
    let id_generation_duration = id_generation_start.elapsed().as_millis() as u64;
    // DEBUG: Detailed timing (only in debug mode)
    tracing::debug!(
        id_generation_ms = id_generation_duration,
        "ID generation completed"
    );

    // OPTIMIZED: Serialize request payload once (needed for queue, but we avoid cloning the Value)
    let payload_serialization_start = std::time::Instant::now();
    // Using serde_json::to_value is already efficient, and we'll only clone the Value once
    let request_payload_json =
        serde_json::to_value(&lead_data).unwrap_or_else(|_| serde_json::json!({}));
    let payload_serialization_duration = payload_serialization_start.elapsed().as_millis() as u64;
    // DEBUG: Detailed timing (only in debug mode)
    tracing::debug!(
        payload_serialization_ms = payload_serialization_duration,
        "Payload serialization completed"
    );

    // Get SSM encryption key for background encryption (derive once, reuse in batch)
    let ssm_key_derivation_start = std::time::Instant::now();
    let pii_encryption_key = match (det_key_result, salt_result) {
        (Ok(Some(det_key)), Ok(Some(salt))) => Some(
            leadsnebula_core::encryption::EncryptionService::derive_key_from_secret(
                &det_key, &salt,
            )
            .to_vec(), // Convert [u8; 32] to Vec<u8>
        ),
        (Ok(Some(_)), Ok(None)) | (Ok(Some(_)), Err(_)) => {
            tracing::warn!("Failed to get key derivation salt from SSM at {} - PII fields will not be encrypted", salt_path);
            None
        }
        (Ok(None), _) | (Err(_), _) => {
            tracing::warn!(
                "Failed to get deterministic key from SSM at {} - PII fields will not be encrypted",
                det_path
            );
            None
        }
    };
    let ssm_key_derivation_duration = ssm_key_derivation_start.elapsed().as_millis() as u64;
    // DEBUG: Detailed timing (only in debug mode)
    tracing::debug!(
        ssm_key_derivation_ms = ssm_key_derivation_duration,
        "SSM key derivation completed"
    );

    // Enqueue lead creation to write-behind queue (decoupled from critical path)
    let queue_enqueue_start = std::time::Instant::now();
    // All encryption happens in the background batch processor
    // DEBUG: Detailed enqueue info (only in debug mode)
    tracing::debug!(
        "Enqueueing lead creation: event_id={}, lead_id={}, publisher_id={}, vertical_id={}, request_type={}, lead_uuid={}",
        event_id,
        lead_id,
        publisher.id,
        vertical.id,
        request_type,
        lead_uuid
    );

    // OPTIMIZED: Minimize clones - use references where possible, move ownership where we can
    // For strings that are only used here, we can move them instead of cloning
    let lead_id_for_queue = if lead_id.is_empty() {
        None
    } else {
        Some(lead_id.clone()) // Need to clone since we use lead_id later
    };

    // OPTIMIZED: Clone strings that are needed both in queue and later
    let event_id_for_queue = event_id.clone();
    let session_id_for_queue = session_id.clone();

    state.write_behind_queue.enqueue(
        leadsnebula_core::services::write_behind_queue::BackgroundTask::LeadCreation {
            uuid: lead_uuid, // CRITICAL: Pass the UUID that will be returned to client
            event_id: event_id_for_queue, // Use cloned value
            lead_id: lead_id_for_queue,
            publisher_id: publisher.id,
            vertical_id: vertical.id,
            request_type: request_type.clone(), // Need to clone, used later
            strategy: strategy.to_string(),
            promise_id: promise_id.as_ref().cloned(), // Clone Option<String> efficiently
            buyer_id: buyer_id_opt.expect("buyer_id must be present after pre-checks"),
            campaign_id: campaign_id_opt.expect("campaign_id must be present after pre-checks"),
            tcpa_consent: lead_data.tcpa_consent.unwrap_or(false),
            tcpa_language: lead_data.tcpa_language.as_deref().unwrap_or("").to_string(),
            is_test: lead_data.is_test.unwrap_or(false),
            session_id: session_id_for_queue, // Use cloned value
            vertical_data: serde_json::json!({}),
            // Raw PII fields (will be encrypted in batch processor)
            // These clones are necessary since lead_data is used later
            first_name: lead_data.first_name.clone(),
            last_name: lead_data.last_name.clone(),
            email: lead_data.email.clone(),
            cell_phone: lead_data.cell_phone.clone(),
            street_address: lead_data.street_address.clone(),
            city: lead_data.city.clone(),
            state: lead_data.state.clone(),
            zip: lead_data.zip.clone(),
            ip_address: lead_data.ip_address.clone(),
            request_payload: request_payload_json, // Move ownership (not used after this)
            pii_encryption_key,                    // Move ownership (not used after this)
        },
    );
    let queue_enqueue_duration = queue_enqueue_start.elapsed().as_millis() as u64;
    // DEBUG: Detailed timing (only in debug mode)
    tracing::debug!(
        queue_enqueue_ms = queue_enqueue_duration,
        "Queue enqueue completed"
    );

    // User-Agent: from lead body or fallback to request header
    let user_agent_for_lead = lead_data.user_agent.clone().or_else(|| {
        headers
            .get("User-Agent")
            .and_then(|v| v.to_str().ok())
            .map(String::from)
    });

    // ASYNC MODE: Return 202 Accepted immediately after enqueueing (routing happens in background)
    // This eliminates routing latency from response time (typically 2-400ms savings)
    // Client receives lead_uuid immediately and can poll for status if needed
    if async_mode {
        // Spawn routing task in background (non-blocking)
        let state_clone = state.clone();
        let lead_clone = leadsnebula_core::models::lead::Lead {
            uuid: lead_uuid,
            event_id: event_id.clone(),
            lead_id: if lead_id.is_empty() {
                None
            } else {
                Some(lead_id.clone())
            },
            publisher_id: Some(publisher.id),
            vertical_id: vertical.id,
            campaign_id: campaign_id_opt,
            buyer_id: buyer_id_opt,
            request_type: request_type.clone(),
            strategy: strategy.to_string(),
            status: if lead_data.is_test.unwrap_or(false) {
                leadsnebula_core::models::enums::LeadStatus::Test
            } else {
                leadsnebula_core::models::enums::LeadStatus::Processing
            },
            promise_id: promise_id.clone(),
            ping_id: None,
            post_id: None,
            session_id: Some(session_id.clone()),
            request_stage: None,
            first_name_encrypted: None,
            last_name_encrypted: None,
            email_encrypted: None,
            cell_phone_encrypted: None,
            street_address_encrypted: None,
            city_encrypted: None,
            state_encrypted: None,
            zip_encrypted: None,
            ip_address_encrypted: None,
            email_sha256: None,
            phone_sha256: None,
            ip_address_hash: None,
            email_domain: None,
            tcpa_consent: lead_data.tcpa_consent.unwrap_or(false),
            tcpa_language: lead_data.tcpa_language.as_deref().unwrap_or("").to_string(),
            is_test: lead_data.is_test.unwrap_or(false),
            user_agent: user_agent_for_lead.clone(),
            referrer: None,
            website_url: lead_data.source.clone(),
            click_id: None,
            url_consent: None,
            best_call_time: None,
            date_of_birth: None,
            home_phone: None,
            jornaya_lead_id: None,
            trusted_form_url: None,
            fbp_cookie: None,
            fbc_cookie: None,
            utm_params: None,
            submitted_at: Some(chrono::Utc::now()),
            sold_at: None,
            retry_count: 0,
            next_retry_at: None,
            vertical_data: serde_json::json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let publisher_id_clone = publisher.id;
        let vertical_slug_clone = vertical.slug.clone();
        let request_type_clone = request_type.clone();
        let timing_clone = timing.clone();
        let metrics_clone = metrics.clone();
        let encryption_key_clone = std::sync::Arc::new(state.config.encryption_key.clone());

        // Spawn background routing task (non-blocking, fire-and-forget)
        tokio::spawn(async move {
            let router = leadsnebula_core::services::ping_tree_router::PingTreeRouter::new(
                lead_clone,
                publisher_id_clone,
                vertical_slug_clone,
                request_type_clone,
                state_clone.cache.clone(),
                Some(state_clone.write_behind_queue.clone()),
            )
            .with_timing_and_metrics(timing_clone.clone(), metrics_clone.clone());

            match router
                .route(state_clone.db_pool.clone(), encryption_key_clone)
                .await
            {
                Ok(_routing_result) => {
                    // Routing completed successfully in background
                    // Lead status is updated via write-behind queue
                    tracing::debug!("Background routing completed successfully");
                }
                Err(e) => {
                    tracing::error!("Background routing failed: {}", e);
                    // Error is logged but doesn't affect the 202 response
                    // Client can check lead status via polling if needed
                }
            }
        });

        // Return 202 Accepted immediately with lead_uuid
        // Client can poll for status using lead_uuid if needed
        // promise_id only for ping (client uses it for post); fullpost does not expose it
        return Ok((
            StatusCode::ACCEPTED,
            Json(LeadResponse {
                status: StatusNode {
                    success: true,
                    status: "processing".to_string(),
                    message: Some("Lead accepted for processing. Routing in progress.".to_string()),
                    error: None,
                },
                lead: LeadNode {
                    promise_id: if request_type == "fullpost" {
                        None
                    } else {
                        promise_id.clone()
                    },
                    lead_id: if lead_id.is_empty() {
                        None
                    } else {
                        Some(lead_id)
                    },
                    lead_uuid: Some(lead_uuid.to_string()),
                    ping_id: None,
                    bid: None,
                    post_id: None,
                    price: None,
                },
                verbose: if verbose_requested {
                    Some(serde_json::json!({
                        "async_mode": true,
                        "message": "Lead accepted for asynchronous processing. Use lead_uuid to check status.",
                        "lead_uuid": lead_uuid.to_string(),
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }))
                } else {
                    None
                },
                http_status: Some(202),
            }),
        ));
    }

    // SYNCHRONOUS MODE (default): Continue with normal routing flow
    // Create minimal Lead object for routing (no DB query needed)
    let lead_object_creation_start = std::time::Instant::now();
    // This allows routing to proceed immediately while DB insert happens in background
    // OPTIMIZED: Reuse event_id string (already created above)
    let lead = leadsnebula_core::models::lead::Lead {
        uuid: lead_uuid,
        event_id: event_id.clone(), // Need to clone since we moved it to queue
        lead_id: if lead_id.is_empty() {
            None
        } else {
            Some(lead_id.clone()) // Need to clone since we use it later
        },
        publisher_id: Some(publisher.id),
        vertical_id: vertical.id,
        campaign_id: campaign_id_opt,
        buyer_id: buyer_id_opt,
        request_type: request_type.clone(), // Need to clone, used later
        strategy: strategy.to_string(),
        status: if lead_data.is_test.unwrap_or(false) {
            leadsnebula_core::models::enums::LeadStatus::Test
        } else {
            leadsnebula_core::models::enums::LeadStatus::Processing
        },
        promise_id: promise_id.clone(),
        ping_id: None,
        post_id: None,
        session_id: Some(session_id),
        request_stage: None,
        first_name_encrypted: None, // Encryption happens in background
        last_name_encrypted: None,
        email_encrypted: None,
        cell_phone_encrypted: None,
        street_address_encrypted: None,
        city_encrypted: None,
        state_encrypted: None,
        zip_encrypted: None,
        ip_address_encrypted: None,
        email_sha256: None,
        phone_sha256: None,
        ip_address_hash: None,
        email_domain: None,
        tcpa_consent: lead_data.tcpa_consent.unwrap_or(false),
        tcpa_language: lead_data.tcpa_language.as_deref().unwrap_or("").to_string(),
        is_test: lead_data.is_test.unwrap_or(false),
        user_agent: user_agent_for_lead,
        referrer: None,
        website_url: lead_data.source.clone(),
        click_id: None,
        url_consent: None,
        best_call_time: None,
        date_of_birth: None,
        home_phone: None,
        jornaya_lead_id: None,
        trusted_form_url: None,
        fbp_cookie: None,
        fbc_cookie: None,
        utm_params: None,
        submitted_at: Some(chrono::Utc::now()),
        sold_at: None,
        retry_count: 0,
        next_retry_at: None,
        vertical_data: serde_json::json!({}),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let lead_object_creation_duration = lead_object_creation_start.elapsed().as_millis() as u64;
    // DEBUG: Detailed timing (only in debug mode)
    tracing::debug!(
        lead_object_creation_ms = lead_object_creation_duration,
        "Lead object creation completed"
    );

    // No need to wait for ping/ping_payloads - they're audit records
    let payload_row_id: Option<uuid::Uuid> = None;

    // Route the lead through ping tree
    // DEBUG: Detailed routing steps (only in debug mode)
    tracing::debug!(stage = "routing_start", "Starting routing phase");
    let routing_start = std::time::Instant::now();
    let timing_arc = timing.clone();
    let metrics_arc = metrics.clone();

    // DETAILED TIMING: Log router creation
    let router_create_start = std::time::Instant::now();
    let router = leadsnebula_core::services::ping_tree_router::PingTreeRouter::new(
        lead,
        publisher.id,
        vertical.slug.clone(),
        request_type.clone(),
        state.cache.clone(),
        Some(state.write_behind_queue.clone()),
    )
    .with_timing_and_metrics(timing_arc.clone(), metrics_arc.clone());
    let router_create_duration = router_create_start.elapsed().as_millis() as u64;
    // DEBUG: Detailed timing (only in debug mode)
    tracing::debug!(router_create_ms = router_create_duration, "Router created");

    // DETAILED TIMING: Log route call
    let route_call_start = std::time::Instant::now();
    let routing_result = router
        .route(
            state.db_pool.clone(),
            std::sync::Arc::new(state.config.encryption_key.clone()),
        )
        .await;
    let route_call_duration = route_call_start.elapsed().as_millis() as u64;
    let routing_duration = routing_start.elapsed().as_millis() as u64;

    // DETAILED TIMING: Log routing breakdown (DEBUG level to reduce overhead)
    tracing::debug!(
        routing_total_ms = routing_duration,
        route_call_ms = route_call_duration,
        router_create_ms = router_create_duration,
        "Routing completed"
    );

    timing_arc.record_total();

    match routing_result {
        Ok(routing_result) => {
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

            // Build a clearer message. For accepted pings include winning bid and instruction to use same lead_id/promise_id in POST.
            let message = if routing_result.status == "accepted" {
                let base = if let Some(b) = bid {
                    format!("Ping Accepted with a bid of ${:.2}", b)
                } else {
                    "Ping Accepted".to_string()
                };
                let instruction = " Include the same lead_id and promise_id from this response in your POST request to complete the lead.";
                Some(format!("{}{}", base, instruction))
            } else if routing_result.status == "sold" {
                if let Some(p) = price {
                    Some(format!("Lead Sold for ${}", p))
                } else {
                    Some("Lead Sold".to_string())
                }
            } else if routing_result.success {
                Some("Lead routed successfully".to_string())
            } else {
                None
            };

            // Fetch buyer and campaign names when verbose requested (for routing display)
            let (buyer_name, campaign_name) = if minimal_mode || !verbose_requested {
                (None, None)
            } else {
                tokio::join!(
                    async {
                        if let Some(bid) = routing_result.buyer_id {
                            let cache_key = format!("buyer:name:{}", bid);
                            if let Some(cache) = &state.cache {
                                cache
                                    .get_or_insert_with(&cache_key, 3600, || async {
                                        sqlx::query_scalar::<_, String>(
                                            "SELECT name FROM buyers WHERE id = $1 AND deleted_at IS NULL",
                                        )
                                        .bind(bid)
                                        .fetch_optional(&*state.db_pool)
                                        .await
                                        .map_err(|e| anyhow::anyhow!("Database error: {}", e))
                                    })
                                    .await
                                    .ok()
                                    .flatten()
                            } else {
                                sqlx::query_scalar::<_, String>(
                                    "SELECT name FROM buyers WHERE id = $1 AND deleted_at IS NULL",
                                )
                                .bind(bid)
                                .fetch_optional(&*state.db_pool)
                                .await
                                .unwrap_or_default()
                            }
                        } else {
                            None
                        }
                    },
                    async {
                        if let Some(cid) = routing_result.campaign_id {
                            let cache_key = format!("campaign:name:{}", cid);
                            if let Some(cache) = &state.cache {
                                cache
                                    .get_or_insert_with(&cache_key, 3600, || async {
                                        sqlx::query_scalar::<_, String>(
                                            "SELECT name FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
                                        )
                                        .bind(cid)
                                        .fetch_optional(&*state.db_pool)
                                        .await
                                        .map_err(|e| anyhow::anyhow!("Database error: {}", e))
                                    })
                                    .await
                                    .ok()
                                    .flatten()
                            } else {
                                sqlx::query_scalar::<_, String>(
                                    "SELECT name FROM campaigns WHERE id = $1 AND deleted_at IS NULL",
                                )
                                .bind(cid)
                                .fetch_optional(&*state.db_pool)
                                .await
                                .unwrap_or_default()
                            }
                        } else {
                            None
                        }
                    }
                )
            };

            // OPTIMIZED: Only build verbose JSON if explicitly requested
            // Skip in minimal mode or when verbose is false to avoid unnecessary overhead
            let verbose_json = if minimal_mode || !verbose_requested {
                None // Skip verbose JSON building entirely if not needed
            } else {
                // Build JSON manually using serde_json::Value::Object to avoid macro overhead
                // OPTIMIZED: Pre-allocate Maps with estimated capacity
                use serde_json::Map;

                // OPTIMIZED: Pre-allocate routing_map with capacity (4 fields)
                let mut routing_map = Map::with_capacity(4);
                routing_map.insert(
                    "buyer_name".to_string(),
                    serde_json::Value::String(buyer_name.as_deref().unwrap_or("").to_string()),
                );
                if let Some(buyer_id) = routing_result.buyer_id {
                    // OPTIMIZED: Pre-allocate UUID string
                    let mut buyer_id_str = String::with_capacity(36);
                    buyer_id_str.push_str(&buyer_id.to_string());
                    routing_map.insert(
                        "buyer_id".to_string(),
                        serde_json::Value::String(buyer_id_str),
                    );
                } else {
                    routing_map.insert("buyer_id".to_string(), serde_json::Value::Null);
                }
                routing_map.insert(
                    "campaign_name".to_string(),
                    serde_json::Value::String(campaign_name.as_deref().unwrap_or("").to_string()),
                );
                if let Some(campaign_id) = routing_result.campaign_id {
                    // OPTIMIZED: Pre-allocate UUID string
                    let mut campaign_id_str = String::with_capacity(36);
                    campaign_id_str.push_str(&campaign_id.to_string());
                    routing_map.insert(
                        "campaign_id".to_string(),
                        serde_json::Value::String(campaign_id_str),
                    );
                } else {
                    routing_map.insert("campaign_id".to_string(), serde_json::Value::Null);
                }

                // OPTIMIZED: Pre-allocate json_obj_map with capacity (6 fields)
                let mut json_obj_map = Map::with_capacity(6);
                // OPTIMIZED: Use String::with_capacity instead of format!
                let mut error_code = String::with_capacity(7);
                error_code.push_str("ERR_200");
                json_obj_map.insert(
                    "error_code".to_string(),
                    serde_json::Value::String(error_code),
                );
                json_obj_map.insert(
                    "timestamp".to_string(),
                    serde_json::Value::String(Utc::now().to_rfc3339()),
                );
                json_obj_map.insert(
                    "endpoint".to_string(),
                    serde_json::Value::String("POST /api/v1/leads".to_string()),
                );
                json_obj_map.insert(
                    "status_code".to_string(),
                    serde_json::Value::Number(200.into()),
                );
                json_obj_map.insert(
                    "routing".to_string(),
                    serde_json::Value::Object(routing_map),
                );

                // Add per_buyer_timings if available (align status with main outcome; fullpost: no bid)
                if let Some(ref timings) = routing_result.per_buyer_timings {
                    let transformed = transform_per_buyer_timings_for_verbose(
                        timings,
                        &routing_result.status,
                        request_type == "fullpost",
                    );
                    json_obj_map.insert(
                        "per_buyer_timings".to_string(),
                        serde_json::Value::Array(transformed),
                    );
                }

                let json_obj = serde_json::Value::Object(json_obj_map);

                Some(json_obj)
            };

            // Save ping payloads for ping requests OR fullpost requests that split into ping/post
            // When fullpost splits, ping_id will be present even though request_type is "fullpost"
            let should_save_ping_payloads = payload_row_id.is_some()
                || (request_type == "fullpost" && routing_result.ping_id.is_some());

            if should_save_ping_payloads {
                // Build JSON manually using serde_json::Value::Object to avoid macro overhead
                // This is more efficient than serde_json::json! macro
                use serde_json::Map;
                let mut routing_result_map = Map::new();
                routing_result_map.insert(
                    "status".to_string(),
                    serde_json::Value::String(routing_result.status.clone()),
                );
                routing_result_map.insert(
                    "success".to_string(),
                    serde_json::Value::Bool(routing_result.success),
                );
                if let Some(ref error) = routing_result.error {
                    routing_result_map.insert(
                        "error".to_string(),
                        serde_json::Value::String(error.clone()),
                    );
                } else {
                    routing_result_map.insert("error".to_string(), serde_json::Value::Null);
                }
                if let Some(price) = routing_result.price {
                    routing_result_map.insert(
                        "price".to_string(),
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(price)
                                .unwrap_or_else(|| serde_json::Number::from(0)),
                        ),
                    );
                } else {
                    routing_result_map.insert("price".to_string(), serde_json::Value::Null);
                }
                if let Some(buyer_id) = routing_result.buyer_id {
                    routing_result_map.insert(
                        "buyer_id".to_string(),
                        serde_json::Value::String(buyer_id.to_string()),
                    );
                } else {
                    routing_result_map.insert("buyer_id".to_string(), serde_json::Value::Null);
                }
                if let Some(campaign_id) = routing_result.campaign_id {
                    routing_result_map.insert(
                        "campaign_id".to_string(),
                        serde_json::Value::String(campaign_id.to_string()),
                    );
                } else {
                    routing_result_map.insert("campaign_id".to_string(), serde_json::Value::Null);
                }
                if let Some(ref ping_id) = routing_result.ping_id {
                    routing_result_map.insert(
                        "ping_id".to_string(),
                        serde_json::Value::String(ping_id.clone()),
                    );
                } else {
                    routing_result_map.insert("ping_id".to_string(), serde_json::Value::Null);
                }
                if let Some(ref post_id) = routing_result.post_id {
                    routing_result_map.insert(
                        "post_id".to_string(),
                        serde_json::Value::String(post_id.clone()),
                    );
                } else {
                    routing_result_map.insert("post_id".to_string(), serde_json::Value::Null);
                }
                if let Some(ref promise_id) = routing_result.promise_id {
                    routing_result_map.insert(
                        "promise_id".to_string(),
                        serde_json::Value::String(promise_id.clone()),
                    );
                } else {
                    routing_result_map.insert("promise_id".to_string(), serde_json::Value::Null);
                }
                // OPTIMIZED: Pre-allocate Map with capacity
                let mut response_json_map = Map::with_capacity(1);
                response_json_map.insert(
                    "routing_result".to_string(),
                    serde_json::Value::Object(routing_result_map),
                );
                let response_json = serde_json::Value::Object(response_json_map);

                // Try to encrypt the response as well
                let mut encrypted_response_opt: Option<String> = None;
                if let Ok(Some(det_key)) = state.ssm.get_parameter(&det_path, true).await {
                    if let Ok(Some(salt)) = state.ssm.get_parameter(&salt_path, true).await {
                        let derived =
                            leadsnebula_core::encryption::EncryptionService::derive_key_from_secret(
                                &det_key, &salt,
                            );
                        if let Ok(bytes) = simd_json::to_vec(&response_json) {
                            if let Ok(resp_str) = String::from_utf8(bytes) {
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
                }

                // Update ping_payloads via write-behind queue (decoupled from critical path)
                // OPTIMIZED: Use reference instead of clone where possible
                state.write_behind_queue.enqueue(
                    leadsnebula_core::services::write_behind_queue::BackgroundTask::PayloadUpdate {
                        lead_id: lead_uuid,
                        payload_type: "ping".to_string(),
                        payload: response_json, // Move ownership (not used after this)
                        post_id: None,
                        request_payload_encrypted: None,
                        response_payload_encrypted: encrypted_response_opt, // Move ownership (not used after this)
                        ping_payloads_row_id: Some(lead_uuid),
                        external_ping_id: routing_result.ping_id.clone(), // Need to clone, used in response
                    },
                );

                // Encryption of buyer_responses moved to background job/cron - spawn overhead eliminated
                // If encryption is needed, it should be handled by a separate background task or write-behind queue
            }

            // For fullpost requests, also save post payloads if post_id is present
            if request_type == "fullpost" && routing_result.post_id.is_some() {
                // OPTIMIZED: Defer JSON serialization - only serialize when needed
                // Build JSON manually using serde_json::Value::Object to avoid macro overhead
                // OPTIMIZED: Pre-allocate Map with estimated capacity (8 fields)
                use serde_json::Map;
                let mut routing_result_map = Map::with_capacity(8);
                routing_result_map.insert(
                    "status".to_string(),
                    serde_json::Value::String(routing_result.status.clone()),
                );
                routing_result_map.insert(
                    "success".to_string(),
                    serde_json::Value::Bool(routing_result.success),
                );
                if let Some(ref error) = routing_result.error {
                    routing_result_map.insert(
                        "error".to_string(),
                        serde_json::Value::String(error.clone()),
                    );
                } else {
                    routing_result_map.insert("error".to_string(), serde_json::Value::Null);
                }
                if let Some(price) = routing_result.price {
                    routing_result_map.insert(
                        "price".to_string(),
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(price)
                                .unwrap_or_else(|| serde_json::Number::from(0)),
                        ),
                    );
                } else {
                    routing_result_map.insert("price".to_string(), serde_json::Value::Null);
                }
                if let Some(buyer_id) = routing_result.buyer_id {
                    routing_result_map.insert(
                        "buyer_id".to_string(),
                        serde_json::Value::String(buyer_id.to_string()),
                    );
                } else {
                    routing_result_map.insert("buyer_id".to_string(), serde_json::Value::Null);
                }
                if let Some(campaign_id) = routing_result.campaign_id {
                    routing_result_map.insert(
                        "campaign_id".to_string(),
                        serde_json::Value::String(campaign_id.to_string()),
                    );
                } else {
                    routing_result_map.insert("campaign_id".to_string(), serde_json::Value::Null);
                }
                if let Some(ref ping_id) = routing_result.ping_id {
                    routing_result_map.insert(
                        "ping_id".to_string(),
                        serde_json::Value::String(ping_id.clone()),
                    );
                } else {
                    routing_result_map.insert("ping_id".to_string(), serde_json::Value::Null);
                }
                if let Some(ref post_id) = routing_result.post_id {
                    routing_result_map.insert(
                        "post_id".to_string(),
                        serde_json::Value::String(post_id.clone()),
                    );
                } else {
                    routing_result_map.insert("post_id".to_string(), serde_json::Value::Null);
                }
                if let Some(ref promise_id) = routing_result.promise_id {
                    routing_result_map.insert(
                        "promise_id".to_string(),
                        serde_json::Value::String(promise_id.clone()),
                    );
                } else {
                    routing_result_map.insert("promise_id".to_string(), serde_json::Value::Null);
                }
                // OPTIMIZED: Pre-allocate Map with capacity
                let mut post_response_json_map = Map::with_capacity(1);
                post_response_json_map.insert(
                    "routing_result".to_string(),
                    serde_json::Value::Object(routing_result_map),
                );
                let post_response_json = serde_json::Value::Object(post_response_json_map);

                // OPTIMIZED: Serialize post_request_json only when needed (for encryption/queue)
                let post_request_json =
                    serde_json::to_value(&lead_data).unwrap_or_else(|_| serde_json::json!({}));

                // Try to encrypt using SSM deterministic key
                // OPTIMIZED: Use String::with_capacity instead of format!
                let env_norm_fp =
                    leadsnebula_core::normalize_env_for_ssm(&state.config.environment).to_string();
                let mut det_path_fp = String::with_capacity(env_norm_fp.len() + 60);
                det_path_fp.push_str("/leadsnebula/");
                det_path_fp.push_str(&env_norm_fp);
                det_path_fp.push_str("/carina/encryption/deterministic_key_v1");
                let mut salt_path_fp = String::with_capacity(env_norm_fp.len() + 60);
                salt_path_fp.push_str("/leadsnebula/");
                salt_path_fp.push_str(&env_norm_fp);
                salt_path_fp.push_str("/carina/encryption/key_derivation_salt_v1");
                let mut enc_req_opt_fp: Option<String> = None;
                let mut enc_resp_opt_fp: Option<String> = None;
                if let Ok(Some(det_key)) =
                    get_ssm_parameter_cached(&state.ssm, &det_path_fp, true).await
                {
                    if let Ok(Some(salt)) =
                        get_ssm_parameter_cached(&state.ssm, &salt_path_fp, true).await
                    {
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

                // Insert into post_payloads for fullpost via write-behind queue (decoupled from critical path)
                state.write_behind_queue.enqueue(
                    leadsnebula_core::services::write_behind_queue::BackgroundTask::PayloadUpdate {
                        lead_id: lead_uuid,
                        payload_type: "post".to_string(),
                        payload: post_request_json.clone(),
                        post_id: routing_result.post_id.clone(),
                        request_payload_encrypted: enc_req_opt_fp.clone(),
                        response_payload_encrypted: enc_resp_opt_fp.clone(),
                        ping_payloads_row_id: None,
                        external_ping_id: None,
                    },
                );
            }

            // Log diagnostic summary and timing before returning (non-blocking)
            timing_arc.flush_to_background(&lead_uuid.to_string());
            metrics_arc.log_summary("carina");

            // Single-line auction duration summary (informative only)
            let pre_checks_ms = timing_arc.get_pre_checks_ms();
            let ping_auction_ms = timing_arc.get_ping_auction_ms();
            let qualification_ms = timing_arc.get_qualification_ms();
            let post_sent_ms = timing_arc.get_post_sent_ms();
            let total_ms = timing_arc.get_total_ms();

            // Store auction timing in vertical_data for frontend reporting
            // Include per-buyer timings and all processing times for verbose/compliance
            let mut auction_timing_base = serde_json::Map::new();
            auction_timing_base.insert("request_type".to_string(), serde_json::json!(request_type));
            auction_timing_base.insert(
                "pre_checks_ms".to_string(),
                serde_json::json!(pre_checks_ms),
            );
            auction_timing_base.insert(
                "ping_auction_ms".to_string(),
                serde_json::json!(ping_auction_ms),
            );
            auction_timing_base.insert(
                "qualification_ms".to_string(),
                serde_json::json!(qualification_ms),
            );
            auction_timing_base.insert("post_ms".to_string(), serde_json::json!(post_sent_ms));
            auction_timing_base.insert("total_ms".to_string(), serde_json::json!(total_ms));

            // Add per-buyer timings if available (each buyer ping as separate datapoint)
            if let Some(ref per_buyer) = routing_result.per_buyer_timings {
                auction_timing_base.insert(
                    "per_buyer_timings".to_string(),
                    serde_json::json!(per_buyer),
                );
            }

            let outcome_str = if routing_result.success {
                "success"
            } else {
                "failure"
            };
            let verbose_obj = request_context_from_headers(
                &headers,
                "/api/v1/leads",
                "POST",
                "Lead submission (auction processing)",
                "carina_api",
                outcome_str,
                "Lead",
                &lead_uuid.to_string(),
                &request_type,
                chrono::Utc::now(),
            );
            let mut verbose_map = verbose_obj.as_object().cloned().unwrap_or_default();
            let db_operations_ms = metrics.get_total_query_time_ms();
            let mut processing_times = serde_json::Map::new();
            processing_times.insert(
                "pre_checks_ms".to_string(),
                serde_json::json!(pre_checks_ms),
            );
            processing_times.insert(
                "ping_auction_ms".to_string(),
                serde_json::json!(ping_auction_ms),
            );
            processing_times.insert(
                "qualification_ms".to_string(),
                serde_json::json!(qualification_ms),
            );
            processing_times.insert("post_ms".to_string(), serde_json::json!(post_sent_ms));
            processing_times.insert(
                "db_operations_ms".to_string(),
                serde_json::json!(db_operations_ms),
            );
            processing_times.insert("total_ms".to_string(), serde_json::json!(total_ms));
            if let Some(ref per_buyer) = routing_result.per_buyer_timings {
                processing_times.insert(
                    "per_buyer_timings".to_string(),
                    serde_json::json!(per_buyer),
                );
            }
            verbose_map.insert(
                "processing_times".to_string(),
                serde_json::Value::Object(processing_times),
            );

            auction_timing_base.insert(
                "db_operations_ms".to_string(),
                serde_json::json!(db_operations_ms),
            );

            let auction_timing_data = serde_json::json!({
                "auction_timing": serde_json::Value::Object(auction_timing_base),
                "verbose": serde_json::Value::Object(verbose_map)
            });

            // Determine final status with explicit sold detection
            // CRITICAL: Ensure sold leads are always marked as sold, even if status string doesn't match exactly
            let final_status = if routing_result.success
                && routing_result.status == "sold"
                && routing_result.price.is_some()
                && routing_result.price.unwrap_or(0.0) > 0.0
            {
                // Explicitly sold: status="sold", success=true, has price
                leadsnebula_core::models::enums::LeadStatus::Sold
            } else if routing_result.success
                && routing_result.post_id.is_some()
                && routing_result.price.is_some()
                && routing_result.price.unwrap_or(0.0) > 0.0
            {
                // Post succeeded with price - must be sold (even if status string doesn't say "sold")
                tracing::warn!(
                    lead_id = %lead_uuid,
                    routing_status = %routing_result.status,
                    has_post_id = true,
                    has_price = true,
                    "Detected sold lead (post succeeded with price) but status string was not 'sold' - correcting to Sold"
                );
                leadsnebula_core::models::enums::LeadStatus::Sold
            } else if routing_result.success
                && routing_result.campaign_id.is_some()
                && routing_result.buyer_id.is_some()
                && routing_result.price.is_some()
                && routing_result.price.unwrap_or(0.0) > 0.0
            {
                // Buyer selected with price - must be sold (fallback for cases where post_id might be None)
                tracing::warn!(
                    lead_id = %lead_uuid,
                    routing_status = %routing_result.status,
                    has_campaign_id = true,
                    has_buyer_id = true,
                    has_price = true,
                    "Detected sold lead (buyer selected with price) but status/post_id conditions not met - correcting to Sold"
                );
                leadsnebula_core::models::enums::LeadStatus::Sold
            } else if routing_result.success
                && (routing_result.status == "accepted" || routing_result.status == "ping_accepted")
            {
                // Ping succeeded - lead is ping_accepted (even if post failed)
                leadsnebula_core::models::enums::LeadStatus::PingAccepted
            } else if routing_result.campaign_id.is_some() && routing_result.buyer_id.is_some() {
                // Buyer was selected (ping succeeded) but status string doesn't indicate success
                // This handles cases where ping succeeded but post failed or status string is wrong
                tracing::warn!(
                    lead_id = %lead_uuid,
                    routing_status = %routing_result.status,
                    routing_success = routing_result.success,
                    has_campaign_id = true,
                    has_buyer_id = true,
                    "Ping succeeded (buyer selected) but status indicates failure - correcting to PingAccepted"
                );
                leadsnebula_core::models::enums::LeadStatus::PingAccepted
            } else {
                leadsnebula_core::models::enums::LeadStatus::Processing
            };

            // Log status determination for debugging
            tracing::warn!(
                lead_id = %lead_uuid,
                routing_status = %routing_result.status,
                routing_success = routing_result.success,
                has_post_id = routing_result.post_id.is_some(),
                has_price = routing_result.price.is_some(),
                price_value = ?routing_result.price,
                final_status = ?final_status,
                "Determining final lead status"
            );

            // Store auction timing in database via write-behind queue (non-blocking)
            let is_sold = matches!(
                final_status,
                leadsnebula_core::models::enums::LeadStatus::Sold
            );

            // For sold leads, update status synchronously as a safety net (with timeout to avoid blocking)
            // This ensures sold leads are immediately visible in the UI
            if is_sold {
                // Clone all values needed for the async block to avoid lifetime issues
                let lead_uuid_clone = lead_uuid;
                let final_status_clone = final_status.clone();
                let campaign_id_clone = routing_result.campaign_id;
                let buyer_id_clone = routing_result.buyer_id;
                let promise_id_clone = routing_result.promise_id.clone();
                let ping_id_clone = routing_result.ping_id.clone();
                let post_id_clone = routing_result.post_id.clone();
                let auction_timing_data_clone = auction_timing_data.clone();
                let db_pool_clone = state.db_pool.clone();

                // Fire and forget with 2000ms timeout to avoid blocking auction
                // Increased from 500ms to handle Neon cold starts and slow connections during first request
                tokio::spawn(async move {
                    let update_start = std::time::Instant::now();
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(2000),
                        // CRITICAL: buyer_id, campaign_id, and post_id are NOT NULL in schema
                        // Use COALESCE to handle None values - keep existing values if None is provided
                        sqlx::query(
                            r#"
                            UPDATE leads
                            SET status = $2, 
                                campaign_id = COALESCE($3, campaign_id), 
                                buyer_id = COALESCE($4, buyer_id), 
                                promise_id = COALESCE($5, promise_id), 
                                ping_id = COALESCE($6, ping_id), 
                                post_id = COALESCE($7, post_id), 
                                sold_at = NOW(), 
                                vertical_data = COALESCE($8, vertical_data), 
                                updated_at = NOW()
                            WHERE uuid = $1
                            "#,
                        )
                        .bind(lead_uuid_clone)
                        .bind(&final_status_clone)
                        .bind(campaign_id_clone)
                        .bind(buyer_id_clone)
                        .bind(promise_id_clone.as_ref())
                        .bind(ping_id_clone.as_ref())
                        .bind(post_id_clone.as_deref().unwrap_or(""))
                        .bind(sqlx::types::Json(&auction_timing_data_clone))
                        .execute(&*db_pool_clone),
                    )
                    .await
                    {
                        Ok(Ok(result)) => {
                            let update_duration = update_start.elapsed();
                            if result.rows_affected() == 0 {
                                tracing::warn!(
                                    "Synchronous update for sold lead {} completed but affected 0 rows (lead may not exist or already updated)",
                                    lead_uuid_clone
                                );
                            } else {
                                tracing::info!(
                                    "Successfully updated sold lead {} status synchronously in {}ms",
                                    lead_uuid_clone,
                                    update_duration.as_millis()
                                );
                            }
                        }
                        Ok(Err(db_err)) => {
                            tracing::error!(
                                "Failed to synchronously update sold lead {} status: {}",
                                lead_uuid_clone,
                                db_err
                            );
                        }
                        Err(_timeout) => {
                            // Timeout - non-critical, write-behind queue will handle it
                            tracing::warn!(
                                "Sold lead {} status update timed out after 2000ms (non-critical, write-behind queue will handle)",
                                lead_uuid_clone
                            );
                        }
                    }
                });
            }

            // Also enqueue to write-behind queue for eventual consistency (handles retries, etc.)
            state.write_behind_queue.enqueue(
                leadsnebula_core::services::write_behind_queue::BackgroundTask::LeadUpdate {
                    lead_id: lead_uuid,
                    status: final_status,
                    campaign_id: routing_result.campaign_id,
                    buyer_id: routing_result.buyer_id,
                    promise_id: routing_result.promise_id.clone(),
                    ping_id: routing_result.ping_id.clone(),
                    post_id: routing_result.post_id.clone(),
                    sold_at: is_sold,
                    inprog_token: None,
                    vertical_data: Some(auction_timing_data.clone()),
                },
            );

            // Log auction process summary (async, non-blocking - 0ms impact)
            // Must have: Complete auction process visibility for troubleshooting
            let auction_summary = serde_json::json!({
                "lead_id": lead_uuid,
                "request_type": request_type,
                "ping_auction_ms": ping_auction_ms,
                "post_ms": post_sent_ms,
                "total_ms": total_ms,
                "winner_campaign_id": routing_result.campaign_id,
                "winner_buyer_id": routing_result.buyer_id,
                "status": routing_result.status,
                "success": routing_result.success,
            });
            tokio::spawn(async move {
                tracing::info!(
                    target: "auction_process",
                    auction_summary = %serde_json::to_string(&auction_summary).unwrap_or_default(),
                    "Auction process completed"
                );
            });

            // Single-line auction duration summary (WARN level to ensure visibility in production)
            if request_type == "ping" {
                tracing::warn!(
                    lead_id = %lead_uuid,
                    request_type = "ping",
                    ping_auction_ms = ping_auction_ms,
                    total_ms = total_ms,
                    "Auction durations"
                );
            } else if request_type == "post" || request_type == "fullpost" {
                tracing::warn!(
                    lead_id = %lead_uuid,
                    request_type = %request_type,
                    ping_auction_ms = ping_auction_ms,
                    post_ms = post_sent_ms,
                    total_ms = total_ms,
                    "Auction durations"
                );
            }

            // Critical path timing: mark post_response_parsed (response ready to return)
            // Log slow requests as WARN (for monitoring), fast requests as DEBUG (reduced overhead)
            let critical_path_elapsed = critical_path_start.elapsed();
            let critical_path_ms = critical_path_elapsed.as_millis() as u64;

            // SENTRY ALERT: Slow critical path
            #[cfg(feature = "sentry")]
            if critical_path_ms > 1000 {
                sentry::capture_message(
                    &format!(
                        "Slow critical path: {}ms for lead {}",
                        critical_path_ms, lead_uuid
                    ),
                    sentry::Level::Warning,
                );
            }

            // Only log if > 500ms (slow requests) or in debug mode
            if critical_path_ms > 500 {
                tracing::warn!(
                    lead_id = %lead_uuid,
                    critical_path_ms = critical_path_ms,
                    "Non-DB critical path timing (slow)"
                );
            } else {
                tracing::debug!(
                    lead_id = %lead_uuid,
                    critical_path_ms = critical_path_ms,
                    "Non-DB critical path timing"
                );
            }

            // BLAME-SHIFTING: Calculate timing breakdown for async logging (zero overhead)
            let total_wall_ms = request_start.elapsed().as_millis() as u64;
            let internal_ms = critical_path_ms;
            let buyer_post_ms = post_sent_ms;
            let external_ms = total_wall_ms.saturating_sub(internal_ms);

            // Get DB query time from metrics for Sentry alert
            let _db_query_time_ms = metrics.get_total_query_time_ms();

            // SENTRY ALERT: Slow DB queries
            #[cfg(feature = "sentry")]
            if _db_query_time_ms > 500 {
                sentry::capture_message(
                    &format!(
                        "Slow DB query: {}ms for lead {}",
                        _db_query_time_ms, lead_uuid
                    ),
                    sentry::Level::Warning,
                );
            }

            // Build response first (don't wait for logging)
            // promise_id is only for ping/post; fullpost does not expose it (single-step flow, no post follow-up)
            let response = LeadResponse {
                status: StatusNode {
                    success: routing_result.success,
                    status: routing_result.status.clone(),
                    message,
                    error: routing_result.error.clone(),
                },
                lead: LeadNode {
                    promise_id: if request_type == "fullpost" {
                        None
                    } else {
                        routing_result.promise_id.clone()
                    },
                    lead_id: Some(lead_id),
                    lead_uuid: Some(lead_uuid.to_string()),
                    ping_id: None,
                    bid: if request_type == "fullpost" {
                        None
                    } else {
                        bid
                    },
                    post_id: routing_result.post_id.clone(),
                    price,
                },
                verbose: verbose_json,
                http_status: Some(200),
            };

            // BLAME-SHIFTING: Log summary asynchronously (zero impact on response time)
            let lead_uuid_for_log = lead_uuid;
            tokio::spawn(async move {
                tracing::warn!(
                    lead_id = %lead_uuid_for_log,
                    internal_ms,
                    buyer_post_ms,
                    external_ms,
                    total_wall_ms,
                    "SUMMARY: LeadsNebula internal: {}ms | Buyer post: {}ms | External overhead: {}ms | Total wall-clock: {}ms",
                    internal_ms, buyer_post_ms, external_ms, total_wall_ms
                );
            });

            Ok((StatusCode::OK, Json(response)))
        }
        Err(e) => {
            tracing::error!("Routing error: {}", e);
            timing_arc.flush_to_background(&lead_uuid.to_string());
            metrics_arc.log_summary("carina");
            let (message, technical) = map_error_to_user(&e.to_string());
            Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LeadResponse {
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
                }),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        missing_required_lead_field, transform_per_buyer_timings_for_verbose, LeadData, LeadNode,
        LeadResponse, StatusNode,
    };
    use serde_json::json;

    fn full_ping_lead() -> LeadData {
        LeadData {
            publisher_id: Some("a1b2c3d4-e5f6-4780-a123-456789abcdef".into()),
            vertical: "solar".into(),
            request_type: Some("ping".into()),
            campaign_token: None,
            promise_id: None,
            lead_id: None,
            source: None,
            first_name: None,
            last_name: None,
            email: None,
            cell_phone: None,
            street_address: None,
            city: None,
            state: None,
            zip: Some("90210".into()),
            monthly_bill: Some(100.0),
            credit_rating: Some("good".into()),
            own_home: Some(true),
            property_type: None,
            roof_shade: Some("partial".into()),
            roof_type: None,
            utility_provider: None,
            purchase_timeframe: None,
            ip_address: Some("192.168.1.1".into()),
            tcpa_consent: Some(true),
            tcpa_language: Some("I agree".into()),
            jornaya_lead_id: Some("j".into()),
            trusted_form_url: Some("https://x".into()),
            is_test: None,
            verbose: None,
            user_agent: None,
        }
    }

    fn full_post_lead() -> LeadData {
        LeadData {
            publisher_id: Some("a1b2c3d4-e5f6-4780-a123-456789abcdef".into()),
            vertical: "solar".into(),
            request_type: Some("post".into()),
            campaign_token: None,
            promise_id: Some("prom_123".into()),
            lead_id: None,
            source: None,
            first_name: Some("John".into()),
            last_name: Some("Doe".into()),
            email: Some("j@example.com".into()),
            cell_phone: Some("5551234567".into()),
            street_address: Some("123 Main St".into()),
            city: Some("Anytown".into()),
            state: Some("CA".into()),
            zip: Some("90210".into()),
            monthly_bill: Some(100.0),
            credit_rating: Some("good".into()),
            own_home: Some(true),
            property_type: Some("single_family".into()),
            roof_shade: Some("partial".into()),
            roof_type: None,
            utility_provider: Some("Acme".into()),
            purchase_timeframe: None,
            ip_address: Some("192.168.1.1".into()),
            tcpa_consent: Some(true),
            tcpa_language: Some("I agree".into()),
            jornaya_lead_id: Some("j".into()),
            trusted_form_url: Some("https://x".into()),
            is_test: None,
            verbose: None,
            user_agent: None,
        }
    }

    fn full_fullpost_lead() -> LeadData {
        LeadData {
            publisher_id: Some("a1b2c3d4-e5f6-4780-a123-456789abcdef".into()),
            vertical: "solar".into(),
            request_type: Some("fullpost".into()),
            campaign_token: None,
            promise_id: None,
            lead_id: None,
            source: None,
            first_name: Some("John".into()),
            last_name: Some("Doe".into()),
            email: Some("j@example.com".into()),
            cell_phone: Some("5551234567".into()),
            street_address: Some("123 Main St".into()),
            city: Some("Anytown".into()),
            state: Some("CA".into()),
            zip: Some("90210".into()),
            monthly_bill: Some(100.0),
            credit_rating: Some("good".into()),
            own_home: Some(true),
            property_type: None,
            roof_shade: Some("partial".into()),
            roof_type: None,
            utility_provider: Some("Acme".into()),
            purchase_timeframe: None,
            ip_address: Some("192.168.1.1".into()),
            tcpa_consent: Some(true),
            tcpa_language: Some("I agree".into()),
            jornaya_lead_id: Some("j".into()),
            trusted_form_url: Some("https://x".into()),
            is_test: None,
            verbose: None,
            user_agent: None,
        }
    }

    #[test]
    fn ping_all_required_present_returns_none() {
        assert!(missing_required_lead_field(&full_ping_lead(), "ping").is_none());
    }

    #[test]
    fn ping_missing_publisher_id() {
        let mut lead = full_ping_lead();
        lead.publisher_id = None;
        assert_eq!(
            missing_required_lead_field(&lead, "ping"),
            Some("publisher_id")
        );
    }

    #[test]
    fn ping_missing_roof_shade() {
        let mut lead = full_ping_lead();
        lead.roof_shade = None;
        assert_eq!(
            missing_required_lead_field(&lead, "ping"),
            Some("roof_shade")
        );
    }

    #[test]
    fn ping_missing_zip() {
        let mut lead = full_ping_lead();
        lead.zip = None;
        assert_eq!(missing_required_lead_field(&lead, "ping"), Some("zip"));
    }

    #[test]
    fn ping_missing_credit_rating() {
        let mut lead = full_ping_lead();
        lead.credit_rating = None;
        assert_eq!(
            missing_required_lead_field(&lead, "ping"),
            Some("credit_rating")
        );
    }

    #[test]
    fn ping_campaign_token_optional() {
        let mut lead = full_ping_lead();
        lead.campaign_token = None;
        assert!(missing_required_lead_field(&lead, "ping").is_none());
    }

    #[test]
    fn ping_purchase_timeframe_optional() {
        let mut lead = full_ping_lead();
        lead.purchase_timeframe = None;
        assert!(missing_required_lead_field(&lead, "ping").is_none());
    }

    #[test]
    fn post_all_required_present_returns_none() {
        assert!(missing_required_lead_field(&full_post_lead(), "post").is_none());
    }

    #[test]
    fn post_missing_promise_id() {
        let mut lead = full_post_lead();
        lead.promise_id = None;
        assert_eq!(
            missing_required_lead_field(&lead, "post"),
            Some("promise_id")
        );
    }

    #[test]
    fn post_missing_roof_shade() {
        let mut lead = full_post_lead();
        lead.roof_shade = None;
        assert_eq!(
            missing_required_lead_field(&lead, "post"),
            Some("roof_shade")
        );
    }

    #[test]
    fn post_missing_cell_phone() {
        let mut lead = full_post_lead();
        lead.cell_phone = None;
        assert_eq!(
            missing_required_lead_field(&lead, "post"),
            Some("cell_phone")
        );
    }

    #[test]
    fn post_campaign_token_and_purchase_timeframe_optional() {
        let mut lead = full_post_lead();
        lead.campaign_token = None;
        lead.purchase_timeframe = None;
        assert!(missing_required_lead_field(&lead, "post").is_none());
    }

    #[test]
    fn fullpost_all_required_present_returns_none() {
        assert!(missing_required_lead_field(&full_fullpost_lead(), "fullpost").is_none());
    }

    #[test]
    fn fullpost_missing_roof_shade() {
        let mut lead = full_fullpost_lead();
        lead.roof_shade = None;
        assert_eq!(
            missing_required_lead_field(&lead, "fullpost"),
            Some("roof_shade")
        );
    }

    #[test]
    fn fullpost_missing_ip_address() {
        let mut lead = full_fullpost_lead();
        lead.ip_address = None;
        assert_eq!(
            missing_required_lead_field(&lead, "fullpost"),
            Some("ip_address")
        );
    }

    #[test]
    fn fullpost_missing_cell_phone() {
        let mut lead = full_fullpost_lead();
        lead.cell_phone = None;
        assert_eq!(
            missing_required_lead_field(&lead, "fullpost"),
            Some("cell_phone")
        );
    }

    #[test]
    fn fullpost_purchase_timeframe_optional() {
        let mut lead = full_fullpost_lead();
        lead.purchase_timeframe = None;
        assert!(missing_required_lead_field(&lead, "fullpost").is_none());
    }

    #[test]
    fn fullpost_campaign_token_optional() {
        let mut lead = full_fullpost_lead();
        lead.campaign_token = None;
        assert!(missing_required_lead_field(&lead, "fullpost").is_none());
    }

    // --- Post: additional required fields (prevent regressions) ---
    #[test]
    fn post_missing_first_name() {
        let mut lead = full_post_lead();
        lead.first_name = None;
        assert_eq!(
            missing_required_lead_field(&lead, "post"),
            Some("first_name")
        );
    }

    #[test]
    fn post_missing_email() {
        let mut lead = full_post_lead();
        lead.email = None;
        assert_eq!(missing_required_lead_field(&lead, "post"), Some("email"));
    }

    #[test]
    fn post_missing_ip_address() {
        let mut lead = full_post_lead();
        lead.ip_address = None;
        assert_eq!(
            missing_required_lead_field(&lead, "post"),
            Some("ip_address")
        );
    }

    #[test]
    fn post_missing_utility_provider() {
        let mut lead = full_post_lead();
        lead.utility_provider = None;
        assert_eq!(
            missing_required_lead_field(&lead, "post"),
            Some("utility_provider")
        );
    }

    // --- Ping: additional required fields ---
    #[test]
    fn ping_missing_ip_address() {
        let mut lead = full_ping_lead();
        lead.ip_address = None;
        assert_eq!(
            missing_required_lead_field(&lead, "ping"),
            Some("ip_address")
        );
    }

    #[test]
    fn ping_missing_monthly_bill() {
        let mut lead = full_ping_lead();
        lead.monthly_bill = None;
        assert_eq!(
            missing_required_lead_field(&lead, "ping"),
            Some("monthly_bill")
        );
    }

    #[test]
    fn ping_missing_tcpa_consent() {
        let mut lead = full_ping_lead();
        lead.tcpa_consent = None;
        assert_eq!(
            missing_required_lead_field(&lead, "ping"),
            Some("tcpa_consent")
        );
    }

    // --- Fullpost: additional required fields ---
    #[test]
    fn fullpost_missing_first_name() {
        let mut lead = full_fullpost_lead();
        lead.first_name = None;
        assert_eq!(
            missing_required_lead_field(&lead, "fullpost"),
            Some("first_name")
        );
    }

    #[test]
    fn fullpost_missing_email() {
        let mut lead = full_fullpost_lead();
        lead.email = None;
        assert_eq!(
            missing_required_lead_field(&lead, "fullpost"),
            Some("email")
        );
    }

    #[test]
    fn fullpost_missing_zip() {
        let mut lead = full_fullpost_lead();
        lead.zip = None;
        assert_eq!(missing_required_lead_field(&lead, "fullpost"), Some("zip"));
    }

    #[test]
    fn fullpost_missing_utility_provider() {
        let mut lead = full_fullpost_lead();
        lead.utility_provider = None;
        assert_eq!(
            missing_required_lead_field(&lead, "fullpost"),
            Some("utility_provider")
        );
    }

    // --- Per-buyer timings transformation (verbose consistency + fullpost no bid) ---
    #[test]
    fn transform_per_buyer_timings_aligns_status_to_main_sold() {
        let timings = vec![json!({
            "bid": 121,
            "buyer_id": "8f5ac648-7e16-41e1-8343-4f2f03e4669a",
            "campaign_id": "e2d5de3b-1cb1-435c-91c9-f33acf775ba8",
            "processing_time_ms": 0,
            "status": "accepted",
            "success": true
        })];
        let out = transform_per_buyer_timings_for_verbose(&timings, "sold", false);
        assert_eq!(out.len(), 1);
        let obj = out[0].as_object().expect("entry is object");
        assert_eq!(obj.get("status").and_then(|v| v.as_str()), Some("sold"));
        assert!(obj.get("bid").is_some(), "ping/post keeps bid in verbose");
    }

    #[test]
    fn transform_per_buyer_timings_fullpost_strips_bid() {
        let timings = vec![json!({
            "bid": 121,
            "buyer_id": "8f5ac648-7e16-41e1-8343-4f2f03e4669a",
            "campaign_id": "e2d5de3b-1cb1-435c-91c9-f33acf775ba8",
            "processing_time_ms": 0,
            "status": "accepted",
            "success": true
        })];
        let out = transform_per_buyer_timings_for_verbose(&timings, "sold", true);
        assert_eq!(out.len(), 1);
        let obj = out[0].as_object().expect("entry is object");
        assert_eq!(obj.get("status").and_then(|v| v.as_str()), Some("sold"));
        assert!(
            obj.get("bid").is_none(),
            "fullpost must not expose bid in per_buyer_timings"
        );
    }

    // --- LeadNode serialization: fullpost has no bid, ping has bid ---
    #[test]
    fn lead_node_fullpost_serializes_without_bid() {
        let node = LeadNode {
            promise_id: None,
            lead_id: Some("lead_john_doe_001".into()),
            lead_uuid: Some("d946682e-a5d8-5da2-b333-02bbad78a088".into()),
            ping_id: None,
            bid: None,
            post_id: Some("RP_xxx".into()),
            price: Some(167.0),
        };
        let json = serde_json::to_value(&node).expect("serialize");
        let obj = json.as_object().expect("lead is object");
        assert!(
            !obj.contains_key("bid"),
            "fullpost response lead must not contain bid"
        );
        assert_eq!(obj.get("price").and_then(|v| v.as_f64()), Some(167.0));
    }

    #[test]
    fn lead_node_ping_serializes_with_bid() {
        let node = LeadNode {
            promise_id: Some("prom_123".into()),
            lead_id: Some("lead_001".into()),
            lead_uuid: Some("uuid-here".into()),
            ping_id: Some("ping_1".into()),
            bid: Some(150.0),
            post_id: None,
            price: None,
        };
        let json = serde_json::to_value(&node).expect("serialize");
        let obj = json.as_object().expect("lead is object");
        assert_eq!(
            obj.get("bid").and_then(|v| v.as_f64()),
            Some(150.0),
            "ping response lead must include bid"
        );
    }

    #[test]
    fn lead_response_fullpost_contract_no_bid_in_lead() {
        let response = LeadResponse {
            status: StatusNode {
                success: true,
                status: "sold".to_string(),
                message: Some("Lead Sold for $167".to_string()),
                error: None,
            },
            lead: LeadNode {
                promise_id: None,
                lead_id: Some("lead_john_doe_001".into()),
                lead_uuid: Some("uuid".into()),
                ping_id: None,
                bid: None,
                post_id: Some("RP_xxx".into()),
                price: Some(167.0),
            },
            verbose: None,
            http_status: Some(200),
        };
        let json = serde_json::to_value(&response).expect("serialize");
        let lead = json
            .get("lead")
            .and_then(|v| v.as_object())
            .expect("lead object");
        assert!(
            !lead.contains_key("bid"),
            "fullpost success response lead must never contain bid"
        );
        assert!(lead.contains_key("price"));
    }
}
