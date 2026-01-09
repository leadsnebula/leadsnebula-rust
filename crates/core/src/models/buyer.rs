use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::models::enums::BuyerStatus;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Buyer {
    pub id: Uuid,
    pub name: String,
    pub instance_id: Uuid,
    pub instance_user_id: Option<Uuid>,
    pub vertical_id: Option<Uuid>,
    pub buyer_integration_id: Option<Uuid>,
    pub status: BuyerStatus,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    // Additional fields from Ruby schema
    #[serde(default = "default_json_object")]
    #[sqlx(default)]
    pub contact_info: serde_json::Value,
    pub ein_tin: Option<String>,
    pub address_street: Option<String>,
    pub address_city: Option<String>,
    pub address_state: Option<String>,
    pub address_zip: Option<String>,
    pub email_address: Option<String>,
    pub representative_first_name: Option<String>,
    pub representative_last_name: Option<String>,
    #[serde(default = "default_json_array")]
    #[sqlx(default)]
    pub documents: serde_json::Value,
    #[sqlx(default)]
    pub post_type: Option<String>,
    pub buyer_type: Option<String>,
}

fn default_json_object() -> serde_json::Value {
    serde_json::json!({})
}

fn default_json_array() -> serde_json::Value {
    serde_json::json!([])
}

impl Buyer {
    pub fn active(&self) -> bool {
        matches!(self.status, BuyerStatus::Active) && self.deleted_at.is_none()
    }

    pub async fn find_by_id(pool: &sqlx::PgPool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Buyer>("SELECT * FROM buyers WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await
    }
}
