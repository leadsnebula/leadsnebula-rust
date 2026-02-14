//! OpenAPI spec for the lead submission API, served at `/documentation` via Scalar.

use axum::extract::State;
use axum::response::{Html, IntoResponse, Json};
use serde_json::json;
use utoipa::openapi::path::ParameterIn;
use utoipa::openapi::schema::{ObjectBuilder, Schema, Type};
use utoipa::openapi::RefOr;
use utoipa::Modify;
use utoipa::OpenApi;

use crate::AppState;

const TEST_PUBLISHER_ID: &str = "a1b2c3d4-e5f6-4780-a123-456789abcdef";
const TEST_API_KEY: &str =
    "pk_test_3abfd248dbed82ed500426a5cac2ead3cf182ace20934ed8ad7dd5592b7b7d08";

/// OpenAPI specification for the lead submission API.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Leads API",
        version = "0.1.0",
        description = "Lead submission and routing.\n\n\
            **Vertical** must be passed on every request; it determines which fields are required.\n\n\
            **ip_address** is required on all request types (ping, post, fullpost).\n\n\
            **cell_phone** is required on post and fullpost; it is not required on ping.\n\n\
            **Verbose**: set `verbose` to `true` to include a `verbose` object in the response (endpoint, timestamp, request_id, etc.).\n\n\
            **Test credentials**: Publisher ID `a1b2c3d4-e5f6-4780-a123-456789abcdef`, API key `pk_test_3abfd248dbed82ed500426a5cac2ead3cf182ace20934ed8ad7dd5592b7b7d08` (after running migration `20260211000001_documentation_test_instance_and_publisher.sql`). Use the API key in the `X-API-Key` header."
    ),
    tags(
        (name = "Solar", description = "Solar vertical operations. Expand to view `Ping`, `Post`, and `Fullpost`. Additional verticals (e.g. HVAC) can follow the same structure.")
    ),
    paths(
        post_leads_ping,
        post_leads_post,
        post_leads_fullpost
    ),
    components(
        schemas(
            DocLeadPingRequest,
            DocLeadPingData,
            DocLeadPostRequest,
            DocLeadPostData,
            DocLeadFullpostRequest,
            DocLeadFullpostData,
            DocLeadResponse,
            DocStatusNode,
            DocLeadNode
        )
    ),
    modifiers(&XApiKeyDefaultModifier, &InlineLeadSchemaModifier)
)]
pub struct ApiDoc;

/// Modifier that sets `schema.default` for the X-API-Key header so Scalar's try-it modal pre-fills the value.
struct XApiKeyDefaultModifier;

impl Modify for XApiKeyDefaultModifier {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let default_value = json!(TEST_API_KEY);
        for (_path, path_item) in openapi.paths.paths.iter_mut() {
            for op in path_item
                .get
                .iter_mut()
                .chain(path_item.post.iter_mut())
                .chain(path_item.put.iter_mut())
                .chain(path_item.delete.iter_mut())
                .chain(path_item.patch.iter_mut())
                .chain(path_item.head.iter_mut())
                .chain(path_item.options.iter_mut())
                .chain(path_item.trace.iter_mut())
            {
                if let Some(params) = op.parameters.as_mut() {
                    for param in params.iter_mut() {
                        if param.name == "X-API-Key" && param.parameter_in == ParameterIn::Header {
                            match param.schema.as_mut() {
                                Some(RefOr::T(Schema::Object(obj))) => {
                                    obj.default = Some(default_value.clone());
                                }
                                _ => {
                                    param.schema = Some(RefOr::T(Schema::Object(
                                        ObjectBuilder::new()
                                            .schema_type(Type::String)
                                            .default(Some(default_value.clone()))
                                            .build(),
                                    )));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Inlines the lead schema into each request body so UIs show required badges on expanded child attributes.
struct InlineLeadSchemaModifier;

impl Modify for InlineLeadSchemaModifier {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = match openapi.components.as_mut() {
            Some(c) => c,
            None => return,
        };
        let schemas = &mut components.schemas;

        // Inline only Ping and Post so required badges show. Fullpost stays as $ref so Scalar
        // uses the path-level example for Try it (avoiding body corruption with inlined schema).
        let pairs: [(&str, &str); 2] = [
            ("DocLeadPingRequest", "DocLeadPingData"),
            ("DocLeadPostRequest", "DocLeadPostData"),
        ];

        for (request_name, data_name) in pairs {
            let lead_schema = match schemas.get(data_name) {
                Some(RefOr::T(Schema::Object(obj))) => Schema::Object(obj.clone()),
                _ => continue,
            };
            if let Some(RefOr::T(Schema::Object(req_obj))) = schemas.get_mut(request_name) {
                req_obj
                    .properties
                    .insert("lead".to_string(), RefOr::T(lead_schema));
            }
        }
    }
}

/// Request body for POST /api/v1/leads/ping
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadPingRequest {
    /// When true, response includes a `verbose` object with compliance/debugging details (endpoint, timestamp, request_id, etc.). Use for troubleshooting or compliance.
    #[schema(example = json!(false))]
    pub verbose: Option<bool>,
    pub lead: DocLeadPingData,
}

/// Solar Ping payload.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadPingData {
    #[schema(example = "solar", required)]
    pub vertical: String,
    #[schema(example = "a1b2c3d4-e5f6-4780-a123-456789abcdef", required)]
    pub publisher_id: String,
    #[schema(example = "ping", required)]
    pub request_type: String,
    #[schema(example = "90210", required)]
    pub zip: String,
    #[schema(example = "192.168.1.1", required)]
    pub ip_address: String,
    #[schema(example = json!(150.0), required)]
    pub monthly_bill: f64,
    #[schema(example = json!(true), required)]
    pub own_home: bool,
    #[schema(example = "partial", required)]
    pub roof_shade: String,
    #[schema(example = "good", required)]
    pub credit_rating: String,
    #[schema(example = json!(true), required)]
    pub tcpa_consent: bool,
    #[schema(example = "I agree to be contacted.", required)]
    pub tcpa_language: String,
    #[schema(example = "jornaya_lead_123", required)]
    pub jornaya_lead_id: String,
    #[schema(example = "https://cert.trustedform.com/example", required)]
    pub trusted_form_url: String,
    #[schema(example = "camp_john_doe_solar")]
    pub campaign_token: Option<String>,
    #[schema(example = "lead_john_doe_001")]
    pub lead_id: Option<String>,
    #[schema(example = "John")]
    pub first_name: Option<String>,
    #[schema(example = "Doe")]
    pub last_name: Option<String>,
    #[schema(example = "john.doe@example.com")]
    pub email: Option<String>,
    #[schema(example = "5551234567")]
    pub cell_phone: Option<String>,
    #[schema(example = "123 Main St")]
    pub street_address: Option<String>,
    #[schema(example = "Anytown")]
    pub city: Option<String>,
    #[schema(example = "CA")]
    pub state: Option<String>,
    #[schema(example = "single_family")]
    pub property_type: Option<String>,
    #[schema(example = "composition")]
    pub roof_type: Option<String>,
    #[schema(example = "Acme Electric")]
    pub utility_provider: Option<String>,
    #[schema(example = "1-3 months")]
    pub purchase_timeframe: Option<String>,
    #[schema(example = json!(false))]
    pub is_test: Option<bool>,
    #[schema(example = json!(false))]
    pub verbose: Option<bool>,
}

/// Request body for POST /api/v1/leads/post
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadPostRequest {
    #[schema(example = json!(false))]
    pub verbose: Option<bool>,
    pub lead: DocLeadPostData,
}

/// Solar Post payload.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadPostData {
    #[schema(example = "solar", required)]
    pub vertical: String,
    #[schema(example = "prom_john_doe_abc123", required)]
    pub promise_id: String,
    #[schema(example = "a1b2c3d4-e5f6-4780-a123-456789abcdef", required)]
    pub publisher_id: String,
    #[schema(example = "post", required)]
    pub request_type: String,
    #[schema(example = "John", required)]
    pub first_name: String,
    #[schema(example = "Doe", required)]
    pub last_name: String,
    #[schema(example = "john.doe@example.com", required)]
    pub email: String,
    #[schema(example = "5551234567", required)]
    pub cell_phone: String,
    #[schema(example = "123 Main St", required)]
    pub street_address: String,
    #[schema(example = "Anytown", required)]
    pub city: String,
    #[schema(example = "CA", required)]
    pub state: String,
    #[schema(example = "90210", required)]
    pub zip: String,
    #[schema(example = "192.168.1.1", required)]
    pub ip_address: String,
    #[schema(example = json!(150.0), required)]
    pub monthly_bill: f64,
    #[schema(example = json!(true), required)]
    pub own_home: bool,
    #[schema(example = "partial", required)]
    pub roof_shade: String,
    #[schema(example = "Acme Electric", required)]
    pub utility_provider: String,
    #[schema(example = "single_family", required)]
    pub property_type: String,
    #[schema(example = json!(true), required)]
    pub tcpa_consent: bool,
    #[schema(example = "I agree to be contacted.", required)]
    pub tcpa_language: String,
    #[schema(example = "good", required)]
    pub credit_rating: String,
    #[schema(example = "jornaya_lead_123", required)]
    pub jornaya_lead_id: String,
    #[schema(example = "https://cert.trustedform.com/example", required)]
    pub trusted_form_url: String,
    #[schema(example = "camp_john_doe_solar")]
    pub campaign_token: Option<String>,
    #[schema(example = "lead_john_doe_001")]
    pub lead_id: Option<String>,
    #[schema(example = "composition")]
    pub roof_type: Option<String>,
    #[schema(example = "1-3 months")]
    pub purchase_timeframe: Option<String>,
    #[schema(example = json!(false))]
    pub is_test: Option<bool>,
    #[schema(example = json!(false))]
    pub verbose: Option<bool>,
}

/// Request body for POST /api/v1/leads/fullpost
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadFullpostRequest {
    #[schema(example = json!(false))]
    pub verbose: Option<bool>,
    pub lead: DocLeadFullpostData,
}

/// Solar Fullpost payload.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadFullpostData {
    #[schema(example = "solar", required)]
    pub vertical: String,
    #[schema(example = "fullpost", required)]
    pub request_type: String,
    #[schema(example = "a1b2c3d4-e5f6-4780-a123-456789abcdef", required)]
    pub publisher_id: String,
    #[schema(example = "John", required)]
    pub first_name: String,
    #[schema(example = "Doe", required)]
    pub last_name: String,
    #[schema(example = "john.doe@example.com", required)]
    pub email: String,
    #[schema(example = "5551234567", required)]
    pub cell_phone: String,
    #[schema(example = "123 Main St", required)]
    pub street_address: String,
    #[schema(example = "Anytown", required)]
    pub city: String,
    #[schema(example = "CA", required)]
    pub state: String,
    #[schema(example = "90210", required)]
    pub zip: String,
    #[schema(example = json!(150.0), required)]
    pub monthly_bill: f64,
    #[schema(example = json!(true), required)]
    pub own_home: bool,
    #[schema(example = "partial", required)]
    pub roof_shade: String,
    #[schema(example = "Acme Electric", required)]
    pub utility_provider: String,
    #[schema(example = "good", required)]
    pub credit_rating: String,
    #[schema(example = json!(true), required)]
    pub tcpa_consent: bool,
    #[schema(example = "I agree to be contacted.", required)]
    pub tcpa_language: String,
    #[schema(example = "jornaya_lead_123", required)]
    pub jornaya_lead_id: String,
    #[schema(example = "https://cert.trustedform.com/example", required)]
    pub trusted_form_url: String,
    /// Required on all request types (ping, post, fullpost).
    #[schema(example = "192.168.1.1", required)]
    pub ip_address: String,
    #[schema(example = "lead_john_doe_001")]
    pub lead_id: Option<String>,
    #[schema(example = "camp_john_doe_solar")]
    pub campaign_token: Option<String>,
    #[schema(example = "single_family")]
    pub property_type: Option<String>,
    #[schema(example = "composition")]
    pub roof_type: Option<String>,
    #[schema(example = "1-3 months")]
    pub purchase_timeframe: Option<String>,
    #[schema(example = json!(false))]
    pub is_test: Option<bool>,
    #[schema(example = json!(false))]
    pub verbose: Option<bool>,
}

/// Response for POST /api/v1/leads
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadResponse {
    pub status: DocStatusNode,
    pub lead: DocLeadNode,
    pub verbose: serde_json::Value,
    pub http_status: u16,
}

#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocStatusNode {
    pub success: bool,
    pub status: String,
    pub message: String,
    pub error: String,
}

/// Lead payload in response. promise_id is only present for ping/post (used to send post); omitted for fullpost.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadNode {
    /// Present for ping/post; omitted for fullpost (not applicable).
    pub promise_id: Option<String>,
    pub lead_id: String,
    pub lead_uuid: String,
    pub ping_id: String,
    pub bid: f64,
    pub post_id: String,
    pub price: f64,
}

#[utoipa::path(
    post,
    path = "/api/v1/leads/ping",
    request_body(content = DocLeadPingRequest, description = "Solar Ping request", example = json!({
        "verbose": false,
        "lead": {
            "vertical": "solar",
            "publisher_id": TEST_PUBLISHER_ID,
            "request_type": "ping",
            "campaign_token": "camp_abc123",
            "promise_id": "prom_xyz789",
            "lead_id": "lead_john_doe_001",
            "first_name": "John",
            "last_name": "Doe",
            "email": "john.doe@example.com",
            "cell_phone": "5551234567",
            "street_address": "123 Main St",
            "city": "Anytown",
            "state": "CA",
            "zip": "90210",
            "monthly_bill": 150.0,
            "credit_rating": "good",
            "own_home": true,
            "property_type": "single_family",
            "roof_shade": "partial",
            "roof_type": "composition",
            "utility_provider": "Acme Electric",
            "purchase_timeframe": "1-3 months",
            "ip_address": "192.168.1.1",
            "tcpa_consent": true,
            "tcpa_language": "I agree to be contacted.",
            "jornaya_lead_id": "jornaya_lead_123",
            "trusted_form_url": "https://cert.trustedform.com/example",
            "is_test": false,
            "verbose": false
        }
    })),
    params(
        ("X-API-Key" = String, Header, description = "Leads Test API key", example = json!(TEST_API_KEY)),
        ("X-HMAC-Signature" = Option<String>, Header, description = "Optional when publisher does not require HMAC", example = json!("hmac_sha256_placeholder"))
    ),
    responses(
        (status = 200, description = "Ping accepted (sync).", body = DocLeadResponse, content_type = "application/json",
            example = json!({
                "status": { "success": true, "status": "ping_accepted", "message": "", "error": "" },
                "lead": {
                    "promise_id": "prom_john_doe_abc123",
                    "lead_id": "lead_john_doe_001",
                    "lead_uuid": "550e8400-e29b-41d4-a716-446655440001",
                    "ping_id": "ping_ext_xyz789",
                    "bid": 45.50,
                    "post_id": "",
                    "price": 0.0
                },
                "verbose": {},
                "http_status": 200
            })
        ),
        (status = 202, description = "Ping accepted (async queued).", body = DocLeadResponse, content_type = "application/json",
            example = json!({
                "status": { "success": true, "status": "accepted", "message": "Lead queued for processing.", "error": "" },
                "lead": {
                    "promise_id": "prom_async_john_doe",
                    "lead_id": "lead_john_doe_001",
                    "lead_uuid": "550e8400-e29b-41d4-a716-446655440002",
                    "ping_id": "",
                    "bid": 0.0,
                    "post_id": "",
                    "price": 0.0
                },
                "verbose": {},
                "http_status": 202
            })
        ),
        (status = 400, description = "Bad request (invalid payload or vertical).", body = DocLeadResponse, content_type = "application/json",
            example = json!({
                "status": { "success": false, "status": "error", "message": "Invalid vertical: solarx", "error": "Invalid vertical slug: solarx" },
                "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
                "verbose": {},
                "http_status": 400
            })
        ),
        (status = 401, description = "Unauthorized: missing or invalid API key, or HMAC signature failure.", body = DocLeadResponse, content_type = "application/json",
            example = json!({
                "status": { "success": false, "status": "error", "message": "Invalid API key", "error": "Authentication failed" },
                "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
                "verbose": {},
                "http_status": 401
            })
        ),
        (status = 404, description = "Not found: lead not found for the given promise_id (e.g. on post).", body = DocLeadResponse, content_type = "application/json",
            example = json!({
                "status": { "success": false, "status": "error", "message": "Lead not found", "error": "Lead not found" },
                "lead": { "promise_id": "prom_missing_123", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
                "verbose": {},
                "http_status": 404
            })
        ),
        (status = 429, description = "Rate limited (max 360 requests/hour).", body = DocLeadResponse, content_type = "application/json",
            example = json!({
                "status": { "success": false, "status": "error", "message": "Rate limit exceeded: max 360 requests per hour", "error": "Too many requests" },
                "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
                "verbose": { "status_code": 429, "rate_limit_per_hour": 360, "remaining": 0, "retry_after_seconds": 1200 },
                "http_status": 429
            })
        ),
        (status = 500, description = "Internal server error: database or routing failure.", body = DocLeadResponse, content_type = "application/json",
            example = json!({
                "status": { "success": false, "status": "error", "message": "An internal server error occurred.", "error": "error returned from database: connection failed" },
                "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
                "verbose": {},
                "http_status": 500
            })
        ),
        (status = 503, description = "Service unavailable: API in minimal mode or temporarily overloaded.", body = DocLeadResponse, content_type = "application/json",
            example = json!({
                "status": { "success": false, "status": "error", "message": "Service temporarily unavailable.", "error": "Service unavailable" },
                "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
                "verbose": {},
                "http_status": 503
            })
        )
    ),
    tag = "Solar",
    summary = "Ping",
    description = "Solar Ping. Required fields are in the request body schema. verbose=true returns diagnostic details. Ping does not use promise_id; use Post with the promise_id from the Ping response to commit."
)]
#[allow(dead_code)]
fn post_leads_ping() {}

#[utoipa::path(
    post,
    path = "/api/v1/leads/post",
    request_body(content = DocLeadPostRequest, description = "Solar Post request", example = json!({
        "verbose": false,
        "lead": {
            "vertical": "solar",
            "request_type": "post",
            "promise_id": "prom_john_doe_abc123",
            "publisher_id": TEST_PUBLISHER_ID,
            "lead_id": "lead_john_doe_001",
            "first_name": "John",
            "last_name": "Doe",
            "email": "john.doe@example.com",
            "cell_phone": "5551234567",
            "street_address": "123 Main St",
            "city": "Anytown",
            "state": "CA",
            "zip": "90210",
            "monthly_bill": 150.0,
            "credit_rating": "good",
            "own_home": true,
            "property_type": "single_family",
            "roof_shade": "partial",
            "roof_type": "composition",
            "utility_provider": "Acme Electric",
            "purchase_timeframe": "1-3 months",
            "ip_address": "192.168.1.1",
            "tcpa_consent": true,
            "tcpa_language": "I agree to be contacted.",
            "jornaya_lead_id": "jornaya_lead_123",
            "trusted_form_url": "https://cert.trustedform.com/example",
            "is_test": false,
            "verbose": false
        }
    })),
    params(
        ("X-API-Key" = String, Header, description = "Leads Test API key", example = json!(TEST_API_KEY)),
        ("X-HMAC-Signature" = Option<String>, Header, description = "Optional when publisher does not require HMAC", example = json!("hmac_sha256_placeholder"))
    ),
    responses(
        (status = 200, description = "Post accepted/sold.", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": true, "status": "sold", "message": "", "error": "" },
            "lead": { "promise_id": "prom_john_doe_abc123", "lead_id": "lead_john_doe_001", "lead_uuid": "550e8400-e29b-41d4-a716-446655440001", "ping_id": "ping_ext_xyz789", "bid": 45.5, "post_id": "post_ext_123", "price": 62.0 },
            "verbose": {},
            "http_status": 200
        })),
        (status = 400, description = "Bad request (missing required field).", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": false, "status": "error", "message": "Missing required field: promise_id", "error": "Missing required field: promise_id" },
            "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
            "verbose": {},
            "http_status": 400
        })),
        (status = 401, description = "Unauthorized.", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": false, "status": "error", "message": "Invalid API key", "error": "Authentication failed" },
            "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
            "verbose": {},
            "http_status": 401
        })),
        (status = 404, description = "Lead not found for promise_id.", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": false, "status": "error", "message": "Lead not found", "error": "Lead not found" },
            "lead": { "promise_id": "prom_missing_123", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
            "verbose": {},
            "http_status": 404
        })),
        (status = 429, description = "Rate limited (max 360 requests/hour).", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": false, "status": "error", "message": "Rate limit exceeded: max 360 requests per hour", "error": "Too many requests" },
            "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
            "verbose": { "status_code": 429, "rate_limit_per_hour": 360, "remaining": 0, "retry_after_seconds": 1200 },
            "http_status": 429
        })),
        (status = 500, description = "Internal server error.", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": false, "status": "error", "message": "An internal server error occurred.", "error": "error returned from database: connection failed" },
            "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
            "verbose": {},
            "http_status": 500
        }))
    ),
    tag = "Solar",
    summary = "Post",
    description = "Solar Post. Required fields are in the request body schema. Include the promise_id from the prior Ping response."
)]
#[allow(dead_code)]
fn post_leads_post() {}

#[utoipa::path(
    post,
    path = "/api/v1/leads/fullpost",
    request_body(content = DocLeadFullpostRequest, description = "Solar Fullpost request", example = json!({
        "verbose": false,
        "lead": {
            "vertical": "solar",
            "request_type": "fullpost",
            "publisher_id": TEST_PUBLISHER_ID,
            "lead_id": "lead_john_doe_001",
            "first_name": "John",
            "last_name": "Doe",
            "email": "john.doe@example.com",
            "cell_phone": "5551234567",
            "street_address": "123 Main St",
            "city": "Anytown",
            "state": "CA",
            "zip": "90210",
            "monthly_bill": 150.0,
            "credit_rating": "good",
            "own_home": true,
            "property_type": "single_family",
            "roof_shade": "partial",
            "roof_type": "composition",
            "utility_provider": "Acme Electric",
            "purchase_timeframe": "1-3 months",
            "ip_address": "192.168.1.1",
            "tcpa_consent": true,
            "tcpa_language": "I agree to be contacted.",
            "jornaya_lead_id": "jornaya_lead_123",
            "trusted_form_url": "https://cert.trustedform.com/example",
            "is_test": false,
            "verbose": false
        }
    })),
    params(
        ("X-API-Key" = String, Header, description = "Leads Test API key", example = json!(TEST_API_KEY)),
        ("X-HMAC-Signature" = Option<String>, Header, description = "Optional when publisher does not require HMAC", example = json!("hmac_sha256_placeholder"))
    ),
    responses(
        (status = 200, description = "Fullpost accepted/sold. promise_id is not returned for fullpost.", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": true, "status": "sold", "message": "", "error": "" },
            "lead": { "lead_id": "lead_john_doe_001", "lead_uuid": "550e8400-e29b-41d4-a716-446655440003", "ping_id": "ping_ext_xyz790", "bid": 44.0, "post_id": "post_ext_124", "price": 61.0 },
            "verbose": {},
            "http_status": 200
        })),
        (status = 202, description = "Fullpost accepted (async queued). promise_id is not returned for fullpost.", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": true, "status": "accepted", "message": "Lead queued for processing.", "error": "" },
            "lead": { "lead_id": "lead_john_doe_001", "lead_uuid": "550e8400-e29b-41d4-a716-446655440004", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
            "verbose": {},
            "http_status": 202
        })),
        (status = 400, description = "Bad request.", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": false, "status": "error", "message": "Invalid payload", "error": "Required data missing for this operation." },
            "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
            "verbose": {},
            "http_status": 400
        })),
        (status = 401, description = "Unauthorized.", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": false, "status": "error", "message": "Invalid API key", "error": "Authentication failed" },
            "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
            "verbose": {},
            "http_status": 401
        })),
        (status = 429, description = "Rate limited (max 360 requests/hour).", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": false, "status": "error", "message": "Rate limit exceeded: max 360 requests per hour", "error": "Too many requests" },
            "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
            "verbose": { "status_code": 429, "rate_limit_per_hour": 360, "remaining": 0, "retry_after_seconds": 1200 },
            "http_status": 429
        })),
        (status = 500, description = "Internal server error.", body = DocLeadResponse, content_type = "application/json", example = json!({
            "status": { "success": false, "status": "error", "message": "An internal server error occurred.", "error": "error returned from database: connection failed" },
            "lead": { "promise_id": "", "lead_id": "", "lead_uuid": "", "ping_id": "", "bid": 0.0, "post_id": "", "price": 0.0 },
            "verbose": {},
            "http_status": 500
        }))
    ),
    tag = "Solar",
    summary = "Fullpost",
    description = "Solar Fullpost (single-step: ping + post in one request). Required fields are in the request body schema. Response does not include promise_id (only used for ping/post two-step flow)."
)]
#[allow(dead_code)]
fn post_leads_fullpost() {}

/// Serves the Scalar API documentation UI at GET /documentation
pub async fn serve_scalar_html(_state: State<AppState>) -> impl IntoResponse {
    let html = utoipa_scalar::Scalar::new(ApiDoc::openapi()).to_html();
    Html(html)
}

/// Serves the OpenAPI spec as JSON at GET /documentation/openapi.json
pub async fn serve_openapi_json(_state: State<AppState>) -> impl IntoResponse {
    Json(ApiDoc::openapi())
}
