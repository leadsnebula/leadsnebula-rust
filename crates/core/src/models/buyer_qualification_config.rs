use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BuyerQualificationConfig {
    pub id: Uuid,
    pub buyer_id: Uuid,
    pub vertical_id: Uuid,
    pub buyer_integration_id: Option<Uuid>,
    pub rule_set_name: String,
    #[serde(default = "default_json_object")]
    #[sqlx(default)]
    pub config: serde_json::Value,
    #[sqlx(default)]
    pub rules_order: Vec<String>,
    pub enabled: bool,
    pub is_active: bool,
    pub timeout_seconds: Option<f64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

fn default_json_object() -> serde_json::Value {
    serde_json::json!({})
}
