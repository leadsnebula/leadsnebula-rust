use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Buyer {
    pub id: Uuid,
    pub name: String,
    pub instance_id: Uuid,
    pub vertical_id: Option<Uuid>,
    pub buyer_integration_id: Option<Uuid>,
    pub status: String,
    pub post_type: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
