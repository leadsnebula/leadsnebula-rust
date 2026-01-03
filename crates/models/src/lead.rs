use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Lead {
    pub uuid: Uuid,
    pub event_id: String,
    pub publisher_id: Option<Uuid>,
    pub campaign_id: Option<Uuid>,
    pub buyer_id: Option<Uuid>,
    pub vertical_id: Uuid,
    pub status: String,
    pub request_type: String,
    pub strategy: String,
    pub promise_id: Option<String>,
    pub ping_id: Option<String>,
    pub post_id: Option<String>,
    pub lead_id: Option<String>,
    pub session_id: String,
    pub vertical_data: serde_json::Value,
    pub submitted_at: Option<DateTime<Utc>>,
    pub sold_at: Option<DateTime<Utc>>,
    pub is_test: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
