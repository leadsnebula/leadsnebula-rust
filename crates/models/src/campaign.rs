use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Campaign {
    pub id: Uuid,
    pub name: Option<String>,
    pub buyer_id: Uuid,
    pub publisher_id: Uuid,
    pub instance_id: Uuid,
    pub vertical: String,
    pub campaign_token: String,
    pub status: String,
    pub is_documentation_test: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
