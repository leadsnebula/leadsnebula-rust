//! OpenAPI spec for the lead submission API, served at `/documentation` via Scalar.

use axum::extract::State;
use axum::response::{Html, IntoResponse};
use serde_json::json;
use utoipa::openapi::path::{ParameterBuilder, ParameterIn};
use utoipa::openapi::schema::{ObjectBuilder, Schema, Type};
use utoipa::openapi::RefOr;
use utoipa::openapi::Required;
use utoipa::Modify;
use utoipa::OpenApi;

use crate::AppState;

/// Leads Test publisher and API key for Try-it (pre-filled in documentation).
const TEST_PUBLISHER_ID: &str = "a1b2c3d4-e5f6-4780-a123-456789abcdef";
const TEST_API_KEY: &str =
    "pk_live_3e334617ff337358ddc911041b24125feb41f2e576d13ed9a8e421a1952ef350";

/// Pre-formatted request-body example JSON with keys in doc order (verbose, lead; inside lead:
/// request_properties, publisher_data, consumer_data, property_data, compliance).
/// Used to replace the serialized example in the embedded spec so Try-it shows correct order.
const PING_EXAMPLE_JSON: &str = r#"{"verbose":false,"lead":{"request_properties":{"vertical":"solar","is_test":false,"request_type":"ping"},"publisher_data":{"publisher_id":"a1b2c3d4-e5f6-4780-a123-456789abcdef","campaign_token":"camp_abc123","source":"https://example.com/solar-form"},"consumer_data":{"first_name":"John","last_name":"Doe","email":"john.doe@example.com","cell_phone":"5551234567","street_address":"123 Main St","city":"Anytown","state":"CA","zip":"90210","credit_rating":"good","ip_address":"192.168.1.1"},"property_data":{"monthly_bill":150.0,"own_home":true,"property_type":"single_family","purchase_timeframe":"1-3 months","roof_shade":"partial","roof_type":"composition","utility_provider":"Acme Electric"},"compliance":{"tcpa_consent":true,"tcpa_language":"I agree to be contacted.","jornaya_lead_id":"jornaya_lead_123","trusted_form_url":"https://cert.trustedform.com/example"}}}"#;
const POST_EXAMPLE_JSON: &str = r#"{"verbose":false,"lead":{"request_properties":{"vertical":"solar","is_test":false,"request_type":"post"},"publisher_data":{"publisher_id":"a1b2c3d4-e5f6-4780-a123-456789abcdef","campaign_token":"camp_john_doe_solar","lead_id":"Populate from the PING response","promise_id":"Populate from the PING response","source":"https://example.com/solar-form"},"consumer_data":{"first_name":"John","last_name":"Doe","email":"john.doe@example.com","cell_phone":"5551234567","street_address":"123 Main St","city":"Anytown","state":"CA","zip":"90210","credit_rating":"good","ip_address":"192.168.1.1"},"property_data":{"monthly_bill":150.0,"own_home":true,"roof_shade":"partial","utility_provider":"Acme Electric","property_type":"single_family","roof_type":"composition","purchase_timeframe":"1-3 months"},"compliance":{"tcpa_consent":true,"tcpa_language":"I agree to be contacted.","jornaya_lead_id":"jornaya_lead_123","trusted_form_url":"https://cert.trustedform.com/example"}}}"#;
const FULLPOST_EXAMPLE_JSON: &str = r#"{"verbose":false,"lead":{"request_properties":{"vertical":"solar","is_test":false,"request_type":"fullpost"},"publisher_data":{"publisher_id":"a1b2c3d4-e5f6-4780-a123-456789abcdef","campaign_token":"camp_john_doe_solar","source":"https://example.com/solar-form"},"consumer_data":{"first_name":"John","last_name":"Doe","email":"john.doe@example.com","cell_phone":"5551234567","street_address":"123 Main St","city":"Anytown","state":"CA","zip":"90210","credit_rating":"good","ip_address":"192.168.1.1"},"property_data":{"monthly_bill":150.0,"own_home":true,"roof_shade":"partial","utility_provider":"Acme Electric","property_type":"single_family","roof_type":"composition","purchase_timeframe":"1-3 months"},"compliance":{"tcpa_consent":true,"tcpa_language":"I agree to be contacted.","jornaya_lead_id":"jornaya_lead_123","trusted_form_url":"https://cert.trustedform.com/example"}}}"#;

const API_DESCRIPTION: &str = r#"Lead submission and routing.

**Models: Fullpost and Ping/Post**

Leads Nebula supports two submission models:

- **Fullpost** — Single request that performs ping and post in one call. The platform may split this into an internal ping and post when sending to the end buyer if the publisher prefers that flow.
- **Ping/Post** — Two-step flow: send a ping first, then send a post with the `lead_id` and `promise_id` from the ping response to commit the lead.

Leads Nebula accepts both models and can separate a fullpost into ping and post when forwarding to the buyer if configured that way.

**Vertical**

`vertical` is required on every request. It determines which fields are required and which vertical-specific rules apply. In this section the value is `solar`; other verticals (e.g. HVAC) will use their own value in their section.

**Verbose flag**

When set to `true` (optional), the response includes a `verbose` object to assist with debugging and troubleshooting. The `verbose` flag must be included **outside** the main lead body (at the top level of the request). Example response shape when `verbose` is used:

```json
"verbose": {
  "endpoint": "POST /api/v1/leads/ping",
  "timestamp": "2025-01-15T12:00:00Z",
  "request_id": "req_abc123",
  "status_code": 200
}
```

Verbose is optional; omit it or set to `false` for normal responses.

**Test credentials**

- **TEST Publisher ID:** `a1b2c3d4-e5f6-4780-a123-456789abcdef`
- **TEST API Key:** `pk_live_3e334617ff337358ddc911041b24125feb41f2e576d13ed9a8e421a1952ef350` (must be sent in the `X-API-Key` header)

**HMAC**

HMAC signing is optional. It must be enabled in your instance; if enabled, the signature must be sent in the `X-HMAC-Signature` header. If your instance does not require HMAC, you can omit this header.

**Headers**

- `Content-Type: application/json` — Required.
- `X-API-Key` — Required. Your publisher API key.
- `X-HMAC-Signature` — Optional. Required only when HMAC is enabled for your instance.
"#;

/// OpenAPI specification for the lead submission API.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Leads API",
        version = "0.1.0",
        description = API_DESCRIPTION
    ),
    tags(
        (name = "Solar", description = "Solar vertical. Set `vertical` to `solar` in request_properties. Expand to view Ping, Post, and Fullpost.")
    ),
    paths(
        post_leads_ping,
        post_leads_post,
        post_leads_fullpost
    ),
    components(
        schemas(
            DocLeadPingRequest,
            DocLeadPingDataNested,
            DocLeadPostRequest,
            DocLeadPostDataNested,
            DocLeadFullpostRequest,
            DocLeadFullpostDataNested,
            DocLeadResponse,
            DocStatusNode,
            DocLeadNode,
            DocVerbose,
            DocVerboseRouting
        )
    ),
    modifiers(
        &XApiKeyDefaultModifier,
        &InlineLeadSchemaModifier,
        &AddJsonExampleModifier,
        &ReorderRequestSchemaModifier
    )
)]
pub struct ApiDoc;

/// Modifier that ensures the X-API-Key header parameter exists with a default so Scalar's try-it pre-fills it.
/// Lead paths do not declare it in #[utoipa::path], so we add it to each operation.
struct XApiKeyDefaultModifier;

fn x_api_key_parameter() -> utoipa::openapi::path::Parameter {
    ParameterBuilder::new()
        .name("X-API-Key")
        .parameter_in(ParameterIn::Header)
        .required(Required::True)
        .description(Some("Your publisher API key (required)."))
        .schema(Some(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .default(Some(json!(TEST_API_KEY)))
                .build(),
        )))
        .build()
}

impl Modify for XApiKeyDefaultModifier {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let api_key_param = x_api_key_parameter();
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
                let has_api_key = op
                    .parameters
                    .as_ref()
                    .map(|p| {
                        p.iter()
                            .any(|x| x.name == "X-API-Key" && x.parameter_in == ParameterIn::Header)
                    })
                    .unwrap_or(false);
                if has_api_key {
                    if let Some(params) = op.parameters.as_mut() {
                        for param in params.iter_mut() {
                            if param.name == "X-API-Key"
                                && param.parameter_in == ParameterIn::Header
                            {
                                let default_value = json!(TEST_API_KEY);
                                match param.schema.as_mut() {
                                    Some(RefOr::T(Schema::Object(obj))) => {
                                        obj.default = Some(default_value.clone());
                                    }
                                    _ => {
                                        param.schema = Some(RefOr::T(Schema::Object(
                                            ObjectBuilder::new()
                                                .schema_type(Type::String)
                                                .default(Some(default_value))
                                                .build(),
                                        )));
                                    }
                                }
                                break;
                            }
                        }
                    }
                } else {
                    let mut params = op.parameters.take().unwrap_or_default();
                    params.insert(0, api_key_param.clone());
                    op.parameters = Some(params);
                }
            }
        }
    }
}

/// Inlines the nested lead schema into each request body so Try-it shows grouped nodes.
struct InlineLeadSchemaModifier;

impl Modify for InlineLeadSchemaModifier {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = match openapi.components.as_mut() {
            Some(c) => c,
            None => return,
        };
        let schemas = &mut components.schemas;

        let pairs: [(&str, &str); 3] = [
            ("DocLeadPingRequest", "DocLeadPingDataNested"),
            ("DocLeadPostRequest", "DocLeadPostDataNested"),
            ("DocLeadFullpostRequest", "DocLeadFullpostDataNested"),
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

/// Reorder request body schema: top-level verbose then lead; inside lead: request_properties, publisher_data, consumer_data, property_data, compliance.
struct ReorderRequestSchemaModifier;

impl Modify for ReorderRequestSchemaModifier {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = match openapi.components.as_mut() {
            Some(c) => c,
            None => return,
        };
        let schemas = &mut components.schemas;
        let top_order = ["verbose", "lead"];
        let lead_order = [
            "request_properties",
            "publisher_data",
            "consumer_data",
            "property_data",
            "compliance",
        ];
        for name in [
            "DocLeadPingRequest",
            "DocLeadPostRequest",
            "DocLeadFullpostRequest",
        ] {
            if let Some(RefOr::T(Schema::Object(obj))) = schemas.get_mut(name) {
                reorder_properties(obj, &top_order);
                if let Some(RefOr::T(Schema::Object(lead))) = obj.properties.get_mut("lead") {
                    reorder_properties(lead, &lead_order);
                }
            }
        }
        for name in [
            "DocLeadPingDataNested",
            "DocLeadPostDataNested",
            "DocLeadFullpostDataNested",
        ] {
            if let Some(RefOr::T(Schema::Object(obj))) = schemas.get_mut(name) {
                reorder_properties(obj, &lead_order);
            }
        }
    }
}

fn reorder_properties(obj: &mut utoipa::openapi::schema::Object, order: &[&str]) {
    use indexmap::IndexMap;
    let mut props = std::mem::take(&mut obj.properties);
    // IndexMap preserves insertion order so the doc UI and Try-it show the desired order.
    let mut new_props: IndexMap<String, RefOr<Schema>> = IndexMap::new();
    for key in order {
        if let Some(v) = props.shift_remove(*key) {
            new_props.insert((*key).to_string(), v);
        }
    }
    for (k, v) in props {
        new_props.insert(k, v);
    }
    obj.properties = new_props;
}

/// Add a "JSON" named example to each lead request body so users can pick a preformatted JSON body.
struct AddJsonExampleModifier;

impl Modify for AddJsonExampleModifier {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use std::collections::BTreeMap;

        let json_examples: [(&str, serde_json::Value); 3] = [
            (
                "/api/v1/leads/ping",
                json!({
                    "verbose": false,
                    "lead": {
                        "request_properties": { "vertical": "solar", "is_test": false, "request_type": "ping" },
                        "publisher_data": { "publisher_id": TEST_PUBLISHER_ID, "campaign_token": "camp_abc123", "source": "https://example.com/solar-form" },
                        "consumer_data": { "first_name": "John", "last_name": "Doe", "email": "john.doe@example.com", "cell_phone": "5551234567", "street_address": "123 Main St", "city": "Anytown", "state": "CA", "zip": "90210", "credit_rating": "good", "ip_address": "192.168.1.1" },
                        "property_data": { "monthly_bill": 150.0, "own_home": true, "property_type": "single_family", "purchase_timeframe": "1-3 months", "roof_shade": "partial", "roof_type": "composition", "utility_provider": "Acme Electric" },
                        "compliance": { "tcpa_consent": true, "tcpa_language": "I agree to be contacted.", "jornaya_lead_id": "jornaya_lead_123", "trusted_form_url": "https://cert.trustedform.com/example" }
                    }
                }),
            ),
            (
                "/api/v1/leads/post",
                json!({
                    "verbose": false,
                    "lead": {
                        "request_properties": { "vertical": "solar", "is_test": false, "request_type": "post" },
                        "publisher_data": { "publisher_id": TEST_PUBLISHER_ID, "campaign_token": "camp_john_doe_solar", "lead_id": "Populate from the PING response", "promise_id": "Populate from the PING response", "source": "https://example.com/solar-form" },
                        "consumer_data": { "first_name": "John", "last_name": "Doe", "email": "john.doe@example.com", "cell_phone": "5551234567", "street_address": "123 Main St", "city": "Anytown", "state": "CA", "zip": "90210", "credit_rating": "good", "ip_address": "192.168.1.1" },
                        "property_data": { "monthly_bill": 150.0, "own_home": true, "roof_shade": "partial", "utility_provider": "Acme Electric", "property_type": "single_family", "roof_type": "composition", "purchase_timeframe": "1-3 months" },
                        "compliance": { "tcpa_consent": true, "tcpa_language": "I agree to be contacted.", "jornaya_lead_id": "jornaya_lead_123", "trusted_form_url": "https://cert.trustedform.com/example" }
                    }
                }),
            ),
            (
                "/api/v1/leads/fullpost",
                json!({
                    "verbose": false,
                    "lead": {
                        "request_properties": { "vertical": "solar", "is_test": false, "request_type": "fullpost" },
                        "publisher_data": { "publisher_id": TEST_PUBLISHER_ID, "campaign_token": "camp_john_doe_solar", "source": "https://example.com/solar-form" },
                        "consumer_data": { "first_name": "John", "last_name": "Doe", "email": "john.doe@example.com", "cell_phone": "5551234567", "street_address": "123 Main St", "city": "Anytown", "state": "CA", "zip": "90210", "credit_rating": "good", "ip_address": "192.168.1.1" },
                        "property_data": { "monthly_bill": 150.0, "own_home": true, "roof_shade": "partial", "utility_provider": "Acme Electric", "property_type": "single_family", "roof_type": "composition", "purchase_timeframe": "1-3 months" },
                        "compliance": { "tcpa_consent": true, "tcpa_language": "I agree to be contacted.", "jornaya_lead_id": "jornaya_lead_123", "trusted_form_url": "https://cert.trustedform.com/example" }
                    }
                }),
            ),
        ];

        for (path_key, value) in json_examples {
            let path_item = match openapi.paths.paths.get_mut(path_key) {
                Some(p) => p,
                None => continue,
            };
            if let Some(ref mut op) = path_item.post {
                if let Some(ref mut body) = op.request_body {
                    if let Some(content) = body.content.get_mut("application/json") {
                        // Single example in requested order so Try-it body is pre-filled correctly.
                        // (If we set both example and examples, OpenAPI 3 says examples wins;
                        // one example only so Scalar uses this as the default body.)
                        content.example = Some(value.clone());
                        content.examples = BTreeMap::new();
                    }
                }
            }
        }
    }
}

// ----- Nested group schemas (Try-it shows these as separate nodes) -----

/// Request properties. Do not duplicate these at the top level of the request; they belong only in this group inside the lead body.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocRequestProperties {
    /// Vertical slug. Use `solar` for Solar; other verticals (e.g. HVAC) use their own value. Required on all requests.
    #[schema(example = "solar")]
    pub vertical: Option<String>,
    /// When true, the lead is marked as a test lead: it receives status `test`, does not count toward revenue, and is excluded from reporting.
    #[schema(example = json!(false))]
    pub is_test: Option<bool>,
    /// One of `fullpost`, `ping`, or `post` depending on the submission model.
    #[schema(example = "ping")]
    pub request_type: Option<String>,
}

#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocPublisherData {
    #[schema(example = "a1b2c3d4-e5f6-4780-a123-456789abcdef")]
    pub publisher_id: Option<String>,
    #[schema(example = "camp_john_doe_solar")]
    pub campaign_token: Option<String>,
    /// Omit on ping and fullpost (server generates and returns it). Required on post; use the value from the ping response.
    #[schema(example = json!(null))]
    pub lead_id: Option<String>,
    /// Required on post only; use the value from the ping response.
    #[schema(example = json!(null))]
    pub promise_id: Option<String>,
    /// Publisher website or traffic source URL (e.g. the page where the lead was captured). Optional; passed through when supported.
    #[schema(example = "https://example.com/solar-form")]
    pub source: Option<String>,
}

#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocConsumerData {
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
    #[schema(example = "90210")]
    pub zip: Option<String>,
    /// Allowed values: good, fair, poor.
    #[schema(example = "good")]
    pub credit_rating: Option<String>,
    #[schema(example = "192.168.1.1")]
    pub ip_address: Option<String>,
    #[schema(example = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")]
    pub user_agent: Option<String>,
}

#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocPropertyData {
    /// Monthly utility bill amount in dollars (numeric).
    #[schema(example = json!(150.0))]
    pub monthly_bill: Option<f64>,
    #[schema(example = json!(true))]
    pub own_home: Option<bool>,
    /// Allowed values: single_family, multi_family, condo, townhouse.
    #[schema(example = "single_family")]
    pub property_type: Option<String>,
    /// Allowed values: 1-3 months, 3-6 months, 6-12 months, 12+ months.
    #[schema(example = "1-3 months")]
    pub purchase_timeframe: Option<String>,
    /// Allowed values: full_sun, partial, full_shade.
    #[schema(example = "partial")]
    pub roof_shade: Option<String>,
    /// Allowed values: composition, metal, tile, flat.
    #[schema(example = "composition")]
    pub roof_type: Option<String>,
    #[schema(example = "Acme Electric")]
    pub utility_provider: Option<String>,
}

#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocCompliance {
    #[schema(example = json!(true))]
    pub tcpa_consent: Option<bool>,
    #[schema(example = "I agree to be contacted.")]
    pub tcpa_language: Option<String>,
    #[schema(example = "jornaya_lead_123")]
    pub jornaya_lead_id: Option<String>,
    #[schema(example = "https://cert.trustedform.com/example")]
    pub trusted_form_url: Option<String>,
}

/// Request body for POST /api/v1/leads/ping. Top-level `verbose` only.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadPingRequest {
    #[schema(example = json!(false))]
    pub verbose: Option<bool>,
    pub lead: DocLeadPingDataNested,
}

/// Solar Ping lead body with grouped nodes (Try-it shows separate sections).
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadPingDataNested {
    pub request_properties: DocRequestProperties,
    pub publisher_data: DocPublisherData,
    pub consumer_data: DocConsumerData,
    pub property_data: DocPropertyData,
    pub compliance: DocCompliance,
}

/// Request body for POST /api/v1/leads/post. Top-level `verbose` only.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadPostRequest {
    #[schema(example = json!(false))]
    pub verbose: Option<bool>,
    pub lead: DocLeadPostDataNested,
}

/// Solar Post lead body with grouped nodes (Try-it shows separate sections).
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadPostDataNested {
    pub request_properties: DocRequestProperties,
    pub publisher_data: DocPublisherData,
    pub consumer_data: DocConsumerData,
    pub property_data: DocPropertyData,
    pub compliance: DocCompliance,
}

/// Request body for POST /api/v1/leads/fullpost. Top-level `verbose` only.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadFullpostRequest {
    #[schema(example = json!(false))]
    pub verbose: Option<bool>,
    pub lead: DocLeadFullpostDataNested,
}

/// Solar Fullpost lead body with grouped nodes (Try-it shows separate sections).
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadFullpostDataNested {
    pub request_properties: DocRequestProperties,
    pub publisher_data: DocPublisherData,
    pub consumer_data: DocConsumerData,
    pub property_data: DocPropertyData,
    pub compliance: DocCompliance,
}

/// Response for POST /api/v1/leads
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocLeadResponse {
    pub status: DocStatusNode,
    pub lead: DocLeadNode,
    /// Present only when the request included `verbose: true` at the top level. Omitted otherwise.
    #[schema(required = false)]
    pub verbose: Option<DocVerbose>,
    #[schema(required = false)]
    pub http_status: Option<u16>,
}

/// Verbose object returned when the request had `verbose: true`. All fields are optional; only those relevant to the response are included.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocVerbose {
    /// Error code (e.g. ERR_200).
    pub error_code: Option<String>,
    /// ISO 8601 timestamp.
    pub timestamp: Option<String>,
    /// Endpoint called (e.g. POST /api/v1/leads).
    pub endpoint: Option<String>,
    /// HTTP status code.
    pub status_code: Option<u16>,
    /// Winning buyer/campaign when request succeeded.
    pub routing: Option<DocVerboseRouting>,
    /// Per-buyer timing and bid/status (ping/post: includes bid; fullpost: no bid).
    pub per_buyer_timings: Option<Vec<serde_json::Value>>,
    /// Rate limit: max requests per hour (429 only).
    pub rate_limit_per_hour: Option<u32>,
    /// Rate limit: remaining in window (429 only).
    pub remaining: Option<u32>,
    /// Rate limit: retry-after seconds (429 only).
    pub retry_after_seconds: Option<u64>,
}

#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct DocVerboseRouting {
    pub buyer_name: Option<String>,
    pub buyer_id: Option<String>,
    pub campaign_name: Option<String>,
    pub campaign_id: Option<String>,
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
    pub lead_id: Option<String>,
    pub lead_uuid: Option<String>,
    /// Present for ping (winning bid amount). Omitted for post/fullpost.
    pub ping_id: Option<String>,
    /// Winning bid amount (ping only). Omitted for post/fullpost.
    pub bid: Option<f64>,
    pub post_id: Option<String>,
    /// Sale price (post/fullpost). Omitted for ping.
    pub price: Option<f64>,
}

#[utoipa::path(
    post,
    path = "/api/v1/leads/ping",
    request_body(content = DocLeadPingRequest, description = "Ping request. lead_id does not exist yet; the server creates it and returns it in the response. Use that value with promise_id when sending the subsequent post. Set verbose at the top level (outside the lead body) if you need the verbose object.", example = json!({
        "verbose": false,
        "lead": {
            "request_properties": { "vertical": "solar", "is_test": false, "request_type": "ping" },
            "publisher_data": { "publisher_id": TEST_PUBLISHER_ID, "campaign_token": "camp_abc123", "source": "https://example.com/solar-form" },
            "consumer_data": {
                "first_name": "John", "last_name": "Doe", "email": "john.doe@example.com",
                "cell_phone": "5551234567", "street_address": "123 Main St", "city": "Anytown",
                "state": "CA", "zip": "90210", "credit_rating": "good", "ip_address": "192.168.1.1"
            },
            "property_data": {
                "monthly_bill": 150.0, "own_home": true, "property_type": "single_family",
                "purchase_timeframe": "1-3 months", "roof_shade": "partial", "roof_type": "composition",
                "utility_provider": "Acme Electric"
            },
            "compliance": {
                "tcpa_consent": true, "tcpa_language": "I agree to be contacted.",
                "jornaya_lead_id": "jornaya_lead_123", "trusted_form_url": "https://cert.trustedform.com/example"
            }
        }
    })),
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
    description = "Ping. Required fields are in the request body. Use the lead_id and promise_id from the response when sending the follow-up post."
)]
#[allow(dead_code)]
fn post_leads_ping() {}

#[utoipa::path(
    post,
    path = "/api/v1/leads/post",
    request_body(content = DocLeadPostRequest, description = "Post request. promise_id is required (from the ping response); lead_id is optional but recommended. Set verbose at the top level (outside the lead body) if needed.", example = json!({
        "verbose": false,
        "lead": {
            "request_properties": { "vertical": "solar", "is_test": false, "request_type": "post" },
            "publisher_data": {
                "publisher_id": TEST_PUBLISHER_ID, "campaign_token": "camp_john_doe_solar",
                "lead_id": "Populate from the PING response", "promise_id": "Populate from the PING response",
                "source": "https://example.com/solar-form"
            },
            "consumer_data": {
                "first_name": "John", "last_name": "Doe", "email": "john.doe@example.com",
                "cell_phone": "5551234567", "street_address": "123 Main St", "city": "Anytown",
                "state": "CA", "zip": "90210", "credit_rating": "good", "ip_address": "192.168.1.1"
            },
            "property_data": {
                "monthly_bill": 150.0, "own_home": true, "roof_shade": "partial",
                "utility_provider": "Acme Electric", "property_type": "single_family",
                "roof_type": "composition", "purchase_timeframe": "1-3 months"
            },
            "compliance": {
                "tcpa_consent": true, "tcpa_language": "I agree to be contacted.",
                "jornaya_lead_id": "jornaya_lead_123", "trusted_form_url": "https://cert.trustedform.com/example"
            }
        }
    })),
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
    description = "Post. Required: promise_id (from ping response). Optional: lead_id (recommended from ping)."
)]
#[allow(dead_code)]
fn post_leads_post() {}

#[utoipa::path(
    post,
    path = "/api/v1/leads/fullpost",
    request_body(content = DocLeadFullpostRequest, description = "Fullpost (single-step ping + post). lead_id is not used; the server handles identification. Set verbose at the top level (outside the lead body) if needed.", example = json!({
        "verbose": false,
        "lead": {
            "request_properties": { "vertical": "solar", "is_test": false, "request_type": "fullpost" },
            "publisher_data": { "publisher_id": TEST_PUBLISHER_ID, "campaign_token": "camp_john_doe_solar", "source": "https://example.com/solar-form" },
            "consumer_data": {
                "first_name": "John", "last_name": "Doe", "email": "john.doe@example.com",
                "cell_phone": "5551234567", "street_address": "123 Main St", "city": "Anytown",
                "state": "CA", "zip": "90210", "credit_rating": "good", "ip_address": "192.168.1.1"
            },
            "property_data": {
                "monthly_bill": 150.0, "own_home": true, "roof_shade": "partial",
                "utility_provider": "Acme Electric", "property_type": "single_family",
                "roof_type": "composition", "purchase_timeframe": "1-3 months"
            },
            "compliance": {
                "tcpa_consent": true, "tcpa_language": "I agree to be contacted.",
                "jornaya_lead_id": "jornaya_lead_123", "trusted_form_url": "https://cert.trustedform.com/example"
            }
        }
    })),
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
    description = "Fullpost: single request that performs ping and post. Required fields are in the request body. Response does not include promise_id (used only in the ping/post two-step flow)."
)]
#[allow(dead_code)]
fn post_leads_fullpost() {}

/// Finds the byte range of the request-body `"example": { ... }` value for a path in the serialized OpenAPI JSON.
/// Returns (start, end) inclusive so spec[start..=end] is the example object.
fn find_request_body_example_span(spec: &str, path_key: &str) -> Option<(usize, usize)> {
    let path_quoted = format!("\"{}\"", path_key);
    let after_path = spec.find(&path_quoted)? + path_quoted.len();
    let after_app_json = spec[after_path..].find("\"application/json\"")? + after_path;
    let after_example_key = spec[after_app_json..].find("\"example\"")? + after_app_json;
    // Value is after "example": possibly with whitespace
    let colon = spec[after_example_key..].find(':')? + after_example_key;
    let open_brace = spec[colon + 1..].find('{')? + (colon + 1);
    let close_brace = find_matching_brace(spec, open_brace)?;
    Some((open_brace, close_brace))
}

/// Skips whitespace from position; returns first index that is not whitespace.
fn skip_whitespace(spec: &str, mut i: usize) -> usize {
    let bytes = spec.as_bytes();
    while i < bytes.len()
        && (bytes[i] == b' ' || bytes[i] == b'\n' || bytes[i] == b'\r' || bytes[i] == b'\t')
    {
        i += 1;
    }
    i
}

/// Given index of opening `"` in JSON string, return index of closing `"` (handles backslash escapes).
fn find_string_end(spec: &str, open: usize) -> Option<usize> {
    let bytes = spec.as_bytes();
    if open >= bytes.len() || bytes[open] != b'"' {
        return None;
    }
    let mut i = open + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Given index of `{` in JSON string, return index of matching `}`. Skips strings and nested braces.
fn find_matching_brace(spec: &str, open: usize) -> Option<usize> {
    let bytes = spec.as_bytes();
    let mut depth = 1u32;
    let mut i = open + 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Desired order of keys inside "lead" in request schemas (main page and Try-it).
const LEAD_PROPERTY_ORDER: [&str; 5] = [
    "request_properties",
    "publisher_data",
    "consumer_data",
    "property_data",
    "compliance",
];

/// Finds the byte range of the "properties" object that is the value of "lead" inside
/// components.schemas.<schema_name>. Returns (start, end) inclusive.
fn find_lead_properties_span(spec: &str, schema_name: &str) -> Option<(usize, usize)> {
    let schema_key = format!("\"{}\"", schema_name);
    let after_schema = spec.find(&schema_key)? + schema_key.len();
    let after_lead_key = spec[after_schema..].find("\"lead\"")? + after_schema;
    let after_props_key = spec[after_lead_key..].find("\"properties\"")? + after_lead_key;
    let colon = spec[after_props_key..].find(':')? + after_props_key;
    let open_brace = spec[colon + 1..].find('{')? + (colon + 1);
    let close_brace = find_matching_brace(spec, open_brace)?;
    Some((open_brace, close_brace))
}

/// Extracts a JSON value starting at the given position (after the key's colon). Returns (value slice, end index inclusive).
fn extract_json_value(spec: &str, start: usize) -> Option<(&str, usize)> {
    let start = skip_whitespace(spec, start);
    let bytes = spec.as_bytes();
    if start >= bytes.len() {
        return None;
    }
    let end = match bytes[start] {
        b'{' => find_matching_brace(spec, start)?,
        b'"' => find_string_end(spec, start)?,
        _ => return None,
    };
    Some((&spec[start..=end], end))
}

/// Reorders the "properties" object inside "lead" for each request schema so the main page
/// (and any consumer that respects key order) shows: request_properties, publisher_data, consumer_data, property_data, compliance.
fn reorder_lead_properties_in_spec(spec: &str) -> String {
    const SCHEMA_NAMES: [&str; 3] = [
        "DocLeadPingRequest",
        "DocLeadPostRequest",
        "DocLeadFullpostRequest",
    ];
    let mut spans: Vec<(usize, usize, String)> = Vec::new();
    for &schema_name in &SCHEMA_NAMES {
        let (props_start, props_end) = match find_lead_properties_span(spec, schema_name) {
            Some(s) => s,
            None => continue,
        };
        let content = &spec[props_start..=props_end];
        let mut values: Vec<String> = Vec::new();
        for &key in &LEAD_PROPERTY_ORDER {
            let key_quoted = format!("\"{}\"", key);
            let key_pattern = format!("{}:", key_quoted);
            let pos_in_content = match content.find(&key_pattern) {
                Some(p) => p,
                None => continue,
            };
            let value_start_in_spec = props_start + pos_in_content + key_pattern.len();
            let (value_slice, _) = match extract_json_value(spec, value_start_in_spec) {
                Some(v) => v,
                None => continue,
            };
            values.push(value_slice.to_string());
        }
        if values.len() != 5 {
            continue;
        }
        let reordered = format!(
            "{{\"{}\":{},\"{}\":{},\"{}\":{},\"{}\":{},\"{}\":{}}}",
            LEAD_PROPERTY_ORDER[0],
            values[0],
            LEAD_PROPERTY_ORDER[1],
            values[1],
            LEAD_PROPERTY_ORDER[2],
            values[2],
            LEAD_PROPERTY_ORDER[3],
            values[3],
            LEAD_PROPERTY_ORDER[4],
            values[4],
        );
        spans.push((props_start, props_end, reordered));
    }
    // Replace from end to start so indices stay valid.
    spans.sort_by_key(|(s, _e, _)| std::cmp::Reverse(*s));
    let mut out = spec.to_string();
    for (start, end, replacement) in spans {
        out.replace_range(start..=end, &replacement);
    }
    out
}

/// Replaces request-body examples in the serialized OpenAPI spec with pre-ordered JSON so the
/// embedded spec (and Try-it) shows keys in doc order: verbose, lead; inside lead: request_properties, publisher_data, consumer_data, property_data, compliance.
fn replace_request_body_examples(spec: &str) -> String {
    const REPLACEMENTS: [(&str, &str); 3] = [
        ("/api/v1/leads/ping", PING_EXAMPLE_JSON),
        ("/api/v1/leads/post", POST_EXAMPLE_JSON),
        ("/api/v1/leads/fullpost", FULLPOST_EXAMPLE_JSON),
    ];
    let mut spans: Vec<(usize, usize, &str)> = REPLACEMENTS
        .iter()
        .filter_map(|(path, replacement)| {
            find_request_body_example_span(spec, path).map(|(s, e)| (s, e, *replacement))
        })
        .collect();
    // Replace from end to start so indices stay valid.
    spans.sort_by_key(|(s, _e, _)| std::cmp::Reverse(*s));
    let mut out = spec.to_string();
    for (start, end, replacement) in spans {
        out.replace_range(start..=end, replacement);
    }
    out
}

/// Scalar default HTML template (script tag with $spec placeholder).
const SCALAR_HTML_TEMPLATE: &str = r#"<!doctype html>
<html>
<head>
    <title>Scalar</title>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
</head>
<body>
<script id="api-reference" type="application/json">
    $spec
</script>
<script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
</body>
</html>"#;

/// Serves the Scalar API documentation UI at GET /documentation
pub async fn serve_scalar_html(_state: State<AppState>) -> impl IntoResponse {
    let openapi = ApiDoc::openapi();
    let spec_str = serde_json::to_string(&openapi).expect("OpenAPI serialization");
    let spec_str = replace_request_body_examples(&spec_str);
    let spec_str = reorder_lead_properties_in_spec(&spec_str);
    let html = SCALAR_HTML_TEMPLATE.replace("$spec", &spec_str);
    Html(html)
}

/// Serves the OpenAPI spec as JSON at GET /documentation/openapi.json (same example and schema order as doc).
pub async fn serve_openapi_json(_state: State<AppState>) -> impl IntoResponse {
    let openapi = ApiDoc::openapi();
    let spec_str = serde_json::to_string(&openapi).expect("OpenAPI serialization");
    let spec_str = replace_request_body_examples(&spec_str);
    let spec_str = reorder_lead_properties_in_spec(&spec_str);
    (
        [(
            axum::http::header::CONTENT_TYPE,
            axum::http::header::HeaderValue::from_static("application/json"),
        )],
        spec_str,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use utoipa::openapi::RefOr;
    use utoipa::openapi::Schema;

    /// Asserts that after modifiers, the request body schemas have property order: verbose, lead;
    /// and inside lead: request_properties, publisher_data, consumer_data, property_data, compliance.
    #[test]
    fn openapi_request_schema_property_order() {
        let openapi = ApiDoc::openapi();
        let components = openapi.components.as_ref().expect("components");
        let schemas = &components.schemas;

        let top_order: &[&str] = &["verbose", "lead"];
        let lead_order: &[&str] = &[
            "request_properties",
            "publisher_data",
            "consumer_data",
            "property_data",
            "compliance",
        ];

        for req_name in [
            "DocLeadPingRequest",
            "DocLeadPostRequest",
            "DocLeadFullpostRequest",
        ] {
            let RefOr::T(Schema::Object(obj)) = schemas.get(req_name).expect(req_name) else {
                panic!("{} is not an Object schema", req_name);
            };
            let keys: Vec<&str> = obj.properties.keys().map(String::as_str).collect();
            assert_eq!(
                keys.as_slice(),
                top_order,
                "{} top-level property order",
                req_name
            );

            let RefOr::T(Schema::Object(lead)) = obj.properties.get("lead").expect("lead") else {
                panic!("{} lead is not an Object", req_name);
            };
            let lead_keys: Vec<&str> = lead.properties.keys().map(String::as_str).collect();
            assert_eq!(
                lead_keys.as_slice(),
                lead_order,
                "{} lead property order",
                req_name
            );
        }
    }

    /// Ensures the serialized spec used for Scalar has request-body examples with key order
    /// verbose, lead (and inside lead: request_properties, publisher_data, consumer_data, property_data, compliance).
    #[test]
    fn embedded_spec_request_example_key_order() {
        let openapi = ApiDoc::openapi();
        let spec_str = serde_json::to_string(&openapi).expect("serialization");
        let modified = replace_request_body_examples(&spec_str);
        // After replacement, the ping path's request-body example must start with verbose then lead.
        assert!(
            modified.contains(r#""verbose":false,"lead":"#),
            "embedded spec should contain pre-ordered example (verbose, lead)"
        );
        assert!(
            modified.contains(PING_EXAMPLE_JSON),
            "embedded spec should contain PING_EXAMPLE_JSON verbatim"
        );
    }

    /// Ensures the embedded spec has lead.properties in doc order so the main page shows
    /// request_properties, publisher_data, consumer_data, property_data, compliance.
    #[test]
    fn embedded_spec_lead_properties_order() {
        let openapi = ApiDoc::openapi();
        let spec_str = serde_json::to_string(&openapi).expect("serialization");
        let modified = reorder_lead_properties_in_spec(&spec_str);
        // In components.schemas the lead.properties object should start with "request_properties".
        let schema_marker = "\"DocLeadFullpostRequest\"";
        let schema_pos = modified
            .find(schema_marker)
            .expect("spec has DocLeadFullpostRequest");
        let rest = &modified[schema_pos..];
        let lead_props_marker = r#""properties":{"request_properties":"#;
        assert!(
            rest.contains(lead_props_marker),
            "after reorder, lead.properties should start with request_properties"
        );
    }
}
